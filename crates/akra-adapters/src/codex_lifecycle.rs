use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[path = "codex_lifecycle/transaction.rs"]
mod transaction;
#[path = "codex_lifecycle/trust.rs"]
mod trust;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const CAPTURE_HOOK_TIMEOUT_SECONDS: u64 = 5;

#[derive(Clone, Debug)]
pub struct CodexHookLifecycle {
    manifest_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexHookCommand {
    command: String,
    command_windows: Option<String>,
}

impl CodexHookCommand {
    pub fn same(command: impl Into<String>) -> Self {
        let command = command.into();
        Self {
            command_windows: Some(command.clone()),
            command,
        }
    }

    pub fn posix(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            command_windows: None,
        }
    }

    pub fn with_windows(command: impl Into<String>, command_windows: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            command_windows: Some(command_windows.into()),
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

struct CodexHookUpdate {
    lifecycle: CodexHookLifecycle,
    command: Option<CodexHookCommand>,
}

/// Applies per-home hook intent as one filesystem transaction. This is used when
/// Windows and WSL installations need different capture commands but must still
/// roll back together on a malformed or concurrently changed manifest.
pub fn apply_codex_hook_updates(
    updates: impl IntoIterator<Item = (PathBuf, Option<CodexHookCommand>)>,
) -> Result<(), CodexLifecycleError> {
    let mut seen = BTreeSet::new();
    let updates = updates
        .into_iter()
        .map(|(home, command)| (normalize_home(home), command))
        .filter(|(home, _)| seen.insert(home_key(home)))
        .map(|(home, command)| CodexHookUpdate {
            lifecycle: CodexHookLifecycle::from_codex_home(&home),
            command,
        })
        .collect::<Vec<_>>();
    transaction::apply_updates(&updates)
}

impl CodexHookLifecycle {
    pub fn new(home: &Path) -> Self {
        Self::from_codex_home(&home.join(".codex"))
    }

    pub fn from_codex_home(codex_home: &Path) -> Self {
        Self {
            manifest_path: codex_home.join("hooks.json"),
        }
    }

    pub fn enable(&self, command: &str) -> Result<(), CodexLifecycleError> {
        transaction::apply_updates(&[CodexHookUpdate {
            lifecycle: self.clone(),
            command: Some(CodexHookCommand::same(command)),
        }])
    }

    pub fn disable(&self) -> Result<(), CodexLifecycleError> {
        transaction::apply_updates(&[CodexHookUpdate {
            lifecycle: self.clone(),
            command: None,
        }])
    }

    pub fn is_enabled(&self) -> Result<bool, CodexLifecycleError> {
        let hooks = self.read_hooks()?;
        Ok(hooks
            .hooks
            .user_prompt_submit
            .iter()
            .flat_map(|group| &group.hooks)
            .any(CodexHook::is_akra_hook))
    }

    fn managed_command(&self) -> Result<Option<CodexHookCommand>, CodexLifecycleError> {
        let hooks = self.read_hooks()?;
        Ok(hooks
            .hooks
            .user_prompt_submit
            .iter()
            .flat_map(|group| &group.hooks)
            .find(|hook| hook.is_akra_hook())
            .map(|hook| CodexHookCommand {
                command: hook.command.clone(),
                command_windows: hook.command_windows.clone(),
            }))
    }

    fn read_hooks(&self) -> Result<CodexHooksFile, CodexLifecycleError> {
        Ok(self.read_snapshot()?.hooks)
    }

    fn read_snapshot(&self) -> Result<ManifestSnapshot, CodexLifecycleError> {
        match read_manifest_bytes(&self.manifest_path)? {
            Some(content) => {
                let hooks = serde_json::from_slice(&content).map_err(std::io::Error::other)?;
                Ok(ManifestSnapshot {
                    original: Some(content),
                    hooks,
                })
            }
            None => Ok(ManifestSnapshot {
                original: None,
                hooks: CodexHooksFile::default(),
            }),
        }
    }

    fn config_path(&self) -> Result<PathBuf, CodexLifecycleError> {
        self.manifest_path
            .parent()
            .map(|parent| parent.join("config.toml"))
            .ok_or(CodexLifecycleError::MissingManifestParent)
    }
}

struct ManifestSnapshot {
    original: Option<Vec<u8>>,
    hooks: CodexHooksFile,
}

#[derive(Debug)]
pub struct CodexHookLifecycleSet {
    lifecycles: Vec<CodexHookLifecycle>,
}

impl CodexHookLifecycleSet {
    pub fn from_codex_homes(homes: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut unique_homes = BTreeSet::new();
        let lifecycles = homes
            .into_iter()
            .map(normalize_home)
            .filter(|home| unique_homes.insert(home_key(home)))
            .map(|home| CodexHookLifecycle::from_codex_home(&home))
            .collect();
        Self { lifecycles }
    }

    pub fn enable(&self, command: &str) -> Result<(), CodexLifecycleError> {
        transaction::enable(self, command)
    }

    pub fn disable(&self) -> Result<(), CodexLifecycleError> {
        transaction::disable(self)
    }

    pub fn is_enabled(&self) -> Result<bool, CodexLifecycleError> {
        transaction::is_enabled(self)
    }

    pub fn managed_command(&self) -> Result<Option<CodexHookCommand>, CodexLifecycleError> {
        transaction::managed_command(self)
    }
}

fn normalize_home(home: PathBuf) -> PathBuf {
    let absolute = if home.is_absolute() {
        home
    } else {
        std::env::current_dir()
            .map(|current| current.join(&home))
            .unwrap_or(home)
    };
    let normalized = normalize_lexically(&absolute);
    normalized.canonicalize().unwrap_or_else(|_| {
        normalized
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| normalized.file_name().map(|name| parent.join(name)))
            .unwrap_or(normalized)
    })
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(windows)]
fn home_key(home: &Path) -> PathBuf {
    PathBuf::from(home.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn home_key(home: &Path) -> PathBuf {
    home.to_path_buf()
}

fn read_manifest_bytes(path: &Path) -> Result<Option<Vec<u8>>, CodexLifecycleError> {
    read_regular_file_bytes(
        path,
        MAX_MANIFEST_BYTES,
        CodexLifecycleError::UnsafeManifest,
        CodexLifecycleError::OversizedManifest,
    )
}

fn read_config_bytes(path: &Path) -> Result<Option<Vec<u8>>, CodexLifecycleError> {
    read_regular_file_bytes(
        path,
        MAX_CONFIG_BYTES,
        CodexLifecycleError::UnsafeConfig,
        CodexLifecycleError::OversizedConfig,
    )
}

fn read_regular_file_bytes(
    path: &Path,
    max_bytes: u64,
    unsafe_error: fn(PathBuf) -> CodexLifecycleError,
    oversized_error: fn(u64) -> CodexLifecycleError,
) -> Result<Option<Vec<u8>>, CodexLifecycleError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(unsafe_error(path.to_path_buf()));
    }
    if metadata.len() > max_bytes {
        return Err(oversized_error(metadata.len()));
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        if !is_wsl_unc(path) {
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(unsafe_error(path.to_path_buf()));
    }
    let mut content = Vec::new();
    file.by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut content)?;
    if content.len() as u64 > max_bytes {
        return Err(oversized_error(content.len() as u64));
    }
    Ok(Some(content))
}

fn is_wsl_unc(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = path.to_string_lossy().to_ascii_lowercase();
        path.starts_with(r"\\wsl.localhost\") || path.starts_with(r"\\?\unc\wsl.localhost\")
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum HookEvent {
    UserPromptSubmit,
    SubagentStart,
}

impl HookEvent {
    const fn trust_label(self) -> &'static str {
        match self {
            Self::UserPromptSubmit => "user_prompt_submit",
            Self::SubagentStart => "subagent_start",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct HookLocation {
    event: HookEvent,
    group: usize,
    handler: usize,
}

impl HookLocation {
    const fn new(event: HookEvent, group: usize, handler: usize) -> Self {
        Self {
            event,
            group,
            handler,
        }
    }
}

fn remove_akra_hooks(hooks: &mut CodexHooksFile) -> Vec<HookLocation> {
    let mut locations = Vec::new();
    remove_akra_event_hooks(
        HookEvent::UserPromptSubmit,
        &mut hooks.hooks.user_prompt_submit,
        &mut locations,
    );
    remove_akra_event_hooks(
        HookEvent::SubagentStart,
        &mut hooks.hooks.subagent_start,
        &mut locations,
    );
    locations
}

fn remove_akra_event_hooks(
    event: HookEvent,
    groups: &mut Vec<CodexMatcherGroup>,
    locations: &mut Vec<HookLocation>,
) {
    for (group_index, group) in groups.iter_mut().enumerate() {
        let mut retained = Vec::with_capacity(group.hooks.len());
        for (handler_index, hook) in group.hooks.drain(..).enumerate() {
            if hook.is_akra_hook() {
                locations.push(HookLocation::new(event, group_index, handler_index));
            } else {
                retained.push(hook);
            }
        }
        group.hooks = retained;
    }
    groups.retain(|group| !group.hooks.is_empty());
}

fn append_akra_hooks(hooks: &mut CodexHooksFile, command: &CodexHookCommand) -> Vec<HookLocation> {
    let user_prompt = HookLocation::new(
        HookEvent::UserPromptSubmit,
        hooks.hooks.user_prompt_submit.len(),
        0,
    );
    hooks
        .hooks
        .user_prompt_submit
        .push(CodexMatcherGroup::akra_hook(command));
    let subagent = HookLocation::new(
        HookEvent::SubagentStart,
        hooks.hooks.subagent_start.len(),
        0,
    );
    hooks
        .hooks
        .subagent_start
        .push(CodexMatcherGroup::akra_hook(command));
    vec![user_prompt, subagent]
}

#[derive(Default, Deserialize, Serialize)]
struct CodexHooksFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default)]
    hooks: CodexHookEvents,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Default, Deserialize, Serialize)]
struct CodexHookEvents {
    #[serde(rename = "UserPromptSubmit", default)]
    user_prompt_submit: Vec<CodexMatcherGroup>,
    #[serde(rename = "SubagentStart", default)]
    subagent_start: Vec<CodexMatcherGroup>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize, Serialize)]
