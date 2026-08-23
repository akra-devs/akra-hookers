#![forbid(unsafe_code)]

use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "akra-hookers", about = "Local coding-agent activity map")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Configure all detected provider integrations.
    Setup {
        #[arg(long)]
        home: Option<std::path::PathBuf>,
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
    },
    /// Run the dashboard and collector runtime (loopback by default).
    Serve {
        #[arg(
            long,
            default_value_t = akra_app::collector::DEFAULT_LOCAL_COLLECTOR_PORT
        )]
        port: u16,
        /// Interface for the dashboard and collector ingress listener.
        #[arg(long, default_value = "127.0.0.1")]
        bind: std::net::IpAddr,
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
        #[arg(long)]
        home: Option<std::path::PathBuf>,
    },
    /// Capture a provider prompt from standard input.
    Capture {
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
        /// Detected Codex installation that invoked this managed hook.
        #[arg(long)]
        capture_target: Option<String>,
        /// WSL distribution that produced the hook payload.
        #[arg(long)]
        wsl_distro: Option<String>,
        /// Exact Codex home that owns the hook and transcript.
        #[arg(long)]
        codex_home: Option<String>,
    },
    /// Report local runtime and provider status.
    Status {
        #[arg(long)]
        home: Option<std::path::PathBuf>,
    },
    /// Disable akra-managed Codex prompt capture.
    Disable {
        #[arg(long)]
        home: Option<std::path::PathBuf>,
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
    },
    /// Print this runtime's stable remote-collector access token.
    ///
    /// Use only on the collector host and paste it into a source machine's
    /// Collection destination setting. It is never included in hook commands
    /// or the normal `serve` readiness output.
    CollectorToken {
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
    },
    /// Inspect normalized local project identity.
    Debug {
        #[command(subcommand)]
        command: DebugCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DebugCommand {
    /// Print the canonical project identity for a working directory.
    ProjectId {
        #[arg(long)]
        cwd: std::path::PathBuf,
    },
}

fn codex_targets(
    home: Option<std::path::PathBuf>,
    executable: &std::path::Path,
    data_dir: &std::path::Path,
) -> akra_app::codex_targets::CodexTargetRegistry {
    match home {
        Some(home) => {
            let codex_home = home.join(".codex");
            akra_app::codex_targets::CodexTargetRegistry::explicit(
                codex_home.clone(),
                hook_command(executable, data_dir, "explicit", &codex_home),
            )
        }
        None => akra_app::codex_targets::CodexTargetRegistry::detect(executable, data_dir),
    }
}

fn hook_command(
    executable: &std::path::Path,
    data_dir: &std::path::Path,
    capture_target: &str,
    codex_home: &std::path::Path,
) -> String {
    match akra_app::paths::hook_command_for_target_and_home(
        executable,
        data_dir,
        capture_target,
        &codex_home.to_string_lossy(),
    ) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("unable to construct Codex hook command: {error}");
            std::process::exit(2);
        }
    }
}

fn main() {
    match Cli::parse().command {
        Command::Capture {
            data_dir,
            capture_target,
            wsl_distro,
            codex_home,
        } => run_capture(data_dir, capture_target, wsl_distro, codex_home),
        command => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Tokio runtime")
            .block_on(run(command)),
    }
}

