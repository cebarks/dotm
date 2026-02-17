use clap::Parser;
use dotm::orchestrator::Orchestrator;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dotm", about = "Dotfile manager with composable roles")]
struct Cli {
    /// Path to the dotfiles directory (default: current directory)
    #[arg(short, long, default_value = ".")]
    dir: PathBuf,

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
        Commands::Deploy {
            host,
            dry_run,
            force,
        } => {
            let hostname = match host {
                Some(h) => h,
                None => hostname::get()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|_| {
                        eprintln!("error: could not detect hostname, use --host to specify");
                        std::process::exit(1);
                    }),
            };

            let target_dir = dirs::home_dir().unwrap_or_else(|| {
                eprintln!("error: could not determine home directory");
                std::process::exit(1);
            });

            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?;
            let report = orch.deploy(&hostname, dry_run, force)?;

            if dry_run {
                println!("Dry run — would deploy {} files:", report.dry_run_actions.len());
                for path in &report.dry_run_actions {
                    println!("  {}", path.display());
                }
            } else {
                if !report.created.is_empty() {
                    println!("Created {} files:", report.created.len());
                    for path in &report.created {
                        println!("  + {}", path.display());
                    }
                }
                if !report.updated.is_empty() {
                    println!("Updated {} files:", report.updated.len());
                    for path in &report.updated {
                        println!("  ~ {}", path.display());
                    }
                }
                if !report.conflicts.is_empty() {
                    eprintln!("Conflicts ({}):", report.conflicts.len());
                    for (path, msg) in &report.conflicts {
                        eprintln!("  ! {} — {}", path.display(), msg);
                    }
                }
            }

            if !report.conflicts.is_empty() {
                std::process::exit(1);
            }
        }
        Commands::Undeploy => {
            eprintln!("undeploy: not yet implemented");
        }
        Commands::Status => {
            eprintln!("status: not yet implemented");
        }
        Commands::Check { warn_suggestions } => {
            eprintln!(
                "check: not yet implemented (warn_suggestions={})",
                warn_suggestions
            );
        }
        Commands::Init { name } => {
            eprintln!("init: not yet implemented (name={})", name);
        }
    }

    Ok(())
}
