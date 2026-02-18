use crate::config::DeployStrategy;
use crate::deployer::{self, DeployResult};
use crate::hash;
use crate::loader::ConfigLoader;
use crate::metadata;
use crate::resolver;
use crate::scanner;
use crate::state::{DeployEntry, DeployState};
use crate::template;
use crate::vars;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use toml::map::Map;
use toml::Value;

pub struct Orchestrator {
    loader: ConfigLoader,
    target_dir: PathBuf,
    state_dir: Option<PathBuf>,
    staging_dir: PathBuf,
    system_mode: bool,
}

#[derive(Debug, Default)]
pub struct DeployReport {
    pub created: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
    pub unchanged: Vec<PathBuf>,
    pub conflicts: Vec<(PathBuf, String)>,
    pub dry_run_actions: Vec<PathBuf>,
}

struct PendingAction {
    pkg_name: String,
    action: scanner::FileAction,
    pkg_target: PathBuf,
    rendered: Option<String>,
    strategy: DeployStrategy,
}

impl Orchestrator {
    pub fn new(dotfiles_dir: &Path, target_dir: &Path) -> Result<Self> {
        let staging_dir = dotfiles_dir.join(".staged");
        let loader = ConfigLoader::new(dotfiles_dir)?;
        Ok(Self {
            loader,
            target_dir: target_dir.to_path_buf(),
            state_dir: None,
            staging_dir,
            system_mode: false,
        })
    }

    pub fn with_state_dir(mut self, state_dir: &Path) -> Self {
        self.state_dir = Some(state_dir.to_path_buf());
        self
    }

    pub fn with_system_mode(mut self, system: bool) -> Self {
        self.system_mode = system;
        self
    }

    pub fn loader(&self) -> &ConfigLoader {
        &self.loader
    }

    fn get_pkg_strategy(&self, pkg_name: &str) -> DeployStrategy {
        self.loader
            .root()
            .packages
            .get(pkg_name)
            .and_then(|c| c.strategy)
            .unwrap_or(DeployStrategy::Stage)
    }

