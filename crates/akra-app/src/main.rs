#![forbid(unsafe_code)]

use std::io::Read;

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
        Command::Setup { home, data_dir } => {
            let home = home.unwrap_or_else(akra_app::paths::user_home);
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let lifecycle = akra_adapters::codex::CodexHookLifecycle::new(&home);
            let executable = std::env::current_exe().expect("current executable");
            lifecycle
                .enable(&akra_app::paths::hook_command(&executable, &data_dir))
                .expect("Codex hook enable");
            println!("codex=enabled");
        }
        Command::Status { home } => {
            let home = home.unwrap_or_else(akra_app::paths::user_home);
            let lifecycle = akra_adapters::codex::CodexHookLifecycle::new(&home);
            let status = if lifecycle.is_enabled().expect("Codex hook status") {
                "enabled"
            } else {
                "disabled"
            };
            println!("codex={status}");
        }
        Command::Disable { home } => {
            let home = home.unwrap_or_else(akra_app::paths::user_home);
            let lifecycle = akra_adapters::codex::CodexHookLifecycle::new(&home);
            lifecycle.disable().expect("Codex hook disable");
            println!("codex=disabled");
        }
        Command::Capture { data_dir } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).expect("stdin");
            if let Err(error) = akra_adapters::codex::CodexAdapter::normalize(&input) {
                eprintln!("invalid Codex UserPromptSubmit payload: {error}");
                std::process::exit(2);
            }
            akra_app::spool::Spool::open(&data_dir.join("spool"))
                .expect("spool")
                .enqueue(input.as_bytes())
                .expect("spool enqueue");
        }
        Command::Serve {
            port,
            data_dir,
            home,
        } => {
            let data_dir = data_dir.unwrap_or_else(akra_app::paths::default_data_dir);
            let home = home.unwrap_or_else(akra_app::paths::user_home);
            std::fs::create_dir_all(&data_dir).expect("data directory");
            let store = std::sync::Arc::new(
                akra_store::ActivityStore::open(&data_dir.join("akra-hookers.sqlite"))
                    .await
                    .expect("store"),
            );
            store.migrate().await.expect("migration");
            let spool = akra_app::spool::Spool::open(&data_dir.join("spool")).expect("spool");
            for item in spool.pending().expect("spool pending") {
                let input = match std::str::from_utf8(item.payload()) {
                    Ok(input) => input,
                    Err(error) => {
                        eprintln!("retaining invalid spool payload: {error}");
                        continue;
                    }
                };
                let event = match akra_adapters::codex::CodexAdapter::normalize(input) {
                    Ok(event) => event,
                    Err(error) => {
                        eprintln!("retaining invalid spool payload: {error}");
                        continue;
                    }
                };
                if !store
                    .provider_enabled(event.provider().as_str())
                    .await
                    .expect("provider state")
                {
                    spool
                        .acknowledge(item)
                        .expect("disabled spool acknowledgement");
                    continue;
                }
                match store
                    .record(
                        event.provider().as_str(),
                        event.session_id(),
                        event.turn_id(),
                        event.cwd(),
                        event.prompt(),
                    )
                    .await
                {
                    Ok(_) => spool.acknowledge(item).expect("spool acknowledgement"),
                    Err(error) => eprintln!("retaining spool payload after store error: {error}"),
                }
            }
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                .await
                .expect("listener");
            let address = listener.local_addr().expect("address");
            let token = Box::leak(format!("akra-{}", Uuid::new_v4()).into_boxed_str());
            let executable = std::env::current_exe().expect("current executable");
            let lifecycle =
                std::sync::Arc::new(akra_adapters::codex::CodexHookLifecycle::new(&home));
            println!("ready url=http://{address} token={token}");
            axum::serve(
                listener,
                akra_app::http::app_with_codex_lifecycle(
                    token,
                    store,
                    lifecycle,
                    akra_app::paths::hook_command(&executable, &data_dir),
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
