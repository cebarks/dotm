use crate::deployer::{self, DeployResult};
use crate::hash;
use crate::loader::ConfigLoader;
use crate::metadata;
use crate::resolver;
use crate::scanner;
use crate::state::{DeployEntry, DeployState};
use crate::template;
use crate::vars;
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Orchestrator {
    loader: ConfigLoader,
    target_dir: PathBuf,
    state_dir: Option<PathBuf>,
    system_mode: bool,
    package_filter: Option<String>,
    no_hooks: bool,
}

#[derive(Debug, Default)]
pub struct DeployReport {
    pub created: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
    pub unchanged: Vec<PathBuf>,
    pub conflicts: Vec<(PathBuf, String)>,
    pub dry_run_actions: Vec<PathBuf>,
    pub orphaned: Vec<PathBuf>,
    pub pruned: Vec<PathBuf>,
}

struct PendingAction {
    pkg_name: String,
    action: scanner::FileAction,
    pkg_target: PathBuf,
    rendered: Option<String>,
    is_system: bool,
}

impl Orchestrator {
    pub fn new(dotfiles_dir: &Path, target_dir: &Path) -> Result<Self> {
        let loader = ConfigLoader::new(dotfiles_dir)?;
        Ok(Self {
            loader,
            target_dir: target_dir.to_path_buf(),
            state_dir: None,
            system_mode: false,
            package_filter: None,
            no_hooks: false,
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

    pub fn with_package_filter(mut self, filter: Option<String>) -> Self {
        self.package_filter = filter;
        self
    }

    pub fn with_no_hooks(mut self, no_hooks: bool) -> Self {
        self.no_hooks = no_hooks;
        self
    }

    pub fn loader(&self) -> &ConfigLoader {
        &self.loader
    }

    pub fn deploy(&mut self, hostname: &str, dry_run: bool, force: bool) -> Result<DeployReport> {
        let mut report = DeployReport::default();
        let mut state = self
            .state_dir
            .as_ref()
            .map(|d| DeployState::new(d))
            .unwrap_or_default();

        if self.state_dir.is_some() {
            state.lock()?;
        }

        // 1. Load host config and merge vars
        let host = self
            .loader
            .load_host(hostname)
            .with_context(|| format!("failed to load host config for '{hostname}'"))?;

        let merged_vars = vars::resolve_vars(&self.loader, hostname)?;

        // 2. Collect packages from roles
        let mut all_requested_packages: Vec<String> = Vec::new();

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
        }

        // 3. Resolve dependencies
        let requested_refs: Vec<&str> = all_requested_packages.iter().map(|s| s.as_str()).collect();
        let mut resolved = resolver::resolve_packages(self.loader.root(), &requested_refs)?;

        // 3.5. Apply package filter if set
        if let Some(ref filter) = self.package_filter {
            let filter_refs: Vec<&str> = vec![filter.as_str()];
            let filtered = resolver::resolve_packages(self.loader.root(), &filter_refs)?;
            resolved.retain(|pkg| filtered.contains(pkg));
        }

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
                eprintln!(
                    "warning: package directory not found: {}",
                    pkg_dir.display()
                );
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

            for action in actions {
                let rendered = if action.kind == scanner::EntryKind::Template {
                    let tmpl_content =
                        std::fs::read_to_string(&action.source).with_context(|| {
                            format!("failed to read template: {}", action.source.display())
                        })?;
                    Some(template::render_template(&tmpl_content, &merged_vars)?)
                } else {
                    None
                };

                pending.push(PendingAction {
                    pkg_name: pkg_name.clone(),
                    action,
                    pkg_target: pkg_target.clone(),
                    rendered,
                    is_system,
                });
            }
        }

        // Phase 2: Target-path collision detection
        let mut target_owners: HashMap<PathBuf, String> = HashMap::new();
        for p in &pending {
            let target_path = p.pkg_target.join(&p.action.target_rel_path);
            if let Some(existing) = target_owners.get(&target_path) {
                bail!(
                    "target collision -- packages '{}' and '{}' both deploy {}",
                    existing,
                    p.pkg_name,
                    target_path.display()
                );
            }
            target_owners.insert(target_path, p.pkg_name.clone());
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
            .map(|e| (e.target.clone(), e.content_hash.as_str()))
            .collect();

        let existing_targets: HashSet<PathBuf> = existing_state
            .entries()
            .iter()
            .map(|e| e.target.clone())
            .collect();

        // Phase 4: Deploy each action (with per-package hooks)
        let mut current_pkg: Option<String> = None;
        let mut skip_pkg: Option<String> = None;
        let mut deploy_error: Option<anyhow::Error> = None;

        for p in &pending {
            let file_result: Result<()> = (|| {
                // Run pre_deploy hook when entering a new package
                if current_pkg.as_deref() != Some(&p.pkg_name) {
                    // Run post_deploy for the previous package
                    if let Some(ref prev_pkg) = current_pkg {
                        if !dry_run && !self.no_hooks {
                            if let Some(pkg_config) = self.loader.root().packages.get(prev_pkg) {
                                if let Some(ref cmd) = pkg_config.post_deploy {
                                    let pkg_target = pending
                                        .iter()
                                        .find(|pp| pp.pkg_name == *prev_pkg)
                                        .map(|pp| &pp.pkg_target)
                                        .unwrap();
                                    if let Err(e) =
                                        crate::hooks::run_hook(cmd, pkg_target, prev_pkg, "deploy")
                                    {
                                        eprintln!("warning: {e}");
                                    }
                                }
                            }
                        }
                    }

                    // Run pre_deploy for the new package
                    if !dry_run && !self.no_hooks {
                        if let Some(pkg_config) = self.loader.root().packages.get(&p.pkg_name) {
                            if let Some(ref cmd) = pkg_config.pre_deploy {
                                if let Err(e) = crate::hooks::run_hook(
                                    cmd,
                                    &p.pkg_target,
                                    &p.pkg_name,
                                    "deploy",
                                ) {
                                    eprintln!(
                                        "warning: pre_deploy hook failed, skipping package '{}': {e}",
                                        p.pkg_name
                                    );
                                    skip_pkg = Some(p.pkg_name.clone());
                                    current_pkg = Some(p.pkg_name.clone());
                                    return Ok(());
                                }
                            }
                        }
                    }
                    skip_pkg = None;
                    current_pkg = Some(p.pkg_name.clone());
                }

                // Skip all files for a package whose pre_deploy hook failed
                if skip_pkg.as_deref() == Some(&p.pkg_name) {
                    report.conflicts.push((
                        p.pkg_target.join(&p.action.target_rel_path),
                        "skipped: pre_deploy hook failed".to_string(),
                    ));
                    return Ok(());
                }

                let target_path = p.pkg_target.join(&p.action.target_rel_path);

                // Determine if this is a user-mode symlink deployment
                let use_symlink = !p.is_system
                    && (p.action.kind == scanner::EntryKind::Base
                        || p.action.kind == scanner::EntryKind::Override);

                // Drift detection: only for copies (templates + system-mode files)
                if !use_symlink && target_path.exists() {
                    if let Some(&expected_hash) = existing_hashes.get(&target_path) {
                        let current_hash = hash::hash_file(&target_path)?;
                        if current_hash != expected_hash && !force {
                            eprintln!(
                                "warning: {} has been modified since last deploy, skipping (use --force to overwrite)",
                                p.action.target_rel_path.display()
                            );
                            report
                                .conflicts
                                .push((target_path, "modified since last deploy".to_string()));
                            return Ok(());
                        }
                    }
                }

                // Determine the effective force flag based on the orchestrator decision tree
                let effective_force = if existing_targets.contains(&target_path) {
                    // Target is in existing state -- managed re-deploy: skip backup
                    true
                } else if target_path.exists() && !target_path.is_symlink() && !target_path.is_dir()
                {
                    // Unmanaged regular file -- backup to originals, pass user's force value
                    force
                } else {
                    // Symlink, directory, or nonexistent -- pass through
                    force
                };

                let is_managed = existing_targets.contains(&target_path);

                // Backup pre-existing file content and metadata before deploying
                // Skip backup for managed re-deploys (preserve the original pre-dotm content)
                let (original_hash, original_owner, original_group, original_mode) =
                    if !dry_run && !is_managed && target_path.exists() && !target_path.is_symlink()
                    {
                        let content = std::fs::read(&target_path)?;
                        let hash = hash::hash_content(&content);
                        state.store_original(&hash, &content)?;

                        let (owner, group, mode) = metadata::read_file_metadata(&target_path)?;
                        (Some(hash), Some(owner), Some(group), Some(mode))
                    } else {
                        (None, None, None, None)
                    };

                // Deploy using the appropriate method
                let result = if use_symlink {
                    deployer::deploy_symlink(&p.action, &p.pkg_target, dry_run, effective_force)?
                } else {
                    deployer::deploy_copy(
                        &p.action,
                        &p.pkg_target,
                        dry_run,
                        effective_force,
                        p.rendered.as_deref(),
                    )?
                };

                match result {
                    DeployResult::Created | DeployResult::Updated => {
                        // For content_hash: hash the source file for symlinks, hash the target file for copies
                        let content_hash = if !dry_run {
                            if use_symlink {
                                hash::hash_file(&p.action.source)?
                            } else {
                                hash::hash_file(&target_path)?
                            }
                        } else {
                            String::new()
                        };

                        // Resolve and apply metadata (only for system-mode packages)
                        let resolved = if !dry_run && p.is_system {
                            if let Some(pkg_config) = self.loader.root().packages.get(&p.pkg_name) {
                                let rel_path_str = p.action.target_rel_path.to_str().unwrap_or("");
                                let resolved = metadata::resolve_metadata(pkg_config, rel_path_str);

                                if resolved.owner.is_some() || resolved.group.is_some() {
                                    if let Err(e) = metadata::apply_ownership(
                                        &target_path,
                                        resolved.owner.as_deref(),
                                        resolved.group.as_deref(),
                                    ) {
                                        eprintln!(
                                            "warning: failed to set ownership on {}: {e}",
                                            target_path.display()
                                        );
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
                            metadata::resolve_metadata(&crate::config::PackageConfig::default(), "")
                        };

                        let abs_source = std::fs::canonicalize(&p.action.source)
                            .unwrap_or_else(|_| p.action.source.clone());

                        state.record(DeployEntry {
                            target: target_path.clone(),
                            staged: None,
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

                Ok(())
            })();

            if let Err(e) = file_result {
                deploy_error = Some(e);
                break;
            }
        }

        // Run post_deploy for the final package
        if let Some(ref last_pkg) = current_pkg {
            if !dry_run
                && !self.no_hooks
                && skip_pkg.as_deref() != Some(last_pkg)
                && deploy_error.is_none()
            {
                if let Some(pkg_config) = self.loader.root().packages.get(last_pkg) {
                    if let Some(ref cmd) = pkg_config.post_deploy {
                        let pkg_target = pending
                            .iter()
                            .find(|pp| pp.pkg_name == *last_pkg)
                            .map(|pp| &pp.pkg_target)
                            .unwrap();
                        if let Err(e) = crate::hooks::run_hook(cmd, pkg_target, last_pkg, "deploy")
                        {
                            eprintln!("warning: {e}");
                        }
                    }
                }
            }
        }

        // Phase 4.5: Detect orphaned files
        if self.state_dir.is_some() {
            let new_targets: std::collections::HashSet<PathBuf> = pending
                .iter()
                .map(|p| p.pkg_target.join(&p.action.target_rel_path))
                .collect();

            let resolved_set: HashSet<&str> = resolved.iter().map(|s| s.as_str()).collect();

            for old_entry in existing_state.entries() {
                // When a --package filter is active, packages outside the
                // filtered set were never candidates for (re)deploy this run,
                // so their old entries must not be treated as orphaned. When
                // there is no filter, a package missing from `resolved` means
                // it was actually removed from the role/host config, and its
                // old entries should still go through normal orphan detection.
                if self.package_filter.is_some()
                    && !resolved_set.contains(old_entry.package.as_str())
                {
                    continue;
                }
                if !new_targets.contains(&old_entry.target) {
                    report.orphaned.push(old_entry.target.clone());

                    if !dry_run && self.loader.root().dotm.auto_prune {
                        if old_entry.target.is_symlink() || old_entry.target.exists() {
                            let _ = std::fs::remove_file(&old_entry.target);
                            crate::state::cleanup_empty_parents(&old_entry.target);
                        }
                        report.pruned.push(old_entry.target.clone());
                    }
                }
            }
        }

        // Phase 5: Save state (including partial state on error, so deployed files are tracked)
        if !dry_run && self.state_dir.is_some() {
            let orphaned_targets: HashSet<&PathBuf> = report.orphaned.iter().collect();
            if deploy_error.is_some() {
                // Merge: keep old entries for targets we didn't deploy this run,
                // but drop ones Phase 4.5 already flagged as orphaned so pruned
                // (or prune-eligible) entries don't get resurrected.
                let deployed_targets: HashSet<PathBuf> =
                    state.entries().iter().map(|e| e.target.clone()).collect();
                for old in existing_state.entries() {
                    if !deployed_targets.contains(&old.target)
                        && !orphaned_targets.contains(&old.target)
                    {
                        state.record(old.clone());
                    }
                }
                if let Err(e) = state.save() {
                    eprintln!("warning: failed to save partial state: {e}");
                }
            } else {
                // Merge: keep old entries for packages we didn't deploy this run,
                // but drop ones Phase 4.5 already flagged as orphaned so pruned
                // (or prune-eligible) entries don't get resurrected.
                let deployed_targets: HashSet<PathBuf> =
                    state.entries().iter().map(|e| e.target.clone()).collect();
                for old in existing_state.entries() {
                    if !deployed_targets.contains(&old.target)
                        && !orphaned_targets.contains(&old.target)
                    {
                        state.record(old.clone());
                    }
                }
                state.save()?;
            }
        }

        if let Some(e) = deploy_error {
            return Err(e);
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
