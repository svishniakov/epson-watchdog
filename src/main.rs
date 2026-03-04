mod config;
mod cups;
mod discovery;
mod installer;
mod watchdog;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "epson-watchdog",
    version,
    about = "Monitors Epson L3150 via mDNS and auto-enables CUPS queue when printer reappears"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover printer, configure CUPS, install launchd agent
    Install {
        /// CUPS queue name to register (default: EPSON_L3150_Series)
        #[arg(long, default_value = "EPSON_L3150_Series")]
        printer_name: String,

        /// Seconds to wait for mDNS discovery
        #[arg(long, default_value_t = 20)]
        discovery_timeout: u64,

        /// Use printer already registered in CUPS (skip mDNS discovery)
        #[arg(long)]
        use_existing: bool,
    },

    /// Remove launchd agent and optionally the CUPS printer
    Uninstall {
        /// Also remove the CUPS printer queue
        #[arg(long)]
        remove_printer: bool,
    },

    /// Run as daemon (called by launchd — do not call manually)
    Run,

    /// Show current printer and queue status
    Status,
}

fn main() -> Result<()> {
    // Default log level: info (can be overridden with RUST_LOG env var)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Install {
            printer_name,
            discovery_timeout,
            use_existing,
        } => installer::install(&printer_name, discovery_timeout, use_existing),

        Commands::Uninstall { remove_printer } => installer::uninstall(remove_printer),

        Commands::Run => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(watchdog::run())
        }

        Commands::Status => cups::print_status(),
    }
}
