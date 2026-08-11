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

fn codex_lifecycle(
    home: Option<std::path::PathBuf>,
) -> akra_adapters::codex::CodexHookLifecycleSet {
    match home {
        Some(home) => {
            akra_adapters::codex::CodexHookLifecycleSet::from_codex_homes([home.join(".codex")])
        }
        None => akra_adapters::codex::CodexHookLifecycleSet::from_codex_homes([
            akra_app::paths::user_home().join(".codex"),
            akra_app::paths::codex_home(),
        ]),
    }
}

fn hook_command(executable: &std::path::Path, data_dir: &std::path::Path) -> String {
    match akra_app::paths::hook_command(executable, data_dir) {
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
            let lifecycle = codex_lifecycle(home);
            let executable = std::env::current_exe().expect("current executable");
            let command = hook_command(&executable, &data_dir);
            let capture_gate = akra_app::capture_gate::CaptureGate::new(&data_dir);
            if !data_dir.join("capture-enabled").exists()
                && let Err(error) = capture_gate.set_enabled(false)
            {
                eprintln!("unable to initialize Codex capture gate: {error}");
                std::process::exit(2);
            }
            if let Err(error) =
                akra_app::capture_gate::enable_codex_capture(&capture_gate, &lifecycle, &command)
            {
                eprintln!("unable to enable Codex capture: {error}");
                std::process::exit(2);
            }
            println!("codex=enabled");
        }
        Command::Status { home } => {
            let lifecycle = codex_lifecycle(home);
            let status = if lifecycle.is_enabled().expect("Codex hook status") {
                "enabled"
            } else {
                "disabled"
            };
            println!("codex={status}");
        }
        Command::Disable { home, data_dir } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let lifecycle = codex_lifecycle(home);
            if let Err(error) = akra_app::capture_gate::disable_codex_capture(
                &akra_app::capture_gate::CaptureGate::new(&data_dir),
                &lifecycle,
            ) {
                eprintln!("unable to disable Codex capture: {error}");
                std::process::exit(2);
            }
            println!("codex=disabled");
        }
        Command::Capture { data_dir } => {
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
            let event =
                akra_adapters::codex::CodexAdapter::normalize(input).unwrap_or_else(|error| {
                    eprintln!("invalid Codex UserPromptSubmit payload: {error}");
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
            let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(
                std::path::Path::new(event.cwd()),
            )
            .unwrap_or_else(|error| {
                eprintln!("unable to capture project origin: {error}");
                std::process::exit(2);
            })
            .origin;
            let payload = serde_json::from_str(input).unwrap_or_else(|error| {
                eprintln!("invalid Codex payload JSON: {error}");
                std::process::exit(2);
            });
            let envelope = akra_app::spool::CaptureEnvelope::new(
                event.provider().as_str(),
                captured_at_us,
                origin,
                payload,
            )
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
            let lifecycle = std::sync::Arc::new(codex_lifecycle(home));
            let capture_gate = akra_app::capture_gate::CaptureGate::new(&data_dir);
            if data_dir.join("capture-enabled").exists() {
                let executable = std::env::current_exe().expect("current executable");
                let command = hook_command(&executable, &data_dir);
                if let Err(error) = akra_app::capture_gate::reconcile_codex_capture(
                    &capture_gate,
                    &lifecycle,
                    &command,
                ) {
                    eprintln!("unable to reconcile Codex capture lifecycle: {error}");
                    std::process::exit(2);
                }
            } else {
                capture_gate
                    .set_enabled(lifecycle.is_enabled().expect("Codex hook status"))
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
            let executable = std::env::current_exe().expect("current executable");
            println!("ready url=http://{address} token={token}");
            axum::serve(
                listener,
                akra_app::http::app_with_codex_lifecycle(
                    token,
                    store,
                    lifecycle,
                    hook_command(&executable, &data_dir),
                    capture_gate,
                ),
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
