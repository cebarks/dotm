use crate::deployer::{self, DeployResult};
use crate::loader::ConfigLoader;
use crate::resolver;
use crate::scanner;
use crate::state::DeployState;
use crate::template;
use crate::vars;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use toml::map::Map;
use toml::Value;

pub struct Orchestrator {
    loader: ConfigLoader,
    target_dir: PathBuf,
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct DeployReport {
    pub created: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
    pub unchanged: Vec<PathBuf>,
    pub conflicts: Vec<(PathBuf, String)>,
    pub dry_run_actions: Vec<PathBuf>,
}

impl Orchestrator {
    pub fn new(dotfiles_dir: &Path, target_dir: &Path) -> Result<Self> {
        let loader = ConfigLoader::new(dotfiles_dir)?;
        Ok(Self {
            loader,
            target_dir: target_dir.to_path_buf(),
            state_dir: None,
        })
    }

    pub fn with_state_dir(mut self, state_dir: &Path) -> Self {
        self.state_dir = Some(state_dir.to_path_buf());
        self
    }

    pub fn deploy(&mut self, hostname: &str, dry_run: bool, force: bool) -> Result<DeployReport> {
        let mut report = DeployReport::default();
        let mut state = self
            .state_dir
            .as_ref()
            .map(|d| DeployState::new(d))
            .unwrap_or_default();

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

        // 5. Scan and deploy each package
        let packages_dir = self.loader.packages_dir();

        for pkg_name in &resolved {
            let pkg_dir = packages_dir.join(pkg_name);
            if !pkg_dir.is_dir() {
                eprintln!("warning: package directory not found: {}", pkg_dir.display());
                continue;
            }

            let actions = scanner::scan_package(&pkg_dir, hostname, &role_names)?;

            // Determine target dir for this package
            let pkg_target = if let Some(pkg_config) = self.loader.root().packages.get(pkg_name) {
                if let Some(ref target) = pkg_config.target {
                    PathBuf::from(shellexpand_tilde(target))
                } else {
                    self.target_dir.clone()
                }
            } else {
                self.target_dir.clone()
            };

            for action in &actions {
                let rendered = if action.is_template {
                    let tmpl_content = std::fs::read_to_string(&action.source)
                        .with_context(|| format!("failed to read template: {}", action.source.display()))?;
                    Some(template::render_template(&tmpl_content, &merged_vars)?)
                } else {
                    None
                };

                let result = deployer::deploy_file(action, &pkg_target, dry_run, force, rendered.as_deref())?;

                let target_path = pkg_target.join(&action.target_rel_path);
                match result {
                    DeployResult::Created | DeployResult::Updated => {
                        if action.is_copy || action.is_template {
                            state.record_copy(target_path.clone());
                        } else {
                            let abs_source = std::fs::canonicalize(&action.source)
                                .unwrap_or_else(|_| action.source.clone());
                            state.record_symlink(target_path.clone(), abs_source);
                        }
                        if matches!(result, DeployResult::Created) {
                            report.created.push(target_path);
                        } else {
                            report.updated.push(target_path);
                        }
                    }
                    DeployResult::Unchanged => report.unchanged.push(target_path),
                    DeployResult::Conflict(msg) => report.conflicts.push((target_path, msg)),
                    DeployResult::DryRun => report.dry_run_actions.push(target_path),
                }
            }
        }

        if !dry_run && self.state_dir.is_some() {
            state.save()?;
        }

        Ok(report)
    }
}

fn shellexpand_tilde(path: &str) -> String {
    if (path.starts_with("~/") || path == "~")
        && let Ok(home) = std::env::var("HOME")
    {
        return path.replacen('~', &home, 1);
    }
    path.to_string()
}
