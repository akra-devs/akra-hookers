#![forbid(unsafe_code)]

use std::io::Read;

use clap::{Parser, Subcommand};

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
        #[arg(long, default_value = ".")]
        home: std::path::PathBuf,
    },
    /// Run the localhost runtime.
    Serve {
        #[arg(long, default_value_t = 0)]
        port: u16,
        #[arg(long, default_value = ".akra-hookers")]
        data_dir: std::path::PathBuf,
    },
    /// Capture a provider prompt from standard input.
    Capture {
        #[arg(long, default_value = ".akra-hookers")]
        data_dir: std::path::PathBuf,
    },
    /// Report local runtime and provider status.
    Status {
        #[arg(long, default_value = ".")]
        home: std::path::PathBuf,
    },
    /// Disable akra-managed Codex prompt capture.
    Disable {
        #[arg(long, default_value = ".")]
        home: std::path::PathBuf,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Setup { home } => {
            let lifecycle = akra_adapters::codex::CodexHookLifecycle::new(&home);
            lifecycle
                .enable("akra-hookers capture")
                .expect("Codex hook enable");
            println!("codex=enabled");
        }
        Command::Status { home } => {
            let lifecycle = akra_adapters::codex::CodexHookLifecycle::new(&home);
            let status = if lifecycle.is_enabled().expect("Codex hook status") {
                "enabled"
            } else {
                "disabled"
            };
            println!("codex={status}");
        }
        Command::Disable { home } => {
            let lifecycle = akra_adapters::codex::CodexHookLifecycle::new(&home);
            lifecycle.disable().expect("Codex hook disable");
            println!("codex=disabled");
        }
        Command::Capture { data_dir } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).expect("stdin");
            let event = match akra_adapters::codex::CodexAdapter::normalize(&input) {
                Ok(event) => event,
                Err(error) => {
                    eprintln!("invalid Codex UserPromptSubmit payload: {error}");
                    std::process::exit(2);
                }
            };
            akra_app::spool::Spool::open(&data_dir.join("spool"))
                .expect("spool")
                .enqueue(input.as_bytes())
                .expect("spool enqueue");
            println!(
                "{}",
                serde_json::json!({
                    "provider": event.provider().as_str(),
                    "prompt": event.prompt(),
                    "status": "spooled"
                })
            );
        }
        Command::Serve { port, data_dir } => {
            std::fs::create_dir_all(&data_dir).expect("data directory");
            let store = std::sync::Arc::new(
                akra_store::ActivityStore::open(&data_dir.join("akra-hookers.sqlite"))
                    .await
                    .expect("store"),
            );
            store.migrate().await.expect("migration");
            let spool = akra_app::spool::Spool::open(&data_dir.join("spool")).expect("spool");
            for payload in spool.drain().expect("spool drain") {
                let input = String::from_utf8(payload).expect("spooled UTF-8");
                let event = akra_adapters::codex::CodexAdapter::normalize(&input)
                    .expect("valid spooled Codex payload");
                store
                    .record(
                        event.provider().as_str(),
                        event.session_id(),
                        event.turn_id(),
                        event.cwd(),
                        event.prompt(),
                    )
                    .await
                    .expect("spool record");
            }
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .expect("listener");
            let address = listener.local_addr().expect("address");
            let token = Box::leak(
                format!(
                    "akra-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("clock")
                        .as_nanos()
                )
                .into_boxed_str(),
            );
            println!("ready url=http://{address} token={token}");
            axum::serve(listener, akra_app::http::app(token, store))
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