    pub fn deploy(&mut self, hostname: &str, dry_run: bool, force: bool) -> Result<DeployReport> {
        let mut report = DeployReport::default();
        let mut state = self
            .state_dir
            .as_ref()
            .map(|d| DeployState::new(d))
            .unwrap_or_default();

        let effective_staging_dir = if self.system_mode {
            self.state_dir
                .as_ref()
                .map(|d| d.join(".staged"))
                .unwrap_or_else(|| self.staging_dir.clone())
        } else {
            self.staging_dir.clone()
        };

        // 1. Load host config
        let host = self
            .loader
            .load_host(hostname)
            .with_context(|| format!("failed to load host config for '{hostname}'"))?;

        // 2. Load roles and collect packages + merge vars
        let mut all_requested_packages: Vec<String> = Vec::new();
        let mut merged_vars: Map<String, Value> = Map::new();

        for role_name in &host.roles {
            let role = self
                .loader
                .load_role(role_name)
                .with_context(|| format!("failed to load role '{role_name}'"))?;

            for pkg in &role.packages {
                if !all_requested_packages.contains(pkg) {
                    all_requested_packages.push(pkg.clone());
                }
            }

            merged_vars = vars::merge_vars(&merged_vars, &role.vars);
        }

        // Host vars override role vars
        merged_vars = vars::merge_vars(&merged_vars, &host.vars);

        // 3. Resolve dependencies
        let requested_refs: Vec<&str> = all_requested_packages.iter().map(|s| s.as_str()).collect();
        let resolved = resolver::resolve_packages(self.loader.root(), &requested_refs)?;

        // 4. Collect role names for override resolution
        let role_names: Vec<&str> = host.roles.iter().map(|s| s.as_str()).collect();

        // Phase 1: Scan all packages and collect pending actions
        let packages_dir = self.loader.packages_dir();
        let mut pending: Vec<PendingAction> = Vec::new();

        for pkg_name in &resolved {
            // Filter packages based on system mode
            let is_system = self
                .loader
                .root()
                .packages
                .get(pkg_name)
                .map(|c| c.system)
                .unwrap_or(false);
            if self.system_mode != is_system {
                continue;
            }

            let pkg_dir = packages_dir.join(pkg_name);
            if !pkg_dir.is_dir() {
                eprintln!("warning: package directory not found: {}", pkg_dir.display());
                continue;
            }

            let actions = scanner::scan_package(&pkg_dir, hostname, &role_names)?;

            let pkg_target = if let Some(pkg_config) = self.loader.root().packages.get(pkg_name) {
                if let Some(ref target) = pkg_config.target {
                    PathBuf::from(expand_path(target, Some(&format!("package '{pkg_name}'")))?)
                } else {
                    self.target_dir.clone()
                }
            } else {
                self.target_dir.clone()
            };

            let strategy = self.get_pkg_strategy(pkg_name);

            for action in actions {
                let rendered = if action.kind == scanner::EntryKind::Template {
                    let tmpl_content = std::fs::read_to_string(&action.source)
                        .with_context(|| format!("failed to read template: {}", action.source.display()))?;
                    Some(template::render_template(&tmpl_content, &merged_vars)?)
                } else {
                    None
                };

                pending.push(PendingAction {
                    pkg_name: pkg_name.clone(),
                    action,
                    pkg_target: pkg_target.clone(),
                    rendered,
                    strategy,
                });
            }
        }

        // Phase 2: Collision detection for staged packages
        let mut staging_owners: HashMap<PathBuf, String> = HashMap::new();
        for p in &pending {
            if p.strategy == DeployStrategy::Stage {
                let staging_path = effective_staging_dir.join(&p.action.target_rel_path);
                if let Some(existing) = staging_owners.get(&staging_path) {
                    bail!(
                        "staging collision -- packages '{}' and '{}' both deploy {}",
                        existing,
                        p.pkg_name,
                        p.action.target_rel_path.display()
                    );
                }
                staging_owners.insert(staging_path, p.pkg_name.clone());
            }
        }

        // Phase 3: Load existing state for drift detection
        let existing_state = self
            .state_dir
            .as_ref()
            .map(|d| DeployState::load(d))
            .transpose()?
            .unwrap_or_default();

        let existing_hashes: HashMap<PathBuf, &str> = existing_state
            .entries()
            .iter()
            .map(|e| (e.staged.clone(), e.content_hash.as_str()))
            .collect();

        // Phase 4: Deploy each action
        for p in &pending {
            let target_path = p.pkg_target.join(&p.action.target_rel_path);

            match p.strategy {
                DeployStrategy::Stage => {
                    let staged_path = effective_staging_dir.join(&p.action.target_rel_path);

                    // Drift detection: if staged file exists and was modified since last deploy
                    if staged_path.exists()
                        && let Some(&expected_hash) = existing_hashes.get(&staged_path) {
                            let current_hash = hash::hash_file(&staged_path)?;
                            if current_hash != expected_hash && !force {
                                eprintln!(
                                    "warning: {} has been modified since last deploy, skipping (use --force to overwrite)",
                                    p.action.target_rel_path.display()
                                );
                                report.conflicts.push((
                                    target_path,
                                    "modified since last deploy".to_string(),
                                ));
                                continue;
                            }
                        }

                    // Backup pre-existing file content and metadata before deploying
                    let (original_hash, original_owner, original_group, original_mode) =
                        if !dry_run && target_path.exists() && !target_path.is_symlink() {
                            let content = std::fs::read(&target_path)?;
                            let hash = hash::hash_content(&content);
                            state.store_original(&hash, &content)?;

                            let (owner, group, mode) = metadata::read_file_metadata(&target_path)?;
                            (Some(hash), Some(owner), Some(group), Some(mode))
                        } else {
                            (None, None, None, None)
                        };

                    let result = deployer::deploy_staged(
                        &p.action,
                        &effective_staging_dir,
                        &p.pkg_target,
                        dry_run,
                        force,
                        p.rendered.as_deref(),
                    )?;

                    match result {
                        DeployResult::Created | DeployResult::Updated => {
                            let content_hash = if !dry_run {
                                hash::hash_file(&staged_path)?
                            } else {
                                String::new()
                            };

                            if !dry_run && self.state_dir.is_some() {
                                let content = std::fs::read(&staged_path)?;
                                state.store_deployed(&content_hash, &content)?;
                            }

                            // Resolve and apply metadata
                            let resolved = if !dry_run {
                                if let Some(pkg_config) = self.loader.root().packages.get(&p.pkg_name) {
                                    let rel_path_str = p.action.target_rel_path.to_str().unwrap_or("");
                                    let resolved = metadata::resolve_metadata(pkg_config, rel_path_str);

                                    if resolved.owner.is_some() || resolved.group.is_some() {
                                        if let Err(e) = metadata::apply_ownership(
                                            &staged_path,
                                            resolved.owner.as_deref(),
                                            resolved.group.as_deref(),
                                        ) {
                                            eprintln!("warning: failed to set ownership on {}: {e}", staged_path.display());
                                        }
                                    }

                                    if let Some(ref mode) = resolved.mode {
                                        deployer::apply_permission_override(&staged_path, mode)?;
                                    }

                                    resolved
                                } else {
                                    metadata::resolve_metadata(
                                        &crate::config::PackageConfig::default(),
                                        "",
                                    )
                                }
                            } else {
                                metadata::resolve_metadata(
                                    &crate::config::PackageConfig {
                                        description: None,
                                        depends: vec![],
                                        suggests: vec![],
                                        target: None,
                                        strategy: None,
                                        system: false,
                                        owner: None,
                                        group: None,
                                        permissions: Default::default(),
                                        ownership: Default::default(),
                                        preserve: Default::default(),
                                    },
                                    "",
                                )
                            };

                            let abs_source = std::fs::canonicalize(&p.action.source)
                                .unwrap_or_else(|_| p.action.source.clone());

                            state.record(DeployEntry {
                                target: target_path.clone(),
                                staged: staged_path.clone(),
                                source: abs_source,
                                content_hash,
                                original_hash,
                                kind: p.action.kind,
                                package: p.pkg_name.clone(),
                                owner: resolved.owner,
                                group: resolved.group,
                                mode: resolved.mode,
                                original_owner,
                                original_group,
                                original_mode,
                            });

                            if matches!(result, DeployResult::Updated) {
                                report.updated.push(target_path.clone());
                            } else {
                                report.created.push(target_path.clone());
                            }
                        }
                        DeployResult::Conflict(msg) => {
                            report.conflicts.push((target_path, msg));
                        }
                        DeployResult::DryRun => {
                            report.dry_run_actions.push(target_path);
                        }
                        _ => {}
                    }
                }
                DeployStrategy::Copy => {
                    // Drift detection: if target exists and was modified since last deploy
                    if target_path.exists() {
                        if let Some(&expected_hash) = existing_hashes.get(&target_path) {
                            let current_hash = hash::hash_file(&target_path)?;
                            if current_hash != expected_hash && !force {
                                eprintln!(
                                    "warning: {} has been modified since last deploy, skipping (use --force to overwrite)",
                                    p.action.target_rel_path.display()
                                );
                                report.conflicts.push((
                                    target_path,
                                    "modified since last deploy".to_string(),
                                ));
                                continue;
                            }
                        }
                    }

                    // Backup pre-existing file content and metadata before deploying
                    let (original_hash, original_owner, original_group, original_mode) =
                        if !dry_run && target_path.exists() && !target_path.is_symlink() {
                            let content = std::fs::read(&target_path)?;
                            let hash = hash::hash_content(&content);
                            state.store_original(&hash, &content)?;

                            let (owner, group, mode) = metadata::read_file_metadata(&target_path)?;
                            (Some(hash), Some(owner), Some(group), Some(mode))
                        } else {
                            (None, None, None, None)
                        };

                    let result = deployer::deploy_copy(
                        &p.action,
                        &p.pkg_target,
                        dry_run,
                        force,
                        p.rendered.as_deref(),
                    )?;

                    match result {
                        DeployResult::Created | DeployResult::Updated => {
                            let content_hash = if !dry_run {
                                hash::hash_file(&target_path)?
                            } else {
                                String::new()
                            };

                            if !dry_run && self.state_dir.is_some() {
                                let content = std::fs::read(&target_path)?;
                                state.store_deployed(&content_hash, &content)?;
                            }

                            // Resolve and apply metadata
                            let resolved = if !dry_run {
                                if let Some(pkg_config) = self.loader.root().packages.get(&p.pkg_name) {
                                    let rel_path_str = p.action.target_rel_path.to_str().unwrap_or("");
                                    let resolved = metadata::resolve_metadata(pkg_config, rel_path_str);

                                    if resolved.owner.is_some() || resolved.group.is_some() {
                                        if let Err(e) = metadata::apply_ownership(
                                            &target_path,
                                            resolved.owner.as_deref(),
                                            resolved.group.as_deref(),
                                        ) {
                                            eprintln!("warning: failed to set ownership on {}: {e}", target_path.display());
                                        }
                                    }

                                    if let Some(ref mode) = resolved.mode {
                                        deployer::apply_permission_override(&target_path, mode)?;
                                    }

                                    resolved
                                } else {
                                    metadata::resolve_metadata(
                                        &crate::config::PackageConfig::default(),
                                        "",
                                    )
                                }
                            } else {
                                metadata::resolve_metadata(
                                    &crate::config::PackageConfig {
                                        description: None,
                                        depends: vec![],
                                        suggests: vec![],
                                        target: None,
                                        strategy: None,
                                        system: false,
                                        owner: None,
                                        group: None,
                                        permissions: Default::default(),
                                        ownership: Default::default(),
                                        preserve: Default::default(),
                                    },
                                    "",
                                )
                            };

                            let abs_source = std::fs::canonicalize(&p.action.source)
                                .unwrap_or_else(|_| p.action.source.clone());

                            state.record(DeployEntry {
                                target: target_path.clone(),
                                staged: target_path.clone(), // for copy strategy, staged = target
                                source: abs_source,
                                content_hash,
                                original_hash,
                                kind: p.action.kind,
                                package: p.pkg_name.clone(),
                                owner: resolved.owner,
                                group: resolved.group,
                                mode: resolved.mode,
                                original_owner,
                                original_group,
                                original_mode,
                            });

                            if matches!(result, DeployResult::Updated) {
                                report.updated.push(target_path);
                            } else {
                                report.created.push(target_path);
                            }
                        }
                        DeployResult::Conflict(msg) => {
                            report.conflicts.push((target_path, msg));
                        }
                        DeployResult::DryRun => {
                            report.dry_run_actions.push(target_path);
                        }
                        _ => {}
                    }
                }
            }
        }

        // Phase 5: Save state
        if !dry_run && self.state_dir.is_some() {
            state.save()?;
        }

        // Warn if .staged/ is not in .gitignore (only relevant for user-mode)
        if !dry_run && !self.system_mode {
            let gitignore_path = self.loader.base_dir().join(".gitignore");
            let staged_ignored = if gitignore_path.exists() {
                std::fs::read_to_string(&gitignore_path)
                    .map(|c| c.lines().any(|l| l.trim() == ".staged" || l.trim() == ".staged/"))
                    .unwrap_or(false)
            } else {
                false
            };
            if !staged_ignored {
                eprintln!("warning: '.staged/' is not in your .gitignore — add it to avoid committing staged files");
            }
        }

        Ok(report)
    }
}

/// Expand shell variables and tilde in a path string.
/// Errors if a referenced environment variable is not defined.
pub fn expand_path(path: &str, context: Option<&str>) -> Result<String> {
    shellexpand::full(path)
        .map(|s| s.into_owned())
        .map_err(|e| {
            if let Some(ctx) = context {
                anyhow::anyhow!("{ctx}: {e}")
            } else {
                anyhow::anyhow!("path expansion failed: {e}")
            }
        })
}