async fn run(command: Command) {
    match command {
        Command::Setup { home, data_dir } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let executable = std::env::current_exe().expect("current executable");
            let targets = codex_targets(home, &executable, &data_dir);
            let capture_gate = akra_app::capture_gate::CaptureGate::new(&data_dir);
            if !data_dir.join("capture-enabled").exists()
                && let Err(error) = capture_gate.set_enabled(false)
            {
                eprintln!("unable to initialize Codex capture gate: {error}");
                std::process::exit(2);
            }
            if let Err(error) = targets.apply_all(&capture_gate, true) {
                eprintln!("unable to enable Codex capture: {error}");
                std::process::exit(2);
            }
            println!("codex=enabled");
        }
        Command::Status { home } => {
            let executable = std::env::current_exe().expect("current executable");
            let data_dir = akra_app::paths::default_data_dir();
            let targets = codex_targets(home, &executable, &data_dir);
            let statuses = targets.statuses();
            let status = if statuses.iter().any(|target| target.enabled) {
                "enabled"
            } else {
                "disabled"
            };
            println!("codex={status}");
            for target in statuses {
                println!(
                    "target={} enabled={} available={} home={}",
                    target.id,
                    target.enabled,
                    target.available,
                    target.codex_home.as_deref().unwrap_or("unknown")
                );
            }
        }
        Command::Disable { home, data_dir } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let executable = std::env::current_exe().expect("current executable");
            let targets = codex_targets(home, &executable, &data_dir);
            if let Err(error) =
                targets.apply_all(&akra_app::capture_gate::CaptureGate::new(&data_dir), false)
            {
                eprintln!("unable to disable Codex capture: {error}");
                std::process::exit(2);
            }
            println!("codex=disabled");
        }
        Command::Capture {
            data_dir,
            capture_target,
            wsl_distro,
            codex_home,
        } => run_capture(data_dir, capture_target, wsl_distro, codex_home),
        Command::Serve {
            port,
            bind,
            data_dir,
            home,
        } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            std::fs::create_dir_all(&data_dir).expect("data directory");
            let executable = std::env::current_exe().expect("current executable");
            let targets = std::sync::Arc::new(codex_targets(home, &executable, &data_dir));
            let capture_gate = akra_app::capture_gate::CaptureGate::new(&data_dir);
            if data_dir.join("capture-enabled").exists() {
                if let Err(error) = targets.reconcile(&capture_gate) {
                    eprintln!("unable to reconcile Codex capture lifecycle: {error}");
                    std::process::exit(2);
                }
            } else {
                let any_enabled = targets.statuses().iter().any(|target| target.enabled);
                capture_gate
                    .set_enabled(any_enabled)
                    .expect("capture gate synchronization");
            }
            let store = std::sync::Arc::new(
                akra_store::ActivityStore::open(&data_dir.join("akra-hookers.sqlite"))
                    .await
                    .expect("store"),
            );
            store.migrate().await.expect("migration");
            store
                .set_provider_enabled(
                    "codex",
                    capture_gate
                        .is_enabled()
                        .expect("capture gate after reconciliation"),
                )
                .await
                .expect("provider state synchronization");
            let spool_directory = data_dir.join("spool");
            let spool = akra_app::spool::Spool::open(&spool_directory).expect("spool");
            akra_app::recovery::drain(&spool, &store).await;
            let recovery_store = std::sync::Arc::clone(&store);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(200));
                loop {
                    interval.tick().await;
                    akra_app::recovery::drain(&spool, &recovery_store).await;
                }
            });
            akra_app::summarization::spawn_worker(
                std::sync::Arc::clone(&store),
                std::sync::Arc::clone(&targets),
            );
            let collector = std::sync::Arc::new(
                akra_app::collector::CollectorManager::open(&data_dir).expect("collector"),
            );
            let relay = std::sync::Arc::clone(&collector);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    if relay.relay_once().await.is_err() {
                        eprintln!(
                            "remote collector delivery worker is unavailable; queued captures are retained"
                        );
                    }
                }
            });
            let listener = tokio::net::TcpListener::bind((bind, port))
                .await
                .expect("listener");
            let address = listener.local_addr().expect("address");
            let token = Box::leak(format!("akra-{}", Uuid::new_v4()).into_boxed_str());
            println!("ready url=http://{address} token={token}");
            axum::serve(
                listener,
                akra_app::http::app_with_codex_targets_and_collector(
                    token,
                    store,
                    targets,
                    capture_gate,
                    Some(collector),
                ),
            )
            .await
            .expect("server");
        }
        Command::CollectorToken { data_dir } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let manager =
                akra_app::collector::CollectorManager::open(&data_dir).unwrap_or_else(|error| {
                    eprintln!("unable to open collector token: {error}");
                    std::process::exit(1);
                });
            println!("{}", manager.collector_token().expose_secret());
        }
        Command::Debug {
            command: DebugCommand::ProjectId { cwd },
        } => match akra_git::ProjectIdentity::from_cwd(&cwd) {
            Ok(identity) => println!("{}", identity.key()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
    }
}

