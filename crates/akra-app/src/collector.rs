//! Local-or-remote capture routing for Codex hook envelopes.
//!
//! The hook command stays stable and reads this state at invocation time. A
//! loopback endpoint preserves the existing local spool path; an external
//! endpoint durably queues an authenticated HTTPS delivery.

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use reqwest::{Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use crate::spool::{CaptureEnvelope, CaptureEnvelopeError, Spool, SpoolError, SpoolItem};

pub const COLLECTOR_PROTOCOL_VERSION: u8 = 1;
/// Dedicated loopback port for the local Akra API/collector. Keep this out of
/// the common 3000/5173 development pair so it can coexist with other apps.
pub const DEFAULT_LOCAL_COLLECTOR_PORT: u16 = 42130;
pub const DEFAULT_COLLECTOR_ENDPOINT: &str = "http://127.0.0.1:42130";
pub const REMOTE_RESULT_RETENTION_US: i64 = 24 * 60 * 60 * 1_000_000;

const CONFIG_FILE: &str = "collector.json";
const REMOTE_TOKEN_FILE: &str = "collector-remote.token";
const INGEST_TOKEN_FILE: &str = "collector-ingest.token";
const LOCK_FILE: &str = ".collector.lock";
const OUTBOX_DIRECTORY: &str = "remote-outbox";
const RETRY_DIRECTORY: &str = "remote-outbox-retry";
const RECEIPT_DIRECTORY: &str = "collector-receipts";
const MAX_CONFIG_BYTES: u64 = 16 * 1024;
const MAX_ENDPOINT_BYTES: usize = 2 * 1024;
const MAX_TOKEN_BYTES: usize = 4 * 1024;
const MAX_RECEIPT_BYTES: u64 = 1024;
const MAX_RETRY_BYTES: u64 = 1024;
const RELAY_BATCH_SIZE: usize = 8;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);
const CONFIGURATION_RETRY_DELAY: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorMode {
    Local,
    Remote,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectorEndpoint {
    url: Url,
    canonical: String,
    mode: CollectorMode,
}

impl CollectorEndpoint {
    pub fn parse(input: &str) -> Result<Self, CollectorError> {
        if input.is_empty()
            || input.len() > MAX_ENDPOINT_BYTES
            || input.chars().any(char::is_control)
        {
            return Err(CollectorError::InvalidEndpoint(
                "address must be non-empty and bounded".to_owned(),
            ));
        }
        let url = Url::parse(input).map_err(|error| {
            CollectorError::InvalidEndpoint(format!("address is not a valid URL: {error}"))
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(CollectorError::InvalidEndpoint(
                "only http and https schemes are supported".to_owned(),
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(CollectorError::InvalidEndpoint(
                "userinfo is not allowed".to_owned(),
            ));
        }
        if url.cannot_be_a_base()
            || url.host().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(CollectorError::InvalidEndpoint(
                "address must contain only scheme, host, and optional port".to_owned(),
            ));
        }
        if url.port() == Some(0) {
            return Err(CollectorError::InvalidEndpoint(
                "port zero is not a connectable destination".to_owned(),
            ));
        }

        let host = raw_authority_host(input).ok_or_else(|| {
            CollectorError::InvalidEndpoint("address has an invalid authority".to_owned())
        })?;
        let normalized_host = host.to_ascii_lowercase();
        if normalized_host == "localhost."
            || normalized_host.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(CollectorError::InvalidEndpoint(
                "address host is ambiguous".to_owned(),
            ));
        }
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        let mode = if loopback {
            CollectorMode::Local
        } else {
            CollectorMode::Remote
        };
        if mode == CollectorMode::Local && url.scheme() != "http" {
            return Err(CollectorError::InvalidEndpoint(
                "loopback collectors use http".to_owned(),
            ));
        }
        if mode == CollectorMode::Remote && url.scheme() != "https" {
            return Err(CollectorError::InsecureRemoteEndpoint);
        }
        let canonical = url.origin().ascii_serialization();
        let url = Url::parse(&canonical).map_err(|error| {
            CollectorError::InvalidEndpoint(format!("address could not be normalized: {error}"))
        })?;
        Ok(Self {
            url,
            canonical,
            mode,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    pub const fn mode(&self) -> CollectorMode {
        self.mode
    }

    fn route(&self, path: &str) -> Result<Url, CollectorError> {
        self.url
            .join(path)
            .map_err(|error| CollectorError::InvalidEndpoint(error.to_string()))
    }
}

fn raw_authority_host(input: &str) -> Option<&str> {
    let (_, remainder) = input.split_once("://")?;
    let authority = remainder.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    if let Some(bracketed) = authority.strip_prefix('[') {
        return bracketed.split_once(']').map(|(host, _)| host);
    }
    match authority.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && port.chars().all(|character| character.is_ascii_digit()) =>
        {
            Some(host)
        }
        _ => Some(authority),
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct CollectorToken(String);

impl CollectorToken {
    fn parse(value: String) -> Result<Self, CollectorError> {
        if value.is_empty()
            || value.len() > MAX_TOKEN_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
        {
            return Err(CollectorError::InvalidToken);
        }
        Ok(Self(value))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CollectorToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CollectorToken([REDACTED])")
    }
}

#[derive(Clone, Debug)]
pub struct CollectorConfig {
    endpoint: CollectorEndpoint,
    remote_token: Option<CollectorToken>,
    source_instance_id: String,
    destination_id: String,
}

impl CollectorConfig {
    pub fn view(&self) -> CollectorConfigView {
        CollectorConfigView {
            endpoint: self.endpoint.as_str().to_owned(),
            mode: self.endpoint.mode(),
            has_token: self.remote_token.is_some(),
        }
    }

    pub fn endpoint(&self) -> &CollectorEndpoint {
        &self.endpoint
    }

    pub fn source_instance_id(&self) -> &str {
        &self.source_instance_id
    }

    pub fn destination_id(&self) -> &str {
        &self.destination_id
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectorConfigInput {
    pub endpoint: String,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollectorConfigView {
    pub endpoint: String,
    pub mode: CollectorMode,
    pub has_token: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistedConfig {
    schema_version: u8,
    endpoint: String,
    /// Accept pre-release collector configuration that kept the remote token
    /// beside endpoint metadata, then migrate it into the credential file.
    #[serde(default, rename = "token", skip_serializing)]
    legacy_token: Option<String>,
    source_instance_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistedRemoteCredential {
    schema_version: u8,
    endpoint: String,
    token: String,
}

#[derive(Clone, Debug)]
pub struct CollectorStore {
    data_dir: PathBuf,
}

impl CollectorStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub fn load(&self) -> Result<CollectorConfig, CollectorError> {
        let _lock = self.lock()?;
        self.load_current_locked()
    }

    pub fn update(
        &self,
        input: CollectorConfigInput,
    ) -> Result<CollectorConfigView, CollectorError> {
        let _lock = self.lock()?;
        let current = self.load_current_locked()?;
        let endpoint = CollectorEndpoint::parse(&input.endpoint)?;
        let remote_token = match endpoint.mode() {
            CollectorMode::Local => None,
            CollectorMode::Remote => match input.token {
                Some(token) => Some(CollectorToken::parse(token)?),
                None if endpoint == current.endpoint => current.remote_token,
                None => return Err(CollectorError::RemoteTokenRequired),
            },
        };
        let persisted = PersistedConfig {
            schema_version: COLLECTOR_PROTOCOL_VERSION,
            endpoint: endpoint.as_str().to_owned(),
            legacy_token: None,
            source_instance_id: current.source_instance_id,
        };
        match endpoint.mode() {
            CollectorMode::Remote => {
                let credential = PersistedRemoteCredential {
                    schema_version: COLLECTOR_PROTOCOL_VERSION,
                    endpoint: endpoint.as_str().to_owned(),
                    token: remote_token
                        .as_ref()
                        .expect("remote collector token is required")
                        .0
                        .clone(),
                };
                // Persist credentials first. If a process stops before the
                // endpoint update, load fails closed because the credential's
                // endpoint does not match the still-current configuration.
                self.persist_remote_credential_locked(&credential)?;
                self.persist_locked(&persisted)?;
                Ok(decode_config(persisted, Some(credential))?.view())
            }
            CollectorMode::Local => {
                self.persist_locked(&persisted)?;
                self.clear_remote_credential_locked()?;
                Ok(decode_config(persisted, None)?.view())
            }
        }
    }

    pub fn load_or_create_ingest_token(&self) -> Result<CollectorToken, CollectorError> {
        let _lock = self.lock()?;
        let path = self.data_dir.join(INGEST_TOKEN_FILE);
        match bounded_read(&path, MAX_TOKEN_BYTES as u64) {
            Ok(value) => CollectorToken::parse(value.trim().to_owned()),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let value = format!(
                    "akra_collector_{}{}",
                    Uuid::new_v4().simple(),
                    Uuid::new_v4().simple()
                );
                persist_atomic(&path, value.as_bytes(), true)?;
                CollectorToken::parse(value)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn lock(&self) -> Result<File, CollectorError> {
        fs::create_dir_all(&self.data_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.data_dir.join(LOCK_FILE))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn load_current_locked(&self) -> Result<CollectorConfig, CollectorError> {
        let mut config = match self.load_config_locked()? {
            Some(config) => config,
            None => {
                let config = PersistedConfig {
                    schema_version: COLLECTOR_PROTOCOL_VERSION,
                    endpoint: DEFAULT_COLLECTOR_ENDPOINT.to_owned(),
                    legacy_token: None,
                    source_instance_id: Uuid::new_v4().to_string(),
                };
                self.persist_locked(&config)?;
                self.clear_remote_credential_locked()?;
                return decode_config(config, None);
            }
        };
        let endpoint = CollectorEndpoint::parse(&config.endpoint)?;
        let mut credential = self.load_remote_credential_locked()?;
        if let Some(token) = config.legacy_token.take() {
            if endpoint.mode() == CollectorMode::Remote {
                let migrated = PersistedRemoteCredential {
                    schema_version: COLLECTOR_PROTOCOL_VERSION,
                    endpoint: endpoint.as_str().to_owned(),
                    token,
                };
                self.persist_remote_credential_locked(&migrated)?;
                credential = Some(migrated);
            }
            self.persist_locked(&config)?;
        }
        if endpoint.mode() == CollectorMode::Local && credential.is_some() {
            self.clear_remote_credential_locked()?;
            credential = None;
        }
        decode_config(config, credential)
    }

    fn load_config_locked(&self) -> Result<Option<PersistedConfig>, CollectorError> {
        let path = self.data_dir.join(CONFIG_FILE);
        match bounded_read(&path, MAX_CONFIG_BYTES) {
            Ok(value) => Ok(Some(serde_json::from_str(&value)?)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn persist_locked(&self, config: &PersistedConfig) -> Result<(), CollectorError> {
        persist_atomic(
            &self.data_dir.join(CONFIG_FILE),
            &serde_json::to_vec_pretty(config)?,
            false,
        )
    }

    fn load_remote_credential_locked(
        &self,
    ) -> Result<Option<PersistedRemoteCredential>, CollectorError> {
        let path = self.data_dir.join(REMOTE_TOKEN_FILE);
        match bounded_read(&path, MAX_CONFIG_BYTES) {
            Ok(value) => Ok(Some(serde_json::from_str(&value)?)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn persist_remote_credential_locked(
        &self,
        credential: &PersistedRemoteCredential,
    ) -> Result<(), CollectorError> {
        persist_atomic(
            &self.data_dir.join(REMOTE_TOKEN_FILE),
            &serde_json::to_vec_pretty(credential)?,
            true,
        )
    }

    fn clear_remote_credential_locked(&self) -> Result<(), CollectorError> {
        let path = self.data_dir.join(REMOTE_TOKEN_FILE);
        match fs::remove_file(path) {
            Ok(()) => sync_directory(&self.data_dir).map_err(CollectorError::Io),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn decode_config(
    config: PersistedConfig,
    credential: Option<PersistedRemoteCredential>,
) -> Result<CollectorConfig, CollectorError> {
    if config.schema_version != COLLECTOR_PROTOCOL_VERSION
        || Uuid::parse_str(&config.source_instance_id).is_err()
    {
        return Err(CollectorError::InvalidConfig);
    }
    let endpoint = CollectorEndpoint::parse(&config.endpoint)?;
    let remote_token = match endpoint.mode() {
        CollectorMode::Local => None,
        CollectorMode::Remote => {
            let credential = credential.ok_or(CollectorError::RemoteTokenRequired)?;
            let credential_endpoint = CollectorEndpoint::parse(&credential.endpoint)?;
            if credential.schema_version != COLLECTOR_PROTOCOL_VERSION
                || credential_endpoint != endpoint
            {
                return Err(CollectorError::RemoteTokenRequired);
            }
            Some(CollectorToken::parse(credential.token)?)
        }
    };
    Ok(CollectorConfig {
        destination_id: destination_id(&endpoint),
        endpoint,
        remote_token,
        source_instance_id: config.source_instance_id,
    })
}

fn destination_id(endpoint: &CollectorEndpoint) -> String {
    let mut digest = Sha256::new();
    digest.update(b"akra-collector-destination\0");
    digest.update(endpoint.as_str().as_bytes());
    hex::encode(digest.finalize())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteCapture {
    pub protocol_version: u8,
    pub capture_id: String,
    pub source_instance_id: String,
    pub destination_id: String,
    pub envelope: CaptureEnvelope,
}

impl RemoteCapture {
    pub fn new(config: &CollectorConfig, envelope: CaptureEnvelope) -> Self {
        Self {
            protocol_version: COLLECTOR_PROTOCOL_VERSION,
            capture_id: Uuid::new_v4().to_string(),
            source_instance_id: config.source_instance_id.clone(),
            destination_id: config.destination_id.clone(),
            envelope,
        }
    }

    fn validate(&self) -> Result<(), CollectorError> {
        if self.protocol_version != COLLECTOR_PROTOCOL_VERSION
            || Uuid::parse_str(&self.capture_id).is_err()
            || Uuid::parse_str(&self.source_instance_id).is_err()
            || self.destination_id.len() != 64
            || !self
                .destination_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CollectorError::InvalidRemoteCapture);
        }
        CaptureEnvelope::decode(&serde_json::to_vec(&self.envelope)?)?;
        if self.envelope.provider() != "codex" {
            return Err(CollectorError::InvalidRemoteCapture);
        }
        let provider_payload = serde_json::to_string(self.envelope.payload())?;
        akra_adapters::codex::CodexAdapter::normalize_capture(&provider_payload)
            .map_err(|_| CollectorError::InvalidRemoteCapture)?;
        Ok(())
    }

    fn is_result(&self) -> bool {
        self.envelope
            .payload()
            .get("hook_event_name")
            .and_then(serde_json::Value::as_str)
            == Some("Stop")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "route", rename_all = "snake_case")]
pub enum CaptureRoute {
    LocalQueued,
    RemoteQueued {
        capture_id: String,
        destination_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveOutcome {
    Accepted,
    Duplicate,
}

#[derive(Clone, Debug, Serialize)]
pub struct CollectorStatus {
    pub config: CollectorConfigView,
    pub pending: usize,
    pub connected: Option<bool>,
    pub last_delivery_at_us: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RelayReport {
    pub attempted: usize,
    pub delivered: usize,
    pub blocked_destination: usize,
    pub expired_results: usize,
    pub pending: usize,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerifyReport {
    pub mode: CollectorMode,
    pub reachable: bool,
    pub status: Option<u16>,
}

pub struct CollectorManager {
    store: CollectorStore,
    local_spool: Spool,
    outbox: Spool,
    retries: PathBuf,
    receipts: PathBuf,
    ingest_token: CollectorToken,
    client: Client,
    receipt_lock: Mutex<()>,
    last_error: Mutex<Option<String>>,
    last_delivery_at_us: Mutex<Option<i64>>,
}

impl CollectorManager {
    pub fn open(data_dir: &Path) -> Result<Self, CollectorError> {
        let store = CollectorStore::new(data_dir);
        store.load()?;
        let ingest_token = store.load_or_create_ingest_token()?;
        let receipts = data_dir.join(RECEIPT_DIRECTORY);
        let retries = data_dir.join(RETRY_DIRECTORY);
        fs::create_dir_all(&receipts)?;
        fs::create_dir_all(&retries)?;
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .build()?;
        Ok(Self {
            store,
            local_spool: Spool::open(&data_dir.join("spool"))?,
            outbox: Spool::open(&data_dir.join(OUTBOX_DIRECTORY))?,
            retries,
            receipts,
            ingest_token,
            client,
            receipt_lock: Mutex::new(()),
            last_error: Mutex::new(None),
            last_delivery_at_us: Mutex::new(None),
        })
    }

    pub fn config(&self) -> Result<CollectorConfigView, CollectorError> {
        Ok(self.store.load()?.view())
    }

    pub fn configure(
        &self,
        input: CollectorConfigInput,
    ) -> Result<CollectorConfigView, CollectorError> {
        let config = self.store.update(input)?;
        // A new token or endpoint is an explicit operator action. Clear the
        // retry schedule so the retained outbox can be considered immediately
        // (items for a different destination remain destination-bound).
        self.clear_retry_records()?;
        self.set_last_error(None)?;
        Ok(config)
    }

    pub fn collector_token(&self) -> CollectorToken {
        self.ingest_token.clone()
    }

    pub fn authenticate(&self, presented: &str) -> bool {
        constant_time_equal(
            self.ingest_token.expose_secret().as_bytes(),
            presented.as_bytes(),
        )
    }

    pub fn capture(&self, envelope: &CaptureEnvelope) -> Result<CaptureRoute, CollectorError> {
        let config = self.store.load()?;
        match config.endpoint.mode() {
            CollectorMode::Local => {
                self.local_spool.enqueue_envelope(envelope)?;
                Ok(CaptureRoute::LocalQueued)
            }
            CollectorMode::Remote => {
                let capture = RemoteCapture::new(&config, envelope.clone());
                let payload = serde_json::to_vec(&capture)?;
                self.outbox.enqueue_marked(&payload, capture.is_result())?;
                Ok(CaptureRoute::RemoteQueued {
                    capture_id: capture.capture_id,
                    destination_id: capture.destination_id,
                })
            }
        }
    }

    pub fn status(&self) -> Result<CollectorStatus, CollectorError> {
        let config = self.config()?;
        let pending = self.outbox.pending()?.len();
        let last_error = if config.mode == CollectorMode::Local && pending > 0 {
            Some("Queued captures belong to a previous collector destination.".to_owned())
        } else {
            self.last_error()?
        };
        Ok(CollectorStatus {
            connected: match config.mode {
                CollectorMode::Local => Some(true),
                CollectorMode::Remote if last_error.is_some() => Some(false),
                CollectorMode::Remote => self.last_delivery_at_us()?.map(|_| true),
            },
            last_delivery_at_us: self.last_delivery_at_us()?,
            config,
            pending,
            last_error,
        })
    }

    pub async fn relay_once(&self) -> Result<RelayReport, CollectorError> {
        let now = now_us()?;
        let expired_results = self
            .outbox
            .expire_result_items(now, REMOTE_RESULT_RETENTION_US)?;
        self.clear_orphaned_retry_records()?;
        let config = self.store.load()?;
        let mut report = RelayReport {
            expired_results,
            ..RelayReport::default()
        };
        if config.endpoint.mode() == CollectorMode::Local {
            report.pending = self.outbox.pending()?.len();
            report.last_error = self.last_error()?;
            return Ok(report);
        }
        let token = config
            .remote_token
            .as_ref()
            .ok_or(CollectorError::RemoteTokenRequired)?;
        let ingest_url = config.endpoint.route("v1/collector/ingest")?;
        let mut last_error = self.last_error()?;
        let candidates = self.relay_candidates(
            self.outbox.pending()?,
            &config,
            now,
            &mut report,
            &mut last_error,
        )?;
        for (item, capture) in candidates {
            report.attempted += 1;
            match self
                .client
                .post(ingest_url.clone())
                .bearer_auth(token.expose_secret())
                .json(&capture)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    self.clear_retry(&item)?;
                    self.outbox.acknowledge(item)?;
                    report.delivered += 1;
                    last_error = None;
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let retry_after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok());
                    self.schedule_retry(
                        &item,
                        now,
                        response_retry_delay(status, retry_after, self.retry_attempts(&item)?),
                    )?;
                    last_error = Some(match status {
                        401 | 403 => "Collector access token was rejected.".to_owned(),
                        404 => "Collector endpoint did not accept captures.".to_owned(),
                        _ => "Collector did not accept the capture yet.".to_owned(),
                    });
                }
                Err(_) => {
                    self.schedule_retry(
                        &item,
                        now,
                        exponential_retry_delay(self.retry_attempts(&item)?),
                    )?;
                    last_error = Some("Collector did not respond in time.".to_owned());
                }
            }
        }
        if report.blocked_destination > 0 && last_error.is_none() {
            last_error =
                Some("Queued captures belong to a previous collector destination.".to_owned());
        }
        self.set_last_error(last_error.clone())?;
        if report.delivered > 0 {
            self.set_last_delivery_at_us(Some(now_us()?))?;
        }
        report.pending = self.outbox.pending()?.len();
        report.last_error = last_error;
        Ok(report)
    }

    fn relay_candidates(
        &self,
        items: Vec<SpoolItem>,
        config: &CollectorConfig,
        now: i64,
        report: &mut RelayReport,
        last_error: &mut Option<String>,
    ) -> Result<Vec<(SpoolItem, RemoteCapture)>, CollectorError> {
        let mut candidates = Vec::with_capacity(RELAY_BATCH_SIZE);
        for item in items {
            if candidates.len() == RELAY_BATCH_SIZE {
                break;
            }
            if !self.retry_is_due(&item, now)? {
                continue;
            }
            let payload = self.outbox.read(&item)?;
            let capture: RemoteCapture = match serde_json::from_slice(&payload) {
                Ok(capture) => capture,
                Err(_) => {
                    self.outbox.defer(&item)?;
                    *last_error = Some("A queued collector capture is invalid.".to_owned());
                    continue;
                }
            };
            if capture.validate().is_err() {
                self.outbox.defer(&item)?;
                *last_error = Some("A queued collector capture is invalid.".to_owned());
                continue;
            }
            if capture.destination_id != config.destination_id {
                report.blocked_destination += 1;
                continue;
            }
            candidates.push((item, capture));
        }
        Ok(candidates)
    }

    pub async fn verify(&self) -> Result<VerifyReport, CollectorError> {
        let config = self.store.load()?;
        if config.endpoint.mode() == CollectorMode::Local {
            self.set_last_error(None)?;
            return Ok(VerifyReport {
                mode: CollectorMode::Local,
                reachable: true,
                status: None,
            });
        }
        let token = config
            .remote_token
            .as_ref()
            .ok_or(CollectorError::RemoteTokenRequired)?;
        let response = self
            .client
            .get(config.endpoint.route("v1/collector/verify")?)
            .bearer_auth(token.expose_secret())
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.set_last_error(Some("Collector did not respond in time.".to_owned()))?;
                return Err(error.into());
            }
        };
        let reachable = response.status().is_success();
        self.set_last_error((!reachable).then(|| match response.status().as_u16() {
            401 | 403 => "Collector access token was rejected.".to_owned(),
            _ => "Collector did not accept the configured connection check.".to_owned(),
        }))?;
        Ok(VerifyReport {
            mode: CollectorMode::Remote,
            reachable,
            status: Some(response.status().as_u16()),
        })
    }

    pub fn receive_authenticated(
        &self,
        presented: &str,
        capture: RemoteCapture,
    ) -> Result<ReceiveOutcome, CollectorError> {
        if !self.authenticate(presented) {
            return Err(CollectorError::Unauthorized);
        }
        self.receive(capture)
    }

    pub fn receive(&self, capture: RemoteCapture) -> Result<ReceiveOutcome, CollectorError> {
        capture.validate()?;
        let bytes = serde_json::to_vec(&capture)?;
        let digest = hex::encode(Sha256::digest(&bytes));
        let _guard = self
            .receipt_lock
            .lock()
            .map_err(|_| CollectorError::StatePoisoned)?;
        let receipt_path = self.receipts.join(format!("{}.json", capture.capture_id));
        match read_receipt(&receipt_path) {
            Ok(receipt) if receipt.digest == digest => return Ok(ReceiveOutcome::Duplicate),
            Ok(_) => return Err(CollectorError::CaptureConflict),
            Err(CollectorError::Io(error)) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let is_result = capture.is_result();
        let envelope = capture
            .envelope
            .into_remote_namespace(&capture.source_instance_id)?;
        self.local_spool
            .enqueue_marked(&serde_json::to_vec(&envelope)?, is_result)?;
        persist_atomic(
            &receipt_path,
            &serde_json::to_vec(&Receipt { digest })?,
            false,
        )?;
        Ok(ReceiveOutcome::Accepted)
    }

    fn retry_path(&self, item: &SpoolItem) -> Result<PathBuf, CollectorError> {
        let name = item
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(CollectorError::InvalidConfig)?;
        Ok(self.retries.join(format!("{name}.retry")))
    }

    fn retry_record(&self, item: &SpoolItem) -> Result<Option<RetryRecord>, CollectorError> {
        let path = self.retry_path(item)?;
        match bounded_read(&path, MAX_RETRY_BYTES) {
            Ok(value) => match serde_json::from_str(&value) {
                Ok(record) => Ok(Some(record)),
                Err(_) => {
                    // Retry metadata is local, derived state. It never carries a
                    // capture or credential, so an interrupted write can be
                    // discarded safely and the durable outbox remains intact.
                    fs::remove_file(path)?;
                    Ok(None)
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn retry_attempts(&self, item: &SpoolItem) -> Result<u8, CollectorError> {
        Ok(self.retry_record(item)?.map_or(0, |record| record.attempts))
    }

    fn retry_is_due(&self, item: &SpoolItem, now: i64) -> Result<bool, CollectorError> {
        Ok(self
            .retry_record(item)?
            .is_none_or(|record| record.retry_after_us <= now))
    }

    fn schedule_retry(
        &self,
        item: &SpoolItem,
        now: i64,
        delay: Duration,
    ) -> Result<(), CollectorError> {
        let attempts = self.retry_attempts(item)?.saturating_add(1);
        let delay_us = i64::try_from(delay.as_micros()).unwrap_or(i64::MAX);
        let record = RetryRecord {
            attempts,
            retry_after_us: now.saturating_add(delay_us),
        };
        persist_atomic(
            &self.retry_path(item)?,
            &serde_json::to_vec(&record)?,
            false,
        )
    }

    fn clear_retry(&self, item: &SpoolItem) -> Result<(), CollectorError> {
        match fs::remove_file(self.retry_path(item)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn clear_retry_records(&self) -> Result<(), CollectorError> {
        for entry in fs::read_dir(&self.retries)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".retry"))
            {
                fs::remove_file(path)?;
            }
        }
        sync_directory(&self.retries)?;
        Ok(())
    }

    fn clear_orphaned_retry_records(&self) -> Result<(), CollectorError> {
        for entry in fs::read_dir(&self.retries)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(pending_name) = name.strip_suffix(".retry") else {
                continue;
            };
            if !self.outbox_path(pending_name).exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    fn outbox_path(&self, file_name: &str) -> PathBuf {
        // All retry names are derived from a filename obtained from SpoolItem,
        // but do not permit a malformed marker to escape the outbox directory.
        self.outbox_directory().join(file_name)
    }

    fn outbox_directory(&self) -> PathBuf {
        self.retries
            .parent()
            .expect("retry directory is under the data directory")
            .join(OUTBOX_DIRECTORY)
    }

    fn last_error(&self) -> Result<Option<String>, CollectorError> {
        self.last_error
            .lock()
            .map(|value| value.clone())
            .map_err(|_| CollectorError::StatePoisoned)
    }

    fn set_last_error(&self, error: Option<String>) -> Result<(), CollectorError> {
        *self
            .last_error
            .lock()
            .map_err(|_| CollectorError::StatePoisoned)? = error;
        Ok(())
    }

    fn last_delivery_at_us(&self) -> Result<Option<i64>, CollectorError> {
        self.last_delivery_at_us
            .lock()
            .map(|value| *value)
            .map_err(|_| CollectorError::StatePoisoned)
    }

    fn set_last_delivery_at_us(&self, value: Option<i64>) -> Result<(), CollectorError> {
        *self
            .last_delivery_at_us
            .lock()
            .map_err(|_| CollectorError::StatePoisoned)? = value;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct RetryRecord {
    attempts: u8,
    retry_after_us: i64,
}

#[derive(Deserialize, Serialize)]
struct Receipt {
    digest: String,
}

fn read_receipt(path: &Path) -> Result<Receipt, CollectorError> {
    Ok(serde_json::from_str(&bounded_read(
        path,
        MAX_RECEIPT_BYTES,
    )?)?)
}

fn constant_time_equal(expected: &[u8], presented: &[u8]) -> bool {
    let mut difference = expected.len() ^ presented.len();
    for index in 0..expected.len().max(presented.len()) {
        difference |= usize::from(
            expected.get(index).copied().unwrap_or_default()
                ^ presented.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn exponential_retry_delay(attempts: u8) -> Duration {
    let shift = u32::from(attempts.saturating_sub(1).min(8));
    Duration::from_secs(1_u64 << shift).min(MAX_RETRY_DELAY)
}

fn response_retry_delay(status: u16, retry_after_seconds: Option<u64>, attempts: u8) -> Duration {
    match status {
        400 | 401 | 403 | 404 | 413 | 422 => CONFIGURATION_RETRY_DELAY,
        429 => retry_after_seconds
            .map(Duration::from_secs)
            .unwrap_or_else(|| exponential_retry_delay(attempts))
            .min(MAX_RETRY_DELAY),
        _ => exponential_retry_delay(attempts),
    }
}

fn now_us() -> Result<i64, CollectorError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CollectorError::Clock)?;
    i64::try_from(elapsed.as_micros()).map_err(|_| CollectorError::Clock)
}

fn bounded_read(path: &Path, maximum: u64) -> io::Result<String> {
    let initial_metadata = fs::symlink_metadata(path)?;
    if !initial_metadata.file_type().is_file() || initial_metadata.len() > maximum {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "file is invalid or oversized",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.file_type().is_file() || opened_metadata.len() > maximum {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "file is invalid or oversized",
        ));
    }
    let mut value = String::new();
    file.take(maximum.saturating_add(1))
        .read_to_string(&mut value)?;
    if value.len() as u64 > maximum {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "file is invalid or oversized",
        ));
    }
    Ok(value)
}

fn persist_atomic(path: &Path, payload: &[u8], secret: bool) -> Result<(), CollectorError> {
    let parent = path.parent().ok_or(CollectorError::MissingParent)?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(payload)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| CollectorError::Io(error.error))?;
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = secret;
    sync_directory(parent)?;
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    let result = options
        .open(path)
        .and_then(|directory| directory.sync_all());
    #[cfg(windows)]
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == ErrorKind::PermissionDenied)
    {
        return Ok(());
    }
    result
}

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("collector address is invalid: {0}")]
    InvalidEndpoint(String),
    #[error("external collectors require HTTPS")]
    InsecureRemoteEndpoint,
    #[error("external collectors require an access token")]
    RemoteTokenRequired,
    #[error("collector token is invalid")]
    InvalidToken,
    #[error("collector configuration is invalid")]
    InvalidConfig,
    #[error("remote capture is invalid")]
    InvalidRemoteCapture,
    #[error("collector authentication failed")]
    Unauthorized,
    #[error("capture id was reused with different content")]
    CaptureConflict,
    #[error("collector state lock is unavailable")]
    StatePoisoned,
    #[error("system clock is unavailable")]
    Clock,
    #[error("collector path has no parent")]
    MissingParent,
    #[error("collector filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("collector JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("collector HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Spool(#[from] SpoolError),
    #[error(transparent)]
    Envelope(#[from] CaptureEnvelopeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use akra_git::{ProjectOriginKind, ProjectOriginSnapshot};
    use serde_json::json;
    use tempfile::TempDir;

    fn envelope(prompt: &str) -> CaptureEnvelope {
        CaptureEnvelope::new_with_source(
            "codex",
            42,
            ProjectOriginSnapshot {
                identity: "source-project".to_owned(),
                kind: ProjectOriginKind::Git,
                display_path: PathBuf::from("C:/source/project"),
            },
            json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "session",
                "turn_id": "turn",
                "cwd": "C:/source/project",
                "prompt": prompt,
            }),
            "windows-native",
            "app",
        )
        .expect("envelope")
    }

    #[test]
    fn endpoint_policy_accepts_exact_loopback_and_requires_remote_https() {
        for address in [
            "http://127.0.0.1:3000",
            "http://127.42.0.1:3000",
            "http://[::1]:3000",
            "http://localhost:3000",
            "https://collector.example",
        ] {
            CollectorEndpoint::parse(address).unwrap_or_else(|error| panic!("{address}: {error}"));
        }
        for address in [
            "http://collector.example",
            "http://127.0.0.1.evil.example:3000",
            "http://localhost.evil.example:3000",
            "http://2130706433:3000",
            "https://localhost.",
            "https://2130706433",
            "https://user@collector.example",
            "https://collector.example/path",
            "https://collector.example?token=x",
            "https://collector.example#fragment",
            "https://collector.example:0",
        ] {
            assert!(
                CollectorEndpoint::parse(address).is_err(),
                "accepted {address}"
            );
        }
    }

    #[test]
    fn endpoint_normalization_keeps_a_destination_identity_stable() {
        let first = CollectorEndpoint::parse("HTTPS://Collector.Example:443/")
            .expect("normalized external endpoint");
        let second = CollectorEndpoint::parse("https://collector.example")
            .expect("canonical external endpoint");

        assert_eq!(first.as_str(), "https://collector.example");
        assert_eq!(first, second);
        assert_eq!(destination_id(&first), destination_id(&second));
    }

    #[test]
    fn config_is_atomic_stable_and_api_view_never_serializes_token() {
        let directory = TempDir::new().expect("state");
        let store = CollectorStore::new(directory.path());
        let first = store.load().expect("first config");
        let second = store.load().expect("second config");
        assert_eq!(first.source_instance_id(), second.source_instance_id());

        let view = store
            .update(CollectorConfigInput {
                endpoint: "https://collector.example".to_owned(),
                token: Some("top-secret-token".to_owned()),
            })
            .expect("remote config");
        let response = serde_json::to_string(&view).expect("view JSON");
        assert!(view.has_token);
        assert!(!response.contains("top-secret-token"));
        assert!(!format!("{:?}", store.load().expect("saved config")).contains("top-secret-token"));
        assert!(
            !std::fs::read_to_string(directory.path().join(CONFIG_FILE))
                .expect("collector config")
                .contains("top-secret-token"),
            "endpoint configuration is not a credential store"
        );
        assert!(
            std::fs::read_to_string(directory.path().join(REMOTE_TOKEN_FILE))
                .expect("credential file")
                .contains("top-secret-token")
        );
    }

    #[test]
    fn legacy_inline_token_is_migrated_to_the_credential_file() {
        let directory = TempDir::new().expect("state");
        std::fs::write(
            directory.path().join(CONFIG_FILE),
            serde_json::to_vec(&json!({
                "schema_version": COLLECTOR_PROTOCOL_VERSION,
                "endpoint": "https://collector.example",
                "token": "legacy-secret",
                "source_instance_id": Uuid::new_v4().to_string(),
            }))
            .expect("legacy config"),
        )
        .expect("write legacy config");

        let config = CollectorStore::new(directory.path())
            .load()
            .expect("migrated config");
        assert!(config.view().has_token);
        assert!(
            std::fs::read_to_string(directory.path().join(REMOTE_TOKEN_FILE))
                .expect("credential file")
                .contains("legacy-secret")
        );
        assert!(
            !std::fs::read_to_string(directory.path().join(CONFIG_FILE))
                .expect("migrated config")
                .contains("legacy-secret")
        );
    }

    #[test]
    fn ingest_token_is_stable_and_debug_redacted() {
        let directory = TempDir::new().expect("state");
        let store = CollectorStore::new(directory.path());
        let first = store.load_or_create_ingest_token().expect("token");
        let second = store.load_or_create_ingest_token().expect("same token");
        assert_eq!(first, second);
        assert!(!format!("{first:?}").contains(first.expose_secret()));
    }

    #[tokio::test]
    async fn outbox_items_are_bound_to_the_configured_destination() {
        let directory = TempDir::new().expect("state");
        let manager = CollectorManager::open(directory.path()).expect("manager");
        manager
            .configure(CollectorConfigInput {
                endpoint: "https://first.example".to_owned(),
                token: Some("first-token".to_owned()),
            })
            .expect("first destination");
        manager.capture(&envelope("queued")).expect("queued");
        manager
            .configure(CollectorConfigInput {
                endpoint: "https://second.example".to_owned(),
                token: Some("second-token".to_owned()),
            })
            .expect("second destination");

        let report = manager.relay_once().await.expect("relay report");
        assert_eq!(report.attempted, 0);
        assert_eq!(report.blocked_destination, 1);
        assert_eq!(report.pending, 1);

        manager
            .configure(CollectorConfigInput {
                endpoint: DEFAULT_COLLECTOR_ENDPOINT.to_owned(),
                token: None,
            })
            .expect("switch back to local");
        let status = manager.status().expect("local status");
        assert_eq!(status.pending, 1);
        assert_eq!(
            status.last_error.as_deref(),
            Some("Queued captures belong to a previous collector destination.")
        );
    }

    #[test]
    fn retry_delayed_items_do_not_consume_the_relay_batch() {
        let directory = TempDir::new().expect("state");
        let manager = CollectorManager::open(directory.path()).expect("manager");
        manager
            .configure(CollectorConfigInput {
                endpoint: "https://collector.example".to_owned(),
                token: Some("token".to_owned()),
            })
            .expect("remote config");
        for index in 0..=RELAY_BATCH_SIZE {
            manager
                .capture(&envelope(&format!("queued-{index}")))
                .expect("queued capture");
        }

        let items = manager.outbox.pending().expect("pending captures");
        let now = now_us().expect("clock");
        for item in &items[..RELAY_BATCH_SIZE] {
            manager
                .schedule_retry(item, now, MAX_RETRY_DELAY)
                .expect("retry schedule");
        }
        let expected = serde_json::from_slice::<RemoteCapture>(
            &manager
                .outbox
                .read(items.last().expect("due capture"))
                .expect("due payload"),
        )
        .expect("due capture");
        let config = manager.store.load().expect("config");
        let mut report = RelayReport::default();
        let mut last_error = None;

        let candidates = manager
            .relay_candidates(items, &config, now, &mut report, &mut last_error)
            .expect("relay candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1.capture_id, expected.capture_id);
    }

    #[test]
    fn previous_destination_items_do_not_consume_the_relay_batch() {
        let directory = TempDir::new().expect("state");
        let manager = CollectorManager::open(directory.path()).expect("manager");
        manager
            .configure(CollectorConfigInput {
                endpoint: "https://first.example".to_owned(),
                token: Some("first-token".to_owned()),
            })
            .expect("first destination");
        for index in 0..RELAY_BATCH_SIZE {
            manager
                .capture(&envelope(&format!("old-{index}")))
                .expect("old capture");
        }
        manager
            .configure(CollectorConfigInput {
                endpoint: "https://second.example".to_owned(),
                token: Some("second-token".to_owned()),
            })
            .expect("second destination");
        manager
            .capture(&envelope("current"))
            .expect("current capture");

        let config = manager.store.load().expect("config");
        let mut blocked = Vec::new();
        let mut current = Vec::new();
        for item in manager.outbox.pending().expect("pending captures") {
            let capture = serde_json::from_slice::<RemoteCapture>(
                &manager.outbox.read(&item).expect("queued payload"),
            )
            .expect("queued capture");
            if capture.destination_id == config.destination_id {
                current.push(item);
            } else {
                blocked.push(item);
            }
        }
        assert_eq!(blocked.len(), RELAY_BATCH_SIZE);
        assert_eq!(current.len(), 1);
        blocked.extend(current);
        let mut report = RelayReport::default();
        let mut last_error = None;

        let candidates = manager
            .relay_candidates(
                blocked,
                &config,
                now_us().expect("clock"),
                &mut report,
                &mut last_error,
            )
            .expect("relay candidates");

        assert_eq!(report.blocked_destination, RELAY_BATCH_SIZE);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1.destination_id, config.destination_id);
    }

    #[test]
    fn retry_schedule_is_bounded_and_a_configuration_change_unblocks_it() {
        let directory = TempDir::new().expect("state");
        let manager = CollectorManager::open(directory.path()).expect("manager");
        manager
            .configure(CollectorConfigInput {
                endpoint: "https://collector.example".to_owned(),
                token: Some("first-token".to_owned()),
            })
            .expect("remote config");
        manager.capture(&envelope("queued")).expect("queued");
        let item = manager.outbox.pending().expect("pending").remove(0);
        let now = now_us().expect("clock");

        manager
            .schedule_retry(&item, now, MAX_RETRY_DELAY)
            .expect("retry schedule");
        assert!(!manager.retry_is_due(&item, now).expect("retry state"));
        assert_eq!(
            response_retry_delay(429, Some(60 * 60), 1),
            MAX_RETRY_DELAY,
            "Retry-After cannot create an unbounded pause"
        );

        manager
            .configure(CollectorConfigInput {
                endpoint: "https://collector.example".to_owned(),
                token: Some("rotated-token".to_owned()),
            })
            .expect("token rotation");
        assert!(
            manager
                .retry_is_due(&item, now)
                .expect("cleared retry state")
        );
    }

    #[test]
    fn receipts_deduplicate_and_conflicting_reuse_is_rejected() {
        let directory = TempDir::new().expect("state");
        let manager = CollectorManager::open(directory.path()).expect("manager");
        let config = manager.store.load().expect("config");
        let capture = RemoteCapture::new(&config, envelope("first"));

        assert_eq!(
            manager.receive(capture.clone()).expect("accepted"),
            ReceiveOutcome::Accepted
        );
        assert_eq!(
            manager.receive(capture.clone()).expect("duplicate"),
            ReceiveOutcome::Duplicate
        );
        let mut conflict = RemoteCapture::new(&config, envelope("different"));
        conflict.capture_id = capture.capture_id;
        assert!(matches!(
            manager.receive(conflict),
            Err(CollectorError::CaptureConflict)
        ));

        let payloads = manager.local_spool.drain().expect("local spool");
        assert_eq!(payloads.len(), 1);
        let stored = CaptureEnvelope::decode(&payloads[0]).expect("stored envelope");
        assert_eq!(stored.capture_source(), None);
        assert!(stored.origin().identity.starts_with("remote:"));
        assert!(
            stored.payload()["session_id"]
                .as_str()
                .expect("session")
                .starts_with("remote:")
        );
    }
}
