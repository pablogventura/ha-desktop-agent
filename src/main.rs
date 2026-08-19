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
        None => ha_desktop_agent::app::run(config, cli.config).await,
    }
}
