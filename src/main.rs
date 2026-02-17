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
    /// Show diffs for files modified since last deploy
    Diff {
        /// Only show diff for a specific file path
        path: Option<String>,
    },
    /// Interactively adopt changes made to deployed files back into source
    Adopt,
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
            let entries = state.entries();

            if entries.is_empty() {
                println!("No files currently managed by dotm.");
                return Ok(());
            }

            let mut _ok_count = 0usize;
            let mut modified_count = 0usize;
            let mut missing_count = 0usize;

            println!("Managed files:");
            for entry in entries {
                let status = state.check_entry_status(entry);
                let label = match status {
                    dotm::state::FileStatus::Ok => {
                        _ok_count += 1;
                        "ok"
                    }
                    dotm::state::FileStatus::Modified => {
                        modified_count += 1;
                        "MODIFIED"
                    }
                    dotm::state::FileStatus::Missing => {
                        missing_count += 1;
                        "MISSING"
                    }
                };
                println!("  [{:8}] {}", label, entry.target.display());
            }

            println!();
            println!(
                "{} managed, {} modified, {} missing.",
                entries.len(),
                modified_count,
                missing_count
            );

            if modified_count > 0 || missing_count > 0 {
                if modified_count > 0 {
                    println!(
                        "Run 'dotm diff' to see changes, 'dotm adopt' to review and accept."
                    );
                }
                std::process::exit(1);
            }
        }
        Commands::Diff { path } => {
            let state_dir = dotm_state_dir();
            let state = dotm::state::DeployState::load(&state_dir)?;
            let mut found_diffs = false;

            for entry in state.entries() {
                if let Some(ref filter) = path
                    && !entry.target.to_str().unwrap_or("").contains(filter)
                {
                    continue;
                }

                let status = state.check_entry_status(entry);
                if status != dotm::state::FileStatus::Modified {
                    continue;
                }

                found_diffs = true;

                let current = std::fs::read_to_string(&entry.staged).unwrap_or_default();
                let original = state
                    .load_original(&entry.content_hash)
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_else(|_| "(original not available)".to_string());

                let label_a = format!("deployed: {}", entry.target.display());
                let label_b = format!("current:  {}", entry.target.display());
                print!(
                    "{}",
                    dotm::diff::format_unified_diff(&original, &current, &label_a, &label_b)
                );
            }

            if !found_diffs {
                println!("No modified files.");
            }
        }
        Commands::Adopt => {
            let state_dir = dotm_state_dir();
            let state = dotm::state::DeployState::load(&state_dir)?;
            let mut adopted_count = 0;

            for entry in state.entries() {
                let status = state.check_entry_status(entry);
                if status != dotm::state::FileStatus::Modified {
                    continue;
                }

                if entry.kind == dotm::scanner::EntryKind::Template {
                    eprintln!(
                        "Skipping {} (template — changes must be manually applied to the .tera source)",
                        entry.target.display()
                    );
                    continue;
                }

                let current = std::fs::read_to_string(&entry.staged)?;
                let original = state
                    .load_original(&entry.content_hash)
                    .map(|b| String::from_utf8_lossy(&b).to_string())?;

                let file_label = entry.target.to_str().unwrap_or("unknown");
                match dotm::adopt::interactive_adopt(file_label, &original, &current)? {
                    Some(patched) => {
                        std::fs::write(&entry.source, &patched)?;
                        std::fs::write(&entry.staged, &patched)?;
                        adopted_count += 1;
                        println!("Adopted changes to {}", entry.source.display());
                    }
                    None => {
                        println!("Skipped {}", entry.target.display());
                    }
                }
            }

            if adopted_count > 0 {
                println!(
                    "\nAdopted changes to {} file(s). Run 'dotm deploy' to re-sync state.",
                    adopted_count
                );
            } else {
                println!("No changes adopted.");
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
