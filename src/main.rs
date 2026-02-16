use clap::Parser;

#[derive(Parser)]
#[command(name = "dotm", about = "Dotfile manager with composable roles")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Deploy configs for the current host
    Deploy {
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing unmanaged files
        #[arg(long)]
        force: bool,
    },
    /// Remove all managed symlinks and copies
    Undeploy,
    /// Show deployment status
    Status,
    /// Validate configuration
    Check {
        /// Warn about undeployed suggested packages
        #[arg(long)]
        warn_suggestions: bool,
    },
    /// Initialize a new package
    Init {
        /// Package name
        name: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Deploy { host, dry_run, force } => {
            eprintln!("deploy: host={:?} dry_run={} force={}", host, dry_run, force);
        }
        Commands::Undeploy => {
            eprintln!("undeploy");
        }
        Commands::Status => {
            eprintln!("status");
        }
        Commands::Check { warn_suggestions } => {
            eprintln!("check: warn_suggestions={}", warn_suggestions);
        }
        Commands::Init { name } => {
            eprintln!("init: {}", name);
        }
    }

    Ok(())
}
