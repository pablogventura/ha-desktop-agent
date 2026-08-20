use clap::{Parser, Subcommand};
use ha_desktop_agent::config::Config;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "ha-desktop-agent",
    version,
    about = "Desktop telemetry and control for Home Assistant"
)]
struct Cli {
    /// Path to YAML config file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Parse and validate the configuration file
    Validate,
    /// Windows service (MQTT + machine collectors). Use --console to skip SCM.
    #[cfg(target_os = "windows")]
    Service {
        #[arg(long)]
        console: bool,
    },
    /// Windows session helper (named pipe client)
    #[cfg(target_os = "windows")]
    Session,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;
    match cli.command {
        Some(Commands::Validate) => {
            println!("config ok (device: {})", config.device.name);
            Ok(())
        }
        #[cfg(target_os = "windows")]
        Some(Commands::Service { console }) => {
            if console {
                ha_desktop_agent::app::run_windows_service(config, cli.config, None).await
            } else {
                ha_desktop_agent::collect::windows::dispatch_windows_service()
                    .map_err(|err| anyhow::anyhow!("{err}"))
            }
        }
        #[cfg(target_os = "windows")]
        Some(Commands::Session) => {
            tokio::task::spawn_blocking(move || {
                ha_desktop_agent::collect::windows::run_session_loop(config)
            })
            .await?
        }
        None => ha_desktop_agent::app::run(config, cli.config).await,
    }
}
