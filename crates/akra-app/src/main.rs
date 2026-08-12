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
    /// Run the localhost runtime.
    Serve {
        #[arg(long, default_value_t = 0)]
        port: u16,
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
        Some(home) => akra_app::codex_targets::CodexTargetRegistry::explicit(
            home.join(".codex"),
            hook_command(executable, data_dir, "explicit"),
        ),
        None => akra_app::codex_targets::CodexTargetRegistry::detect(executable, data_dir),
    }
}

fn hook_command(
    executable: &std::path::Path,
    data_dir: &std::path::Path,
    capture_target: &str,
) -> String {
    match akra_app::paths::hook_command_for_target(executable, data_dir, capture_target) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("unable to construct Codex hook command: {error}");
            std::process::exit(2);
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
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
        } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            match akra_app::capture_gate::CaptureGate::new(&data_dir).is_enabled() {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    eprintln!("unable to read capture gate: {error}");
                    std::process::exit(2);
                }
            }
            let mut input = Vec::new();
            std::io::stdin()
                .take((akra_app::spool::MAX_CAPTURE_INPUT_BYTES + 1) as u64)
                .read_to_end(&mut input)
                .expect("stdin");
            if input.len() > akra_app::spool::MAX_CAPTURE_INPUT_BYTES {
                eprintln!(
                    "capture input exceeds the {}-byte limit",
                    akra_app::spool::MAX_CAPTURE_INPUT_BYTES
                );
                std::process::exit(2);
            }
            let input = std::str::from_utf8(&input).unwrap_or_else(|error| {
                eprintln!("invalid Codex payload UTF-8: {error}");
                std::process::exit(2);
            });
            let payload: serde_json::Value = serde_json::from_str(input).unwrap_or_else(|error| {
                eprintln!("invalid Codex payload JSON: {error}");
                std::process::exit(2);
            });
            let event =
                akra_adapters::codex::CodexAdapter::normalize(input).unwrap_or_else(|error| {
                    eprintln!("invalid Codex hook payload: {error}");
                    std::process::exit(2);
                });
            let elapsed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|error| {
                    eprintln!("system clock is before the Unix epoch: {error}");
                    std::process::exit(2);
                });
            let captured_at_us = i64::try_from(elapsed.as_micros()).unwrap_or_else(|error| {
                eprintln!("capture timestamp is out of range: {error}");
                std::process::exit(2);
            });
            let origin =
                capture_origin(event.cwd(), wsl_distro.as_deref()).unwrap_or_else(|error| {
                    eprintln!("unable to capture project origin: {error}");
                    std::process::exit(2);
                });
            let capture_context = match capture_target.as_ref() {
                Some(_) => akra_app::capture_source::codex_managed_capture_context(
                    &payload,
                    wsl_distro.as_deref(),
                ),
                None => {
                    akra_app::capture_source::codex_capture_context(&payload, wsl_distro.as_deref())
                }
            };
            let envelope = match capture_target.as_deref() {
                Some(target) => akra_app::spool::CaptureEnvelope::new_with_source_and_activity(
                    event.provider().as_str(),
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
                    event.provider().as_str(),
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
                std::process::exit(2);
            });
            if let Err(error) = akra_app::spool::Spool::open(&data_dir.join("spool"))
                .and_then(|spool| spool.enqueue_envelope(&envelope))
            {
                eprintln!("unable to spool capture: {error}");
                std::process::exit(2);
            }
        }
        Command::Serve {
            port,
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
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .expect("listener");
            let address = listener.local_addr().expect("address");
            let token = Box::leak(format!("akra-{}", Uuid::new_v4()).into_boxed_str());
            println!("ready url=http://{address} token={token}");
            axum::serve(
                listener,
                akra_app::http::app_with_codex_targets(token, store, targets, capture_gate),
            )
            .await
            .expect("server");
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