struct CodexMatcherGroup {
    #[serde(default)]
    hooks: Vec<CodexHook>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl CodexMatcherGroup {
    fn akra_hook(command: &CodexHookCommand) -> Self {
        Self {
            hooks: vec![CodexHook {
                hook_type: "command".to_owned(),
                command: command.command.clone(),
                command_windows: command.command_windows.clone(),
                asynchronous: None,
                timeout: Some(CAPTURE_HOOK_TIMEOUT_SECONDS),
                managed: Some(true),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct CodexHook {
    #[serde(rename = "type")]
    hook_type: String,
    command: String,
    #[serde(
        default,
        rename = "commandWindows",
        skip_serializing_if = "Option::is_none"
    )]
    command_windows: Option<String>,
    #[serde(default, rename = "async", skip_serializing_if = "Option::is_none")]
    asynchronous: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout: Option<u64>,
    #[serde(
        default,
        rename = "akraHookersManaged",
        skip_serializing_if = "Option::is_none"
    )]
    managed: Option<bool>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl CodexHook {
    fn is_akra_hook(&self) -> bool {
        self.hook_type == "command"
            && (self.managed == Some(true) || is_legacy_managed_command(&self.command))
    }
}

fn is_legacy_managed_command(command: &str) -> bool {
    let command = command.trim();
    if matches!(
        command.to_ascii_lowercase().as_str(),
        "akra-hookers capture" | "akra-hookers.exe capture"
    ) {
        return true;
    }
    let Some((executable, data_dir)) = command.split_once(" capture --data-dir ") else {
        return false;
    };
    let Some(executable) = legacy_path_argument(executable) else {
        return false;
    };
    if legacy_path_argument(data_dir).is_none() {
        return false;
    }
    std::path::Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("akra-hookers")
                || name.eq_ignore_ascii_case("akra-hookers.exe")
        })
}

fn legacy_path_argument(argument: &str) -> Option<&str> {
    let argument = argument.trim();
    let first = argument.chars().next()?;
    if matches!(first, '\'' | '"') {
        let inner = argument.strip_prefix(first)?.strip_suffix(first)?;
        return (!inner.is_empty() && !inner.contains(first)).then_some(inner);
    }
    (!argument.is_empty()
        && !argument.chars().any(|character| {
            character.is_whitespace()
                || matches!(
                    character,
                    '\'' | '"' | '&' | '|' | '<' | '>' | '(' | ')' | '^' | ';'
                )
        }))
    .then_some(argument)
}

#[derive(Debug, Error)]
pub enum CodexLifecycleError {
    #[error("Codex hook manifest has no parent directory")]
    MissingManifestParent,
    #[error("Codex lifecycle filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Codex lifecycle serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Codex hook manifest must be a regular non-link file: {0}")]
    UnsafeManifest(PathBuf),
    #[error("Codex hook manifest is {0} bytes, exceeding the {MAX_MANIFEST_BYTES}-byte limit")]
    OversizedManifest(u64),
    #[error("Codex config must be a regular non-link file: {0}")]
    UnsafeConfig(PathBuf),
    #[error("Codex config is {0} bytes, exceeding the {MAX_CONFIG_BYTES}-byte limit")]
    OversizedConfig(u64),
    #[error("Codex config is not valid UTF-8: {path}: {source}")]
    InvalidConfigEncoding {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("Codex config is not valid TOML: {path}: {source}")]
    InvalidConfigToml {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },
    #[error("Codex config has a non-table hooks/state value: {0}")]
    InvalidConfigShape(PathBuf),
    #[error("Codex hook manifest changed during lifecycle update: {0}")]
    ConcurrentManifestChange(PathBuf),
    #[error("Codex config changed during lifecycle update: {0}")]
    ConcurrentConfigChange(PathBuf),
    #[error("{source}; additionally failed to roll back a Codex hook manifest: {rollback}")]
    Rollback {
        #[source]
        source: Box<CodexLifecycleError>,
        rollback: Box<CodexLifecycleError>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn per_home_updates_apply_distinct_commands_atomically() {
        let homes = TempDir::new().expect("homes");
        let first = homes.path().join("first");
        let second = homes.path().join("second");

        apply_codex_hook_updates([
            (
                first.clone(),
                Some(CodexHookCommand::same("windows capture")),
            ),
            (second.clone(), Some(CodexHookCommand::posix("wsl capture"))),
        ])
        .expect("per-home update");

        let first_manifest = fs::read_to_string(first.join("hooks.json")).expect("first manifest");
        let second_manifest =
            fs::read_to_string(second.join("hooks.json")).expect("second manifest");
        assert!(first_manifest.contains("windows capture"));
        assert!(!first_manifest.contains("wsl capture"));
        assert!(second_manifest.contains("wsl capture"));
        assert!(!second_manifest.contains("windows capture"));
        assert!(!second_manifest.contains("commandWindows"));
    }

    #[test]
    fn shared_home_serializes_distinct_posix_and_windows_commands() {
        let home = TempDir::new().expect("home");

        apply_codex_hook_updates([(
            home.path().to_path_buf(),
            Some(CodexHookCommand::with_windows(
                "wsl capture --wsl-distro $WSL_DISTRO_NAME",
                "powershell.exe windows capture",
            )),
        )])
        .expect("shared update");

        let manifest = fs::read_to_string(home.path().join("hooks.json")).expect("manifest");
        let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("valid manifest");
        let hook = &manifest["hooks"]["UserPromptSubmit"][0]["hooks"][0];
        assert_eq!(hook["command"], "wsl capture --wsl-distro $WSL_DISTRO_NAME");
        assert_eq!(hook["commandWindows"], "powershell.exe windows capture");
    }

    #[test]
    fn lexical_aliases_create_one_manifest_lifecycle_and_lock() {
        let directory = TempDir::new().expect("home");
        let home = directory.path().join(".codex");
        fs::create_dir_all(&home).expect("Codex home");
        let alias = home.join("missing").join("..");

        let lifecycle = CodexHookLifecycleSet::from_codex_homes([home.clone(), alias]);

        assert_eq!(lifecycle.lifecycles.len(), 1);
        lifecycle
            .enable("akra-hookers capture")
            .expect("single locked lifecycle");
        assert!(lifecycle.is_enabled().expect("status"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_create_one_manifest_lifecycle() {
        let directory = TempDir::new().expect("home");
        let home = directory.path().join("codex");
        let alias = directory.path().join("alias");
        fs::create_dir_all(&home).expect("Codex home");
        std::os::unix::fs::symlink(&home, &alias).expect("alias");

        let lifecycle = CodexHookLifecycleSet::from_codex_homes([home, alias]);

        assert_eq!(lifecycle.lifecycles.len(), 1);
    }
}
