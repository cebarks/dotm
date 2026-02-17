use clap::Parser;
use dotm::orchestrator::Orchestrator;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dotm", about = "Dotfile manager with composable roles", version)]
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

            let state_dir = dotm_state_dir();
            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?.with_state_dir(&state_dir);
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
            let state_dir = dotm_state_dir();
            let state = dotm::state::DeployState::load(&state_dir)?;
            let removed = state.undeploy()?;
            println!("Removed {removed} managed files.");
        }
        Commands::Status => {
            let state_dir = dotm_state_dir();
            let state = dotm::state::DeployState::load(&state_dir)?;

            let symlinks = state.symlinks();
            let copies = state.copies();

            if symlinks.is_empty() && copies.is_empty() {
                println!("No files currently managed by dotm.");
                return Ok(());
            }

            println!("Managed files:");
            for entry in symlinks {
                let status = if entry.target.is_symlink() {
                    "ok"
                } else {
                    "MISSING"
                };
                println!(
                    "  [{}] {} -> {}",
                    status,
                    entry.target.display(),
                    entry.source.display()
                );
            }
            for path in copies {
                let status = if path.exists() { "ok" } else { "MISSING" };
                println!("  [{}] {} (copy)", status, path.display());
            }
        }
        Commands::Check { warn_suggestions } => {
            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;
            let mut errors: Vec<String> = Vec::new();

            // Validate all host configs
            let hosts_dir = cli.dir.join("hosts");
            if hosts_dir.is_dir() {
                for entry in std::fs::read_dir(&hosts_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                        let stem = path.file_stem().unwrap().to_str().unwrap();
                        match loader.load_host(stem) {
                            Ok(host) => {
                                for role_name in &host.roles {
                                    if let Err(e) = loader.load_role(role_name) {
                                        errors.push(format!(
                                            "host '{}' references invalid role '{}': {}",
                                            stem, role_name, e
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                errors.push(format!("invalid host config '{}': {}", stem, e));
                            }
                        }
                    }
                }
            }

            // Validate package dependencies
            let root = loader.root();
            for (pkg_name, pkg_config) in &root.packages {
                for dep in &pkg_config.depends {
                    if !root.packages.contains_key(dep) {
                        errors.push(format!(
                            "package '{}' depends on unknown package '{}'",
                            pkg_name, dep
                        ));
                    }
                }
                if warn_suggestions {
                    for sug in &pkg_config.suggests {
                        if !root.packages.contains_key(sug) {
                            eprintln!(
                                "warning: package '{}' suggests unknown package '{}'",
                                pkg_name, sug
                            );
                        }
                    }
                }

                // Check package directory exists
                let pkg_dir = loader.packages_dir().join(pkg_name);
                if !pkg_dir.is_dir() {
                    errors.push(format!(
                        "package '{}' declared but directory not found: {}",
                        pkg_name,
                        pkg_dir.display()
                    ));
                }
            }

            // Check for circular dependencies
            let all_pkgs: Vec<&str> = root.packages.keys().map(|s| s.as_str()).collect();
            if let Err(e) = dotm::resolver::resolve_packages(root, &all_pkgs) {
                errors.push(format!("dependency resolution error: {}", e));
            }

            if errors.is_empty() {
                println!("Configuration is valid.");
            } else {
                eprintln!("Configuration errors:");
                for err in &errors {
                    eprintln!("  - {}", err);
                }
                std::process::exit(1);
            }
        }
        Commands::Init { name } => {
            let pkg_dir = cli.dir.join("packages").join(&name);
            if pkg_dir.exists() {
                eprintln!(
                    "error: package '{}' already exists at {}",
                    name,
                    pkg_dir.display()
                );
                std::process::exit(1);
            }
            std::fs::create_dir_all(&pkg_dir)?;
            println!("Created package: {}", pkg_dir.display());
            println!("Add files mirroring their home directory structure.");
        }
    }

    Ok(())
}

fn dotm_state_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/state"))
        .join("dotm")
}
