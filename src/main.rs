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
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
    /// Remove all managed symlinks and copies
    Undeploy {
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
    /// Show deployment status
    Status {
        /// Show all files, not just problems
        #[arg(short, long)]
        verbose: bool,
        /// One-line summary for shell integration (no output when clean)
        #[arg(short, long)]
        short: bool,
        /// Filter to a specific package
        #[arg(short, long)]
        package: Option<String>,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
    /// Show diffs for files modified since last deploy
    Diff {
        /// Only show diff for a specific file path
        path: Option<String>,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
    /// Interactively adopt changes made to deployed files back into source
    Adopt {
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
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
    /// Commit all changes in the dotfiles repository
    Commit {
        /// Commit message (auto-generated if not provided)
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Push dotfiles repository to remote
    Push,
    /// Pull dotfiles repository from remote
    Pull,
    /// Restore files to their pre-dotm state
    Restore {
        /// Restore only system packages
        #[arg(long)]
        system: bool,
        /// Filter to a specific package
        #[arg(short, long)]
        package: Option<String>,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
    /// Pull, deploy, and optionally push in one step
    Sync {
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Skip pushing after deploy
        #[arg(long)]
        no_push: bool,
        /// Overwrite existing unmanaged files
        #[arg(long)]
        force: bool,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Deploy {
            host,
            dry_run,
            force,
            system,
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

            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };

            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?
                .with_state_dir(&state_dir)
                .with_system_mode(system);

            if system && !orch.loader().root().packages.values().any(|p| p.system) {
                println!("no system packages configured");
                return Ok(());
            }

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
        Commands::Restore { system, package, dry_run } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let state = dotm::state::DeployState::load_locked(&state_dir)?;

            if state.entries().is_empty() {
                println!("No files currently managed by dotm.");
                return Ok(());
            }

            if dry_run {
                let mut count = 0;
                for entry in state.entries() {
                    if let Some(ref filter) = package {
                        if entry.package != *filter {
                            continue;
                        }
                    }
                    if entry.original_hash.is_some() {
                        println!("  restore {}", entry.target.display());
                    } else {
                        println!("  remove  {}", entry.target.display());
                    }
                    count += 1;
                }
                println!("Dry run — would restore {} files.", count);
            } else {
                let restored = state.restore(package.as_deref())?;
                println!("Restored {} files.", restored);
            }
        }
        Commands::Undeploy { system } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let state = dotm::state::DeployState::load_locked(&state_dir)?;
            let removed = state.undeploy()?;
            println!("Removed {removed} managed files.");
        }
        Commands::Status { verbose, short, package, system } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let state = dotm::state::DeployState::load(&state_dir)?;
            let entries = state.entries();

            if entries.is_empty() {
                if !short {
                    println!("No files currently managed by dotm.");
                }
                return Ok(());
            }

            let statuses: Vec<dotm::state::FileStatus> = entries
                .iter()
                .map(|e| state.check_entry_status(e))
                .collect();

            let mut groups = dotm::status::group_by_package(entries, &statuses);

            if let Some(ref pkg_name) = package {
                groups.retain(|g| g.name == *pkg_name);
                if groups.is_empty() {
                    eprintln!("error: no deployed package named '{pkg_name}'");
                    std::process::exit(1);
                }
            }

            let total: usize = groups.iter().map(|g| g.total).sum();
            let modified: usize = groups.iter().map(|g| g.modified).sum();
            let missing: usize = groups.iter().map(|g| g.missing).sum();

            let color = dotm::status::use_color();

            // Git summary (optional — only when in a git repo)
            if let Some(git_repo) = dotm::git::GitRepo::open(&cli.dir) {
                match git_repo.summary() {
                    Ok(summary) => {
                        if !short {
                            dotm::status::print_git_summary(&summary, color);
                        }
                    }
                    Err(e) => {
                        if !short {
                            eprintln!("warning: failed to read git status: {e}");
                        }
                    }
                }
            }

            if short {
                dotm::status::print_short(total, modified, missing, color);
            } else {
                if verbose || package.is_some() {
                    dotm::status::print_status_verbose(&groups, color);
                } else {
                    dotm::status::print_status_default(&groups, color);
                }
                println!();
                dotm::status::print_footer(total, modified, missing, color);

                if modified > 0 {
                    println!("Run 'dotm diff' to see changes, 'dotm adopt' to review and accept.");
                }
            }

            if modified > 0 || missing > 0 {
                std::process::exit(1);
            }
        }
        Commands::Diff { path, system } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let state = dotm::state::DeployState::load(&state_dir)?;
            let mut found_diffs = false;

            for entry in state.entries() {
                if let Some(ref filter) = path
                    && !entry.target.to_str().unwrap_or("").contains(filter)
                {
                    continue;
                }

                let status = state.check_entry_status(entry);
                if !status.is_modified() {
                    continue;
                }

                found_diffs = true;

                let current = std::fs::read_to_string(&entry.staged).unwrap_or_default();
                let original = state
                    .load_deployed(&entry.content_hash)
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
        Commands::Adopt { system } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let mut state = dotm::state::DeployState::load_locked(&state_dir)?;
            let mut adopted_count = 0;
            let num_entries = state.entries().len();

            for idx in 0..num_entries {
                let (is_modified, is_template, staged, source, target, content_hash) = {
                    let entry = &state.entries()[idx];
                    let status = state.check_entry_status(entry);
                    (
                        status.is_modified(),
                        entry.kind == dotm::scanner::EntryKind::Template,
                        entry.staged.clone(),
                        entry.source.clone(),
                        entry.target.clone(),
                        entry.content_hash.clone(),
                    )
                };

                if !is_modified {
                    continue;
                }

                if is_template {
                    eprintln!(
                        "Skipping {} (template — changes must be manually applied to the .tera source)",
                        target.display()
                    );
                    continue;
                }

                let current = std::fs::read_to_string(&staged)?;
                let original = state
                    .load_deployed(&content_hash)
                    .map(|b| String::from_utf8_lossy(&b).to_string())?;

                let file_label = target.to_str().unwrap_or("unknown");
                match dotm::adopt::interactive_adopt(file_label, &original, &current)? {
                    Some(patched) => {
                        std::fs::write(&source, &patched)?;
                        std::fs::write(&staged, &patched)?;

                        let new_hash = dotm::hash::hash_content(patched.as_bytes());
                        state.store_deployed(&new_hash, patched.as_bytes())?;
                        state.update_entry_hash(idx, new_hash);

                        adopted_count += 1;
                        println!("Adopted changes to {}", source.display());
                    }
                    None => {
                        println!("Skipped {}", target.display());
                    }
                }
            }

            if adopted_count > 0 {
                state.save()?;
                println!("\nAdopted changes to {} file(s).", adopted_count);
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

            // Validate system package configuration
            errors.extend(dotm::config::validate_system_packages(root));

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
        Commands::Commit { message } => {
            let git_repo = dotm::git::GitRepo::open(&cli.dir).ok_or_else(|| {
                anyhow::anyhow!("dotfiles directory is not a git repository")
            })?;

            let msg = match message {
                Some(m) => m,
                None => {
                    let dirty = git_repo.dirty_files()?;
                    if dirty.is_empty() {
                        anyhow::bail!("nothing to commit — working tree is clean");
                    }
                    let mut body = format!("dotm: update {} files\n\n", dirty.len());
                    for f in &dirty {
                        body.push_str(&format!("  {}\n", f.path));
                    }
                    body
                }
            };

            git_repo.commit_all(&msg)?;
            println!("Committed changes.");
        }
        Commands::Push => {
            let git_repo = dotm::git::GitRepo::open(&cli.dir).ok_or_else(|| {
                anyhow::anyhow!("dotfiles directory is not a git repository")
            })?;

            match git_repo.push()? {
                dotm::git::PushResult::Success => println!("Pushed successfully."),
                dotm::git::PushResult::NoRemote => {
                    eprintln!("error: no remote configured");
                    std::process::exit(1);
                }
                dotm::git::PushResult::Rejected(msg) => {
                    eprintln!("Push rejected:\n{msg}");
                    std::process::exit(1);
                }
                dotm::git::PushResult::Error(msg) => {
                    eprintln!("Push failed:\n{msg}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Pull => {
            let git_repo = dotm::git::GitRepo::open(&cli.dir).ok_or_else(|| {
                anyhow::anyhow!("dotfiles directory is not a git repository")
            })?;

            match git_repo.pull()? {
                dotm::git::PullResult::Success => println!("Pulled successfully."),
                dotm::git::PullResult::AlreadyUpToDate => println!("Already up to date."),
                dotm::git::PullResult::NoRemote => {
                    eprintln!("error: no remote configured");
                    std::process::exit(1);
                }
                dotm::git::PullResult::Conflicts(files) => {
                    eprintln!("Pull resulted in conflicts:");
                    for f in &files {
                        eprintln!("  ! {f}");
                    }
                    eprintln!(
                        "\nResolve conflicts in the dotfiles repo, then run 'dotm deploy'."
                    );
                    std::process::exit(1);
                }
                dotm::git::PullResult::Error(msg) => {
                    eprintln!("Pull failed:\n{msg}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Sync {
            host,
            no_push,
            force,
            system,
        } => {
            let git_repo = dotm::git::GitRepo::open(&cli.dir).ok_or_else(|| {
                anyhow::anyhow!("dotfiles directory is not a git repository")
            })?;

            // Step 1: Pull
            println!("Pulling from remote...");
            match git_repo.pull()? {
                dotm::git::PullResult::Success => println!("Pulled successfully."),
                dotm::git::PullResult::AlreadyUpToDate => println!("Already up to date."),
                dotm::git::PullResult::NoRemote => {
                    eprintln!("warning: no remote configured, skipping pull");
                }
                dotm::git::PullResult::Conflicts(files) => {
                    eprintln!("Pull resulted in merge conflicts:");
                    for f in &files {
                        eprintln!("  ! {f}");
                    }
                    eprintln!(
                        "\nSync aborted. Resolve conflicts in the dotfiles repo, then retry."
                    );
                    std::process::exit(1);
                }
                dotm::git::PullResult::Error(msg) => {
                    eprintln!("Pull failed:\n{msg}");
                    eprintln!("Sync aborted.");
                    std::process::exit(1);
                }
            }

            // Step 2: Deploy
            println!("Deploying...");
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

            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };

            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?
                .with_state_dir(&state_dir)
                .with_system_mode(system);

            if system && !orch.loader().root().packages.values().any(|p| p.system) {
                println!("no system packages configured");
                return Ok(());
            }

            let report = orch.deploy(&hostname, false, force)?;

            if !report.created.is_empty() {
                println!("Created {} files.", report.created.len());
            }
            if !report.updated.is_empty() {
                println!("Updated {} files.", report.updated.len());
            }
            if !report.conflicts.is_empty() {
                eprintln!("Deploy conflicts ({}):", report.conflicts.len());
                for (path, msg) in &report.conflicts {
                    eprintln!("  ! {} — {}", path.display(), msg);
                }
            }

            // Step 3: Push (unless --no-push)
            if !no_push {
                println!("Pushing to remote...");
                match git_repo.push()? {
                    dotm::git::PushResult::Success => println!("Pushed successfully."),
                    dotm::git::PushResult::NoRemote => {
                        eprintln!("warning: no remote configured, skipping push");
                    }
                    dotm::git::PushResult::Rejected(msg) => {
                        eprintln!("Push rejected:\n{msg}");
                        std::process::exit(1);
                    }
                    dotm::git::PushResult::Error(msg) => {
                        eprintln!("Push failed:\n{msg}");
                        std::process::exit(1);
                    }
                }
            }

            println!("Sync complete.");
        }
    }

    Ok(())
}

fn dotm_state_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/state"))
        .join("dotm")
}

fn system_state_dir() -> PathBuf {
    PathBuf::from("/var/lib/dotm")
}

fn check_system_privileges() {
    if nix::unistd::geteuid().as_raw() != 0 {
        eprintln!("error: system packages require root privileges — run with sudo");
        std::process::exit(1);
    }
}