fn run_capture(
    data_dir: Option<std::path::PathBuf>,
    capture_target: Option<String>,
    wsl_distro: Option<String>,
    codex_home: Option<String>,
) {
    if std::env::var_os(akra_app::summarization::SUMMARY_CHILD_ENV).is_some() {
        println!("{{}}");
        return;
    }
    let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
    match akra_app::capture_gate::CaptureGate::new(&data_dir).is_enabled() {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            eprintln!("unable to read capture gate: {error}");
            std::process::exit(1);
        }
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take((akra_app::spool::MAX_CAPTURE_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .unwrap_or_else(|error| {
            eprintln!("unable to read Codex hook payload: {error}");
            std::process::exit(1);
        });
    if input.len() > akra_app::spool::MAX_CAPTURE_INPUT_BYTES {
        eprintln!(
            "capture input exceeds the {}-byte limit",
            akra_app::spool::MAX_CAPTURE_INPUT_BYTES
        );
        std::process::exit(1);
    }
    let input = std::str::from_utf8(&input).unwrap_or_else(|error| {
        eprintln!("invalid Codex payload UTF-8: {error}");
        std::process::exit(1);
    });
    let payload: serde_json::Value = serde_json::from_str(input).unwrap_or_else(|error| {
        eprintln!("invalid Codex payload JSON: {error}");
        std::process::exit(1);
    });
    let capture = akra_adapters::codex::CodexAdapter::normalize_capture_value(&payload)
        .unwrap_or_else(|error| {
            eprintln!("invalid Codex hook payload: {error}");
            std::process::exit(1);
        });
    let is_result = matches!(&capture, akra_adapters::codex::CodexCapture::Result(_));
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|error| {
            eprintln!("system clock is before the Unix epoch: {error}");
            std::process::exit(1);
        });
    let captured_at_us = i64::try_from(elapsed.as_micros()).unwrap_or_else(|error| {
        eprintln!("capture timestamp is out of range: {error}");
        std::process::exit(1);
    });
    let (provider, cwd) = match &capture {
        akra_adapters::codex::CodexCapture::Activity(event) => {
            (event.provider().as_str(), event.cwd())
        }
        akra_adapters::codex::CodexCapture::Result(event) => {
            (event.provider().as_str(), event.cwd())
        }
    };
    let origin = capture_origin(cwd, wsl_distro.as_deref()).unwrap_or_else(|error| {
        eprintln!("unable to capture project origin: {error}");
        std::process::exit(1);
    });
    let capture_context = match capture_target.as_ref() {
        Some(_) => akra_app::capture_source::codex_managed_capture_context(
            &payload,
            wsl_distro.as_deref(),
            codex_home.as_deref(),
        ),
        None => akra_app::capture_source::codex_capture_context(&payload, wsl_distro.as_deref()),
    };
    let result_capture_target = if is_result {
        wsl_distro.as_deref().map(|distro| format!("wsl:{distro}"))
    } else {
        None
    };
    let envelope_target = result_capture_target
        .as_deref()
        .or(capture_target.as_deref());
    let envelope = match envelope_target {
        Some(target) => akra_app::spool::CaptureEnvelope::new_with_source_and_activity(
            provider,
            captured_at_us,
            origin,
            payload,
            target,
            capture_context.client,
            capture_context.activity_kind,
            capture_context.agent_id,
            capture_context.agent_type,
        ),
        None => akra_app::spool::CaptureEnvelope::new_with_activity(
            provider,
            captured_at_us,
            origin,
            payload,
            capture_context.activity_kind,
            capture_context.agent_id,
            capture_context.agent_type,
        ),
    }
    .unwrap_or_else(|error| {
        eprintln!("unable to construct capture envelope: {error}");
        std::process::exit(1);
    });
    akra_app::collector::capture_once(&data_dir, &envelope).unwrap_or_else(|error| {
        eprintln!("unable to spool capture: {error}");
        std::process::exit(1);
    });
    if is_result {
        println!("{{}}");
    }
}

fn capture_origin(
    cwd: &str,
    wsl_distro: Option<&str>,
) -> Result<akra_git::ProjectOriginSnapshot, akra_git::IdentityError> {
    #[cfg(windows)]
    if let Some(distro) = wsl_distro {
        if is_wsl_windows_mount(cwd)
            && let Ok(windows_cwd) = akra_app::paths::wsl_cwd_to_windows(distro, cwd)
        {
            return akra_git::ProjectIdentity::capture_snapshot_from_cwd(&windows_cwd)
                .map(|snapshot| snapshot.origin);
        }
        return akra_git::ProjectIdentity::capture_snapshot_from_wsl(distro, cwd)
            .map(|snapshot| snapshot.origin);
    }
    #[cfg(not(windows))]
    let _ = wsl_distro;
    akra_git::ProjectIdentity::capture_snapshot_from_cwd(std::path::Path::new(cwd))
        .map(|snapshot| snapshot.origin)
}

#[cfg(windows)]
fn is_wsl_windows_mount(cwd: &str) -> bool {
    let Some(mounted) = cwd.strip_prefix("/mnt/") else {
        return false;
    };
    let bytes = mounted.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && (bytes.len() == 1 || bytes.get(1) == Some(&b'/'))
}
