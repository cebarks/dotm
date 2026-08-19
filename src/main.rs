use clap::{CommandFactory, Parser};
use dotm::orchestrator::Orchestrator;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "dotm",
    about = "Dotfile manager with composable roles",
    version
)]
struct Cli {
    /// Path to the dotfiles directory (default: current directory)
    #[arg(short, long, env = "DOTM_DIR", default_value = ".")]
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
        /// Deploy only this package (and its dependencies)
        #[arg(short, long)]
        package: Option<String>,
        /// Skip pre/post deploy hooks
        #[arg(long)]
        no_hooks: bool,
    },
    /// Remove all managed symlinks and copies
    Undeploy {
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
        /// Undeploy only this package
        #[arg(short, long)]
        package: Option<String>,
        /// Skip pre/post deploy hooks
        #[arg(long)]
        no_hooks: bool,
    },
    /// Run package setup tasks (one-time/occasional imperative initialization)
    Setup {
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Show what would be executed without running
        #[arg(long)]
        dry_run: bool,
        /// Re-run setup even if already executed successfully
        #[arg(short, long)]
        force: bool,
        /// List available setup tasks and their status
        #[arg(short, long)]
        list: bool,
        /// Run setup only for this package (and its setup_after/depends dependencies)
        #[arg(short, long)]
        package: Option<String>,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
        /// Print captured command output even on success
        #[arg(short, long)]
        verbose: bool,
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
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
    /// Adopt drifted changes from copy/override files back into dotfile sources
    Adopt {
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
        /// Filter to a specific file path
        path: Option<String>,
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
    /// Add existing files to a package
    Add {
        /// Package to add files to
        package: String,
        /// Files to add
        #[arg(required = true)]
        files: Vec<std::path::PathBuf>,
        /// Overwrite if file already exists in package
        #[arg(long)]
        force: bool,
        /// Operate on system packages
        #[arg(long)]
        system: bool,
    },
    /// List available packages, roles, or hosts
    List {
        #[command(subcommand)]
        what: ListWhat,
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
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
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
    /// Remove files that are no longer managed by any package
    Prune {
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Show what would be pruned without removing
        #[arg(long)]
        dry_run: bool,
        /// Operate on system packages
        #[arg(long)]
        system: bool,
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
        /// Skip pre/post deploy hooks
        #[arg(long)]
        no_hooks: bool,
    },
}

#[derive(clap::Subcommand)]
enum ListWhat {
    /// List packages
    Packages {
        /// Show package details
        #[arg(short, long)]
        verbose: bool,
    },
    /// List roles
    Roles {
        /// Show included packages
        #[arg(short, long)]
        verbose: bool,
    },
    /// List hosts
    Hosts {
        /// Show assigned roles
        #[arg(short, long)]
        verbose: bool,
        /// Show host → role → package tree
        #[arg(long)]
        tree: bool,
    },
}

fn resolve_target_dir(config_target: &str) -> anyhow::Result<std::path::PathBuf> {
    let expanded = dotm::orchestrator::expand_path(config_target, Some("dotm.target"))?;
    Ok(std::path::PathBuf::from(expanded))
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Deploy {
            host,
            dry_run,
            force,
            system,
            package,
            no_hooks,
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

            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;
            let target_dir = resolve_target_dir(&loader.root().dotm.target)?;

            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };

            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?
                .with_state_dir(&state_dir)
                .with_system_mode(system)
                .with_package_filter(package)
                .with_no_hooks(no_hooks);

            if system && !orch.loader().root().packages.values().any(|p| p.system) {
                println!("no system packages configured");
                return Ok(());
            }

            let report = orch.deploy(&hostname, dry_run, force)?;

            if dry_run {
                println!(
                    "Dry run — would deploy {} files:",
                    report.dry_run_actions.len()
                );
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
                if !report.orphaned.is_empty() {
                    if report.pruned.is_empty() {
                        eprintln!(
                            "Warning: {} orphaned files (no longer managed):",
                            report.orphaned.len()
                        );
                        for path in &report.orphaned {
                            eprintln!("  ? {}", path.display());
                        }
                        eprintln!(
                            "Run 'dotm prune' to clean up, or set auto_prune = true in dotm.toml."
                        );
                    } else {
                        println!("Pruned {} orphaned files.", report.pruned.len());
                    }
                }
            }

            if !report.conflicts.is_empty() {
                std::process::exit(1);
            }
        }
        Commands::Restore {
            system,
            package,
            dry_run,
        } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let mut state = dotm::state::DeployState::load_locked(&state_dir)?;

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
        Commands::Undeploy {
            system,
            package,
            no_hooks,
        } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let mut state = dotm::state::DeployState::load_locked(&state_dir)?;

            // Load config for undeploy hooks (optional — hooks skipped if config
            // unavailable, or if --no-hooks was passed)
            let packages = if no_hooks {
                None
            } else {
                dotm::loader::ConfigLoader::new(&cli.dir)
                    .ok()
                    .map(|l| l.root().packages.clone())
            };

            let removed = if let Some(ref pkg) = package {
                state.undeploy_package(pkg, packages.as_ref(), &cli.dir)?
            } else {
                state.undeploy(packages.as_ref(), &cli.dir)?
            };
            println!("Removed {removed} managed files.");
        }
        Commands::Setup {
            host,
            dry_run,
            force,
            list,
            package,
            system,
            verbose,
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

            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };

            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;
            let target_dir = resolve_target_dir(&loader.root().dotm.target)?;
            let orch = dotm::setup::SetupOrchestrator::new(loader, state_dir, system, target_dir);
            let package_filter = package.map(|p| vec![p]);

            if list {
                let entries = orch.list(&hostname)?;
                if entries.is_empty() {
                    println!("No setup tasks configured.");
                    return Ok(());
                }
                println!("Available setup tasks:\n");
                for entry in entries {
                    println!("{}", entry.package);
                    println!("  Command: {}", entry.command);
                    match entry.status {
                        dotm::setup::SetupListStatus::NotRun => {
                            println!("  Status: \u{25cb} Not run");
                        }
                        dotm::setup::SetupListStatus::Success(ts) => {
                            println!("  Status: \u{2713} Success (last run: {ts})");
                        }
                        dotm::setup::SetupListStatus::Failed(ts) => {
                            println!("  Status: \u{2717} Failed (last run: {ts})");
                            if let Some(err) = entry.error {
                                println!("  Error: {err}");
                            }
                        }
                        dotm::setup::SetupListStatus::Changed => {
                            println!("  Status: \u{26a0} Changed (script modified since last run)");
                        }
                    }
                    println!();
                }
                return Ok(());
            }

            let report = orch.run(&hostname, package_filter, dry_run, force)?;

            if dry_run {
                if report.dry_run.is_empty() && report.skipped.is_empty() {
                    println!("No setup tasks to run.");
                }
                for (pkg, cmd, reason) in &report.dry_run {
                    println!("Setup (dry run): {pkg}");
                    println!("  Would execute: {cmd}");
                    println!("  Reason: {reason}");
                    println!();
                }
                for (pkg, reason) in &report.skipped {
                    println!("Setup (dry run): {pkg}");
                    println!("  Would skip: {reason}");
                    println!();
                }
            } else {
                for pkg in &report.success {
                    println!("\u{2713} Setup succeeded: {pkg}");
                }
                for (pkg, reason) in &report.skipped {
                    println!("\u{2296} Setup skipped: {pkg} ({reason})");
                }
                for (pkg, err) in &report.failed {
                    eprintln!("\u{2717} Setup failed: {pkg}");
                    if let Some(msg) = err {
                        eprintln!("  Error: {msg}");
                    }
                }

                if verbose {
                    // Verbose output placeholder — Task 11 wires real output printing.
                }

                if !report.failed.is_empty() {
                    std::process::exit(1);
                }
            }
        }
        Commands::Status {
            verbose,
            short,
            package,
            system,
        } => {
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
                dotm::status::print_status(&groups, color, verbose || package.is_some());
                println!();
                dotm::status::print_footer(total, modified, missing, color);

                if modified > 0 {
                    println!("Run 'dotm diff' to see changes, 'dotm deploy' to re-sync.");
                }
            }

            if modified > 0 || missing > 0 {
                std::process::exit(1);
            }
        }
        Commands::Diff { path, host, system } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let state = dotm::state::DeployState::load(&state_dir)?;
            let mut found_diffs = false;

            // Try to load config for full diff support
            let config_context: Option<toml::map::Map<String, toml::Value>> = (|| {
                let loader = match dotm::loader::ConfigLoader::new(&cli.dir) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("warning: could not load config: {e}");
                        return None;
                    }
                };
                let hostname = host.clone().or_else(|| {
                    hostname::get()
                        .ok()
                        .map(|h| h.to_string_lossy().to_string())
                })?;
                dotm::vars::resolve_vars_lenient(&loader, &hostname)
            })();

            if config_context.is_none()
                && state
                    .entries()
                    .iter()
                    .any(|e| e.kind == dotm::scanner::EntryKind::Template)
            {
                eprintln!(
                    "warning: could not resolve template variables; showing drift status only for templates"
                );
            }

            for entry in state.entries() {
                if let Some(ref filter) = path {
                    if !entry.target.to_str().unwrap_or("").contains(filter) {
                        continue;
                    }
                }

                // Skip symlink entries (use git diff for those)
                if entry.target.is_symlink() {
                    continue;
                }

                let status = state.check_entry_status(entry);
                if !status.is_modified() {
                    continue;
                }

                found_diffs = true;

                if let Some(ref vars) = config_context {
                    // Full diff: re-render or read source, compare to target
                    let expected = if entry.kind == dotm::scanner::EntryKind::Template {
                        std::fs::read_to_string(&entry.source)
                            .ok()
                            .and_then(|tmpl| dotm::template::render_template(&tmpl, vars).ok())
                    } else {
                        std::fs::read_to_string(&entry.source).ok()
                    };

                    let current = std::fs::read_to_string(&entry.target).unwrap_or_default();

                    if let Some(expected) = expected {
                        let label_a = format!("expected: {}", entry.target.display());
                        let label_b = format!("current:  {}", entry.target.display());
                        print!(
                            "{}",
                            dotm::diff::format_unified_diff(
                                &expected, &current, &label_a, &label_b
                            )
                        );
                    } else {
                        println!("  M {} (source unavailable)", entry.target.display());
                    }
                } else {
                    println!("  M {}", entry.target.display());
                }
            }

            if !found_diffs {
                println!("No modified files.");
            }
        }
        Commands::Adopt { system, path } => {
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
                let (is_modified, is_symlink, is_template, source, target, _content_hash) = {
                    let entry = &state.entries()[idx];
                    let status = state.check_entry_status(entry);
                    (
                        status.is_modified(),
                        entry.target.is_symlink(),
                        entry.kind == dotm::scanner::EntryKind::Template,
                        entry.source.clone(),
                        entry.target.clone(),
                        entry.content_hash.clone(),
                    )
                };

                // Path filter
                if let Some(ref filter) = path {
                    if !target.to_str().unwrap_or("").contains(filter) {
                        continue;
                    }
                }

                // Skip symlinks — edits are already source edits
                if is_symlink {
                    continue;
                }

                if !is_modified {
                    continue;
                }

                if is_template {
                    eprintln!(
                        "Skipping {} (template — edit the .tera source directly)",
                        target.display()
                    );
                    continue;
                }

                // For copy/override entries: source = expected, target = drifted
                let expected = match std::fs::read_to_string(&source) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "Skipping {} (could not read source: {})",
                            target.display(),
                            e
                        );
                        continue;
                    }
                };
                let current = match std::fs::read_to_string(&target) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "Skipping {} (could not read target: {})",
                            target.display(),
                            e
                        );
                        continue;
                    }
                };

                let file_label = target.to_str().unwrap_or("unknown");
                match dotm::adopt::interactive_adopt(file_label, &expected, &current)? {
                    Some(patched) => {
                        // Write changes back to the source in the dotfiles repo
                        std::fs::write(&source, &patched)?;
                        // Re-copy to the target to keep it in sync
                        std::fs::write(&target, &patched)?;

                        let new_hash = dotm::hash::hash_content(patched.as_bytes());
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
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .expect("invalid host filename");
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

            // Emit deprecation warnings for strategy field
            let dep_warnings = dotm::config::deprecated_strategy_warnings(loader.root());
            for w in &dep_warnings {
                eprintln!("{w}");
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
        Commands::Add {
            package,
            files,
            force,
            system: _,
        } => {
            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;

            if !loader.root().packages.contains_key(&package) {
                eprintln!("error: unknown package '{package}'");
                std::process::exit(1);
            }

            let pkg_config = &loader.root().packages[&package];
            let global_target = resolve_target_dir(&loader.root().dotm.target)?;
            let target_dir = if let Some(ref target) = pkg_config.target {
                PathBuf::from(dotm::orchestrator::expand_path(
                    target,
                    Some(&format!("package '{package}'")),
                )?)
            } else {
                global_target
            };

            let packages_dir = loader.packages_dir();
            let pkg_dir = packages_dir.join(&package);

            let mut moved = 0;
            for file in &files {
                let abs_file = std::fs::canonicalize(file).unwrap_or_else(|_| {
                    eprintln!("error: file not found: {}", file.display());
                    std::process::exit(1);
                });

                let rel_path = abs_file.strip_prefix(&target_dir).unwrap_or_else(|_| {
                    eprintln!(
                        "error: {} is not under the package target directory ({})",
                        abs_file.display(),
                        target_dir.display()
                    );
                    std::process::exit(1);
                });

                let dest = pkg_dir.join(rel_path);

                if dest.exists() && !force {
                    eprintln!(
                        "error: {} already exists in package (use --force to overwrite)",
                        dest.display()
                    );
                    std::process::exit(1);
                }

                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                std::fs::rename(&abs_file, &dest)?;
                println!("  {} → {}", abs_file.display(), dest.display());
                moved += 1;
            }

            if moved > 0 {
                println!("Added {} file(s) to package '{package}'.", moved);
                println!("Run 'dotm deploy' to create symlinks.");
            }
        }
        Commands::List { what } => {
            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;
            match what {
                ListWhat::Packages { verbose } => {
                    print!("{}", dotm::list::render_packages(loader.root(), verbose));
                }
                ListWhat::Roles { verbose } => {
                    print!("{}", dotm::list::render_roles(&loader, verbose)?);
                }
                ListWhat::Hosts { verbose, tree } => {
                    if tree {
                        print!("{}", dotm::list::render_tree(&loader)?);
                    } else {
                        print!("{}", dotm::list::render_hosts(&loader, verbose)?);
                    }
                }
            }
        }
        Commands::Commit { message } => {
            let git_repo = dotm::git::GitRepo::open(&cli.dir)
                .ok_or_else(|| anyhow::anyhow!("dotfiles directory is not a git repository"))?;

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
            let git_repo = dotm::git::GitRepo::open(&cli.dir)
                .ok_or_else(|| anyhow::anyhow!("dotfiles directory is not a git repository"))?;

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
            let git_repo = dotm::git::GitRepo::open(&cli.dir)
                .ok_or_else(|| anyhow::anyhow!("dotfiles directory is not a git repository"))?;

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
                    eprintln!("\nResolve conflicts in the dotfiles repo, then run 'dotm deploy'.");
                    std::process::exit(1);
                }
                dotm::git::PullResult::Error(msg) => {
                    eprintln!("Pull failed:\n{msg}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "dotm", &mut std::io::stdout());
        }
        Commands::Prune {
            host,
            dry_run,
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

            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;
            let target_dir = resolve_target_dir(&loader.root().dotm.target)?;

            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };

            // Load existing state to find what's currently managed.
            // Use load() not load_locked() — the orchestrator's deploy() acquires
            // its own lock, and holding two flock fds on the same file deadlocks.
            let mut existing_state = dotm::state::DeployState::load(&state_dir)?;
            if existing_state.entries().is_empty() {
                println!("No files currently managed by dotm.");
                return Ok(());
            }

            // Run a deploy scan to determine what *would* be deployed now
            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?
                .with_state_dir(&state_dir)
                .with_system_mode(system);
            let report = orch.deploy(&hostname, true, false)?; // dry run to get the target set

            let new_targets: std::collections::HashSet<std::path::PathBuf> = report
                .dry_run_actions
                .iter()
                .cloned()
                .chain(report.conflicts.iter().map(|(path, _)| path.clone()))
                .collect();

            let mut pruned_targets = Vec::new();
            let mut orphan_count = 0;
            for entry in existing_state.entries() {
                if !new_targets.contains(&entry.target) {
                    orphan_count += 1;
                    if dry_run {
                        println!("  ? {}", entry.target.display());
                    } else {
                        if entry.target.is_symlink() || entry.target.exists() {
                            let _ = std::fs::remove_file(&entry.target);
                            dotm::state::cleanup_empty_parents(&entry.target);
                        }
                        println!("  - {}", entry.target.display());
                        pruned_targets.push(entry.target.clone());
                    }
                }
            }

            let orphans_found = orphan_count;

            if dry_run {
                if orphans_found > 0 {
                    println!("Dry run — would prune {orphans_found} orphaned files.");
                } else {
                    println!("No orphaned files to prune.");
                }
            } else if orphans_found > 0 {
                // Remove pruned entries from state before re-deploy so state
                // stays consistent even if re-deploy fails
                // No lock held here — load() was used to avoid deadlock with
                // orchestrator's deploy(). Narrow race if two dotm processes
                // prune concurrently; acceptable for a single-user CLI tool.
                existing_state.remove_targets(&pruned_targets);
                existing_state.save()?;
                drop(existing_state);

                // Re-deploy to update state without orphans
                let mut orch2 = Orchestrator::new(&cli.dir, &target_dir)?
                    .with_state_dir(&state_dir)
                    .with_system_mode(system);
                orch2.deploy(&hostname, false, true)?;
                println!("Pruned {} orphaned files.", pruned_targets.len());
            } else {
                println!("No orphaned files to prune.");
            }
        }
        Commands::Sync {
            host,
            no_push,
            force,
            system,
            no_hooks,
        } => {
            let git_repo = dotm::git::GitRepo::open(&cli.dir)
                .ok_or_else(|| anyhow::anyhow!("dotfiles directory is not a git repository"))?;

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

            let loader = dotm::loader::ConfigLoader::new(&cli.dir)?;
            let target_dir = resolve_target_dir(&loader.root().dotm.target)?;

            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };

            let mut orch = Orchestrator::new(&cli.dir, &target_dir)?
                .with_state_dir(&state_dir)
                .with_system_mode(system)
                .with_no_hooks(no_hooks);

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
    let home = dirs::home_dir().unwrap_or_else(|| {
        eprintln!("error: could not determine home directory");
        std::process::exit(1);
    });
    let dotm_dir = home.join(".dotm");

    if dotm_dir.join("dotm-state.json").exists() {
        return dotm_dir;
    }

    // Legacy fallback: check XDG_STATE_HOME
    let legacy = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        .unwrap_or_else(|| {
            eprintln!("error: could not determine state directory");
            std::process::exit(1);
        })
        .join("dotm");

    if legacy.join("dotm-state.json").exists() {
        match migrate_state_dir(&legacy, &dotm_dir) {
            Ok(()) => {
                eprintln!(
                    "note: migrated state from {} to {}",
                    legacy.display(),
                    dotm_dir.display()
                );
                return dotm_dir;
            }
            Err(e) => {
                eprintln!(
                    "warning: could not migrate state to {}: {e}",
                    dotm_dir.display()
                );
                return legacy;
            }
        }
    }

    // Default to new location
    dotm_dir
}

fn migrate_state_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if !dest.exists() {
            std::fs::rename(entry.path(), &dest)?;
        }
    }
    // Remove legacy dir if now empty
    let _ = std::fs::remove_dir(from);
    Ok(())
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
