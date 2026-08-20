use crate::config::{PackageConfig, RootConfig};
use crate::hash;
use crate::loader::ConfigLoader;
use crate::orchestrator::expand_path;
use crate::resolver;
use crate::setup_state::{SetupEntry, SetupState, SetupStatus};
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub struct SetupOrchestrator {
    loader: ConfigLoader,
    state_dir: PathBuf,
    system_mode: bool,
    target_dir: PathBuf,
}

#[derive(Debug)]
pub struct SetupResultEntry {
    pub package: String,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct SetupReport {
    pub succeeded: Vec<SetupResultEntry>,
    pub failed: Vec<SetupResultEntry>,
    pub skipped: Vec<(String, &'static str)>,
    pub dry_run: Vec<(String, String, &'static str)>,
}

impl SetupOrchestrator {
    pub fn new(loader: ConfigLoader, state_dir: PathBuf, system_mode: bool, target_dir: PathBuf) -> Self {
        Self {
            loader,
            state_dir,
            system_mode,
            target_dir,
        }
    }

    pub fn loader(&self) -> &ConfigLoader {
        &self.loader
    }

    /// Resolve `hostname`'s roles into a fully `depends`-expanded package list.
    fn resolve_host_packages(&self, hostname: &str) -> Result<Vec<String>> {
        let host = self.loader.load_host(hostname)?;
        let mut role_packages: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for role_name in &host.roles {
            let role = self.loader.load_role(role_name)?;
            for pkg in role.packages {
                if seen.insert(pkg.clone()) {
                    role_packages.push(pkg);
                }
            }
        }
        let refs: Vec<&str> = role_packages.iter().map(|s| s.as_str()).collect();
        resolver::resolve_packages(self.loader.root(), &refs)
    }

    pub fn run(
        &self,
        hostname: &str,
        package_filter: Option<Vec<String>>,
        dry_run: bool,
        force: bool,
    ) -> Result<SetupReport> {
        let mut state = SetupState::load(&self.state_dir)?;
        let mut report = SetupReport::default();

        let mut resolved = self.resolve_host_packages(hostname)?;

        if let Some(filter) = &package_filter {
            let filter_refs: Vec<&str> = filter.iter().map(|s| s.as_str()).collect();
            let mut filtered = resolver::resolve_packages(self.loader.root(), &filter_refs)?;
            expand_setup_after_closure(self.loader.root(), &mut filtered)?;
            resolved.retain(|pkg| filtered.contains(pkg));
        }

        let setup_packages: Vec<String> = resolved
            .into_iter()
            .filter(|pkg| {
                self.loader
                    .root()
                    .packages
                    .get(pkg)
                    .is_some_and(|c| c.setup.is_some() && c.system == self.system_mode)
            })
            .collect();

        let order = resolve_setup_order(self.loader.root(), &setup_packages)?;
        let packages_dir = self.loader.packages_dir();

        for pkg_name in order {
            let pkg_config = self
                .loader
                .root()
                .packages
                .get(&pkg_name)
                .expect("package resolved from root config must exist in root config");
            let setup_cmd = pkg_config
                .setup
                .as_ref()
                .expect("filtered to only packages with a setup field");
            let pkg_dir = packages_dir.join(&pkg_name);

            let current_hash = compute_setup_hash(&pkg_dir, setup_cmd)?;
            let (should_run, reason) = should_run_setup(&pkg_name, &current_hash, &state, force);

            if !should_run {
                report.skipped.push((pkg_name, reason));
                continue;
            }

            if dry_run {
                report.dry_run.push((pkg_name, setup_cmd.clone(), reason));
                continue;
            }

            let entry = match execute_setup(&pkg_name, &pkg_dir, pkg_config, &self.target_dir) {
                Ok(entry) => entry,
                Err(e) => {
                    state.save()?;
                    return Err(e);
                }
            };
            let did_succeed = entry.status == SetupStatus::Success;
            let result_entry = SetupResultEntry {
                package: pkg_name.clone(),
                output: entry.output.clone(),
                error: entry.error.clone(),
            };
            state.update(pkg_name, entry);

            if did_succeed {
                report.succeeded.push(result_entry);
            } else {
                report.failed.push(result_entry);
                break;
            }
        }

        if !dry_run {
            state.save()?;
        }

        Ok(report)
    }
}

#[derive(Debug)]
pub struct SetupListEntry {
    pub package: String,
    pub command: String,
    pub status: SetupListStatus,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum SetupListStatus {
    NotRun,
    Success(String),
    Failed(String),
    Changed,
}

impl SetupOrchestrator {
    pub fn list(
        &self,
        hostname: &str,
        package_filter: Option<&[String]>,
    ) -> Result<Vec<SetupListEntry>> {
        let state = SetupState::load(&self.state_dir)?;
        let resolved = self.resolve_host_packages(hostname)?;
        let packages_dir = self.loader.packages_dir();

        let mut entries = Vec::new();
        for pkg_name in resolved {
            if let Some(filter) = package_filter {
                if !filter.contains(&pkg_name) {
                    continue;
                }
            }
            let Some(pkg_config) = self.loader.root().packages.get(&pkg_name) else {
                continue;
            };
            if pkg_config.system != self.system_mode {
                continue;
            }
            let Some(setup_cmd) = &pkg_config.setup else {
                continue;
            };

            let pkg_dir = packages_dir.join(&pkg_name);
            let current_hash = compute_setup_hash(&pkg_dir, setup_cmd)?;
            let state_entry = state.get(&pkg_name);

            let status = match state_entry {
                None => SetupListStatus::NotRun,
                Some(e) if e.script_hash != current_hash => SetupListStatus::Changed,
                Some(e) if e.status == SetupStatus::Failed => {
                    SetupListStatus::Failed(e.last_run.clone())
                }
                Some(e) => SetupListStatus::Success(e.last_run.clone()),
            };

            entries.push(SetupListEntry {
                package: pkg_name,
                command: setup_cmd.clone(),
                status,
                error: state_entry.and_then(|e| e.error.clone()),
            });
        }

        Ok(entries)
    }
}

/// Extend `packages` in place with the transitive closure of `setup_after`
/// references reachable from the packages already in the list.
fn expand_setup_after_closure(root: &RootConfig, packages: &mut Vec<String>) -> Result<()> {
    let mut seen: HashSet<String> = packages.iter().cloned().collect();
    let mut stack: Vec<String> = packages.clone();

    while let Some(pkg_name) = stack.pop() {
        let Some(pkg_config) = root.packages.get(&pkg_name) else {
            continue;
        };
        for after in &pkg_config.setup_after {
            if !root.packages.contains_key(after) {
                bail!("package '{pkg_name}' setup_after unknown package '{after}'");
            }
            if seen.insert(after.clone()) {
                packages.push(after.clone());
                stack.push(after.clone());
            }
        }
    }

    Ok(())
}

/// Compute the content hash for a package's setup command, used for
/// change detection. If `setup` looks like a script path (no whitespace,
/// and resolves to an existing file under the package directory), hash
/// the file's bytes. Otherwise hash the command string itself.
pub fn compute_setup_hash(pkg_dir: &Path, setup_cmd: &str) -> Result<String> {
    if !setup_cmd.contains(char::is_whitespace) {
        let script_path = pkg_dir.join(setup_cmd);
        if script_path.is_file() {
            return hash::hash_file(&script_path);
        }
    }
    Ok(hash::hash_content(setup_cmd.as_bytes()))
}

/// Combine package `depends` and `setup_after` into a single ordering
/// constraint graph over `packages` (the set of packages that have a
/// `setup` field), then topologically sort.
///
/// `setup_after` entries are validated against `root.packages` (the
/// package must exist at all); a reference to a package that exists but
/// isn't in `packages` (i.e. has no `setup` field) is a no-op constraint.
pub fn resolve_setup_order(root: &RootConfig, packages: &[String]) -> Result<Vec<String>> {
    let pkg_set: HashSet<&str> = packages.iter().map(|s| s.as_str()).collect();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for pkg_name in packages {
        let mut deps: Vec<String> = Vec::new();
        if let Some(pkg_config) = root.packages.get(pkg_name) {
            for dep in &pkg_config.depends {
                if pkg_set.contains(dep.as_str()) && !deps.contains(dep) {
                    deps.push(dep.clone());
                }
            }
            for after in &pkg_config.setup_after {
                if !root.packages.contains_key(after) {
                    bail!("package '{pkg_name}' setup_after unknown package '{after}'");
                }
                if pkg_set.contains(after.as_str()) && !deps.contains(after) {
                    deps.push(after.clone());
                }
            }
        }
        graph.insert(pkg_name.clone(), deps);
    }

    topological_sort(&graph, packages)
}

fn topological_sort(
    graph: &HashMap<String, Vec<String>>,
    packages: &[String],
) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();

    for pkg in packages {
        if !visited.contains(pkg) {
            topo_visit(pkg, graph, &mut visited, &mut Vec::new(), &mut result)?;
        }
    }

    Ok(result)
}

fn topo_visit(
    pkg: &str,
    graph: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
    result: &mut Vec<String>,
) -> Result<()> {
    if stack.contains(&pkg.to_string()) {
        stack.push(pkg.to_string());
        bail!("circular setup dependency detected: {}", stack.join(" -> "));
    }
    if visited.contains(pkg) {
        return Ok(());
    }

    stack.push(pkg.to_string());
    if let Some(deps) = graph.get(pkg) {
        for dep in deps {
            topo_visit(dep, graph, visited, stack, result)?;
        }
    }
    stack.pop();

    visited.insert(pkg.to_string());
    result.push(pkg.to_string());
    Ok(())
}

/// Execute a package's setup command, capturing combined stdout+stderr.
/// `pkg_dir` is both the working directory for the command and the base
/// for resolving script-path setup values. `default_target` is used for
/// `DOTM_SETUP_ROOT` when the package has no explicit `target` override.
pub fn execute_setup(
    package: &str,
    pkg_dir: &Path,
    config: &PackageConfig,
    default_target: &Path,
) -> Result<SetupEntry> {
    let setup_cmd = config
        .setup
        .as_deref()
        .expect("execute_setup called on package without a setup field");
    let shell_str = config.setup_shell.as_deref().unwrap_or("sh");
    let hash = compute_setup_hash(pkg_dir, setup_cmd)?;

    let setup_root: PathBuf = match &config.target {
        Some(t) => PathBuf::from(expand_path(t, Some(&format!("package '{package}'")))?),
        None => default_target.to_path_buf(),
    };

    let mut shell_parts = shell_str.split_whitespace();
    let shell_bin = shell_parts
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("sh");

    let mut cmd = Command::new(shell_bin);
    for flag in shell_parts {
        cmd.arg(flag);
    }
    cmd.arg("-c")
        .arg(setup_cmd)
        .current_dir(pkg_dir)
        .env("DOTM_PACKAGE", package)
        .env("DOTM_SETUP_ROOT", setup_root.to_string_lossy().as_ref())
        .env("DOTM_PACKAGES_DIR", pkg_dir.parent().unwrap_or(pkg_dir));

    let start = Instant::now();
    let output = cmd.output().with_context(|| {
        format!("failed to execute setup for package '{package}' with shell '{shell_str}'")
    })?;
    let duration_ms = start.elapsed().as_millis() as u64;

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    let captured_output = if combined.is_empty() {
        None
    } else {
        Some(combined)
    };

    let last_run = chrono::Utc::now().to_rfc3339();

    if output.status.success() {
        Ok(SetupEntry {
            last_run,
            script_hash: hash,
            status: SetupStatus::Success,
            exit_code: output.status.code().unwrap_or(0),
            duration_ms,
            error: None,
            output: captured_output,
        })
    } else {
        let exit_code = output.status.code().unwrap_or(1);
        Ok(SetupEntry {
            last_run,
            script_hash: hash,
            status: SetupStatus::Failed,
            exit_code,
            duration_ms,
            error: Some(format!(
                "command '{setup_cmd}' exited with code {exit_code}"
            )),
            output: captured_output,
        })
    }
}

/// Decide whether a package's setup should run, given its current script
/// hash and prior state. Returns (should_run, human-readable reason).
pub fn should_run_setup(
    package: &str,
    current_hash: &str,
    state: &SetupState,
    force: bool,
) -> (bool, &'static str) {
    if force {
        return (true, "forced re-run");
    }

    match state.get(package) {
        None => (true, "never run"),
        Some(entry) => {
            if entry.script_hash != current_hash {
                (true, "script changed")
            } else if entry.status == SetupStatus::Failed {
                (true, "previous run failed")
            } else {
                (false, "already run successfully")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DotmSettings, RootConfig};
    use crate::loader::ConfigLoader;
    use crate::setup_state::{SetupState, SetupStatus};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_root(packages: Vec<(&str, Vec<&str>, Vec<&str>, Option<&str>)>) -> RootConfig {
        let mut pkg_map = HashMap::new();
        for (name, depends, setup_after, setup) in packages {
            pkg_map.insert(
                name.to_string(),
                PackageConfig {
                    depends: depends.into_iter().map(String::from).collect(),
                    setup_after: setup_after.into_iter().map(String::from).collect(),
                    setup: setup.map(String::from),
                    ..Default::default()
                },
            );
        }
        RootConfig {
            dotm: DotmSettings {
                target: "~".to_string(),
                packages_dir: "packages".to_string(),
                auto_prune: false,
            },
            packages: pkg_map,
        }
    }

    fn make_success_entry(hash: &str) -> SetupEntry {
        SetupEntry {
            last_run: "2026-03-31T12:00:00+00:00".to_string(),
            script_hash: hash.to_string(),
            status: SetupStatus::Success,
            exit_code: 0,
            duration_ms: 10,
            error: None,
            output: None,
        }
    }

    #[test]
    fn inline_command_hashes_the_string() {
        let dir = TempDir::new().unwrap();
        let h1 = compute_setup_hash(dir.path(), "brew bundle --file=~/.Brewfile").unwrap();
        let h2 = compute_setup_hash(dir.path(), "brew bundle --file=~/.Brewfile").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1, hash::hash_content(b"brew bundle --file=~/.Brewfile"));
    }

    #[test]
    fn existing_script_path_hashes_file_contents() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        let script = dir.path().join("scripts/apply.sh");
        std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();

        let h = compute_setup_hash(dir.path(), "scripts/apply.sh").unwrap();
        assert_eq!(h, hash::hash_file(&script).unwrap());
    }

    #[test]
    fn missing_script_path_falls_back_to_string_hash() {
        let dir = TempDir::new().unwrap();
        let h = compute_setup_hash(dir.path(), "scripts/missing.sh").unwrap();
        assert_eq!(h, hash::hash_content(b"scripts/missing.sh"));
    }

    #[test]
    fn script_hash_changes_when_file_content_changes() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("setup.sh");
        std::fs::write(&script, "echo one").unwrap();
        let h1 = compute_setup_hash(dir.path(), "setup.sh").unwrap();

        std::fs::write(&script, "echo two").unwrap();
        let h2 = compute_setup_hash(dir.path(), "setup.sh").unwrap();

        assert_ne!(h1, h2);
    }

    #[test]
    fn multiword_command_never_treated_as_script_even_if_first_word_is_a_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("setup.sh"), "echo hi").unwrap();
        let h = compute_setup_hash(dir.path(), "setup.sh --flag").unwrap();
        assert_eq!(h, hash::hash_content(b"setup.sh --flag"));
    }

    #[test]
    fn should_run_when_never_run() {
        let dir = TempDir::new().unwrap();
        let state = SetupState::new(dir.path());
        let hash = compute_setup_hash(dir.path(), "echo hi").unwrap();
        let (run, reason) = should_run_setup("pkg", &hash, &state, false);
        assert!(run);
        assert_eq!(reason, "never run");
    }

    #[test]
    fn should_skip_when_already_success_and_hash_matches() {
        let dir = TempDir::new().unwrap();
        let hash = compute_setup_hash(dir.path(), "echo hi").unwrap();
        let mut state = SetupState::new(dir.path());
        state.update("pkg".to_string(), make_success_entry(&hash));

        let (run, reason) = should_run_setup("pkg", &hash, &state, false);
        assert!(!run);
        assert_eq!(reason, "already run successfully");
    }

    #[test]
    fn should_run_when_script_changed() {
        let dir = TempDir::new().unwrap();
        let mut state = SetupState::new(dir.path());
        state.update("pkg".to_string(), make_success_entry("old-hash"));

        let new_hash = compute_setup_hash(dir.path(), "echo hi").unwrap();
        let (run, reason) = should_run_setup("pkg", &new_hash, &state, false);
        assert!(run);
        assert_eq!(reason, "script changed");
    }

    #[test]
    fn should_run_when_previous_failed() {
        let dir = TempDir::new().unwrap();
        let hash = compute_setup_hash(dir.path(), "echo hi").unwrap();
        let mut entry = make_success_entry(&hash);
        entry.status = SetupStatus::Failed;
        let mut state = SetupState::new(dir.path());
        state.update("pkg".to_string(), entry);

        let (run, reason) = should_run_setup("pkg", &hash, &state, false);
        assert!(run);
        assert_eq!(reason, "previous run failed");
    }

    #[test]
    fn setup_after_orders_before_dependent() {
        let root = make_root(vec![
            ("a", vec![], vec!["b"], Some("echo a")),
            ("b", vec![], vec![], Some("echo b")),
        ]);
        let order = resolve_setup_order(&root, &["a".to_string(), "b".to_string()]).unwrap();
        let a_pos = order.iter().position(|p| p == "a").unwrap();
        let b_pos = order.iter().position(|p| p == "b").unwrap();
        assert!(b_pos < a_pos);
    }

    #[test]
    fn package_depends_also_orders_setup() {
        let root = make_root(vec![
            ("a", vec!["b"], vec![], Some("echo a")),
            ("b", vec![], vec![], Some("echo b")),
        ]);
        let order = resolve_setup_order(&root, &["a".to_string(), "b".to_string()]).unwrap();
        let a_pos = order.iter().position(|p| p == "a").unwrap();
        let b_pos = order.iter().position(|p| p == "b").unwrap();
        assert!(b_pos < a_pos);
    }

    #[test]
    fn circular_setup_after_errors() {
        let root = make_root(vec![
            ("a", vec![], vec!["b"], Some("echo a")),
            ("b", vec![], vec!["a"], Some("echo b")),
        ]);
        let result = resolve_setup_order(&root, &["a".to_string(), "b".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("circular"));
    }

    #[test]
    fn unknown_setup_after_errors() {
        let root = make_root(vec![("a", vec![], vec!["missing"], Some("echo a"))]);
        let result = resolve_setup_order(&root, &["a".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown package 'missing'")
        );
    }

    #[test]
    fn setup_after_referencing_package_without_setup_field_is_a_noop() {
        let root = make_root(vec![
            ("a", vec![], vec!["b"], Some("echo a")),
            ("b", vec![], vec![], None),
        ]);
        let order = resolve_setup_order(&root, &["a".to_string()]).unwrap();
        assert_eq!(order, vec!["a".to_string()]);
    }

    #[test]
    fn execute_setup_success_captures_output_and_records_hash() {
        let pkg_dir = TempDir::new().unwrap();
        let cfg = PackageConfig {
            setup: Some("echo hello-from-setup".to_string()),
            ..Default::default()
        };
        let entry = execute_setup("mypkg", pkg_dir.path(), &cfg, pkg_dir.path()).unwrap();
        assert_eq!(entry.status, SetupStatus::Success);
        assert_eq!(entry.exit_code, 0);
        assert!(entry.error.is_none());
        assert!(entry.output.as_deref().unwrap().contains("hello-from-setup"));
        assert_eq!(
            entry.script_hash,
            compute_setup_hash(pkg_dir.path(), "echo hello-from-setup").unwrap()
        );
    }

    #[test]
    fn execute_setup_failure_records_exit_code_and_error() {
        let pkg_dir = TempDir::new().unwrap();
        let cfg = PackageConfig {
            setup: Some("exit 7".to_string()),
            ..Default::default()
        };
        let entry = execute_setup("mypkg", pkg_dir.path(), &cfg, pkg_dir.path()).unwrap();
        assert_eq!(entry.status, SetupStatus::Failed);
        assert_eq!(entry.exit_code, 7);
        assert!(entry.error.as_ref().unwrap().contains("exited with code 7"));
    }

    #[test]
    fn execute_setup_uses_default_target_when_package_has_none() {
        let pkg_dir = TempDir::new().unwrap();
        let cfg = PackageConfig {
            setup: Some(
                "echo PKG=$DOTM_PACKAGE ROOT=$DOTM_SETUP_ROOT DIR=$DOTM_PACKAGES_DIR".to_string(),
            ),
            ..Default::default()
        };
        let default_target = pkg_dir.path().join("fallback-target");
        let entry = execute_setup("mypkg", pkg_dir.path(), &cfg, &default_target).unwrap();
        let output = entry.output.unwrap();
        assert!(output.contains("PKG=mypkg"));
        assert!(output.contains(&format!("ROOT={}", default_target.display())));
        assert!(output.contains(&format!(
            "DIR={}",
            pkg_dir.path().parent().unwrap().display()
        )));
    }

    #[test]
    fn execute_setup_expands_explicit_target_and_ignores_default() {
        let pkg_dir = TempDir::new().unwrap();
        let cfg = PackageConfig {
            setup: Some("echo ROOT=$DOTM_SETUP_ROOT".to_string()),
            target: Some("$HOME/explicit-target".to_string()),
            ..Default::default()
        };
        let default_target = pkg_dir.path().join("should-not-be-used");
        let entry = execute_setup("mypkg", pkg_dir.path(), &cfg, &default_target).unwrap();
        let output = entry.output.unwrap();
        let home = std::env::var("HOME").unwrap();
        assert!(output.contains(&format!("ROOT={home}/explicit-target")));
        assert!(!output.contains("should-not-be-used"));
    }

    #[test]
    fn execute_setup_custom_shell_with_flags() {
        let pkg_dir = TempDir::new().unwrap();
        let cfg = PackageConfig {
            setup: Some("echo flagged".to_string()),
            setup_shell: Some("sh -x".to_string()),
            ..Default::default()
        };
        let entry = execute_setup("mypkg", pkg_dir.path(), &cfg, pkg_dir.path()).unwrap();
        assert_eq!(entry.status, SetupStatus::Success);
        assert!(entry.output.unwrap().contains("flagged"));
    }

    #[test]
    fn force_flag_overrides_skip() {
        let dir = TempDir::new().unwrap();
        let hash = compute_setup_hash(dir.path(), "echo hi").unwrap();
        let mut state = SetupState::new(dir.path());
        state.update("pkg".to_string(), make_success_entry(&hash));

        let (run, reason) = should_run_setup("pkg", &hash, &state, true);
        assert!(run);
        assert_eq!(reason, "forced re-run");
    }

    // --- Orchestrator test helpers ---

    fn write_fixture(dir: &Path) {
        std::fs::write(
            dir.join("dotm.toml"),
            r#"
[dotm]
target = "~"

[packages.a]
description = "a"
setup = "echo setup-a"

[packages.b]
description = "b"
setup = "exit 3"

[packages.c]
description = "c"
setup = "echo setup-c"
setup_after = ["a"]

[packages.no-setup]
description = "no setup field"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("packages/a")).unwrap();
        std::fs::create_dir_all(dir.join("packages/b")).unwrap();
        std::fs::create_dir_all(dir.join("packages/c")).unwrap();
        std::fs::create_dir_all(dir.join("packages/no-setup")).unwrap();
        std::fs::create_dir_all(dir.join("hosts")).unwrap();
        std::fs::create_dir_all(dir.join("roles")).unwrap();
        std::fs::write(
            dir.join("hosts/test-host.toml"),
            "hostname = \"test-host\"\nroles = [\"all\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("roles/all.toml"),
            "packages = [\"a\", \"b\", \"c\", \"no-setup\"]\n",
        )
        .unwrap();
    }

    fn write_system_fixture(dir: &Path) {
        std::fs::write(
            dir.join("dotm.toml"),
            r#"
[dotm]
target = "~"

[packages.user-pkg]
description = "user-space package"
setup = "echo user-setup"

[packages.sys-pkg]
description = "system package"
setup = "echo sys-setup"
system = true
target = "/tmp/dotm-system-fixture-target"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("packages/user-pkg")).unwrap();
        std::fs::create_dir_all(dir.join("packages/sys-pkg")).unwrap();
        std::fs::create_dir_all(dir.join("hosts")).unwrap();
        std::fs::create_dir_all(dir.join("roles")).unwrap();
        std::fs::write(
            dir.join("hosts/test-host.toml"),
            "hostname = \"test-host\"\nroles = [\"all\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("roles/all.toml"),
            "packages = [\"user-pkg\", \"sys-pkg\"]\n",
        )
        .unwrap();
    }

    /// Extract package names from a slice of SetupResultEntry for assertions.
    fn pkg_names(entries: &[SetupResultEntry]) -> Vec<String> {
        entries.iter().map(|e| e.package.clone()).collect()
    }

    fn test_orch(loader: ConfigLoader, state_dir: &Path) -> SetupOrchestrator {
        SetupOrchestrator::new(
            loader,
            state_dir.to_path_buf(),
            false,
            PathBuf::from("/tmp/dotm-setup-test-target"),
        )
    }

    #[test]
    fn orchestrator_runs_all_setup_packages_in_order() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch.run("test-host", None, false, false).unwrap();

        assert!(pkg_names(&report.succeeded).contains(&"a".to_string()));
        assert!(report.failed.iter().any(|e| e.package == "b"));
        // Verify stop-on-failure: "c" comes after "b" in resolved order
        // and must NOT have run.
        assert!(!pkg_names(&report.succeeded).contains(&"c".to_string()));
    }

    #[test]
    fn orchestrator_dry_run_does_not_execute_or_save_state() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch
            .run("test-host", Some(vec!["a".to_string()]), true, false)
            .unwrap();

        assert_eq!(report.dry_run.len(), 1);
        assert!(report.succeeded.is_empty());
        assert!(!state_dir.path().join("setup-state.json").exists());
    }

    #[test]
    fn orchestrator_package_filter_resolves_setup_after_dependencies_too() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch
            .run("test-host", Some(vec!["c".to_string()]), false, false)
            .unwrap();

        assert_eq!(pkg_names(&report.succeeded), vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn orchestrator_second_run_skips_unchanged_success() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        orch.run("test-host", Some(vec!["a".to_string()]), false, false)
            .unwrap();

        let loader2 = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch2 = test_orch(loader2, state_dir.path());
        let report2 = orch2
            .run("test-host", Some(vec!["a".to_string()]), false, false)
            .unwrap();

        assert_eq!(report2.succeeded.len(), 0);
        assert_eq!(report2.skipped.len(), 1);
    }

    #[test]
    fn orchestrator_force_reruns_successful_package() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        orch.run("test-host", Some(vec!["a".to_string()]), false, false)
            .unwrap();

        let loader2 = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch2 = test_orch(loader2, state_dir.path());
        let report2 = orch2
            .run("test-host", Some(vec!["a".to_string()]), false, true)
            .unwrap();

        assert_eq!(pkg_names(&report2.succeeded), vec!["a".to_string()]);
    }

    #[test]
    fn orchestrator_ignores_packages_without_setup_field() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch.run("test-host", None, true, false).unwrap();

        let all_dry_run_names: Vec<&String> = report.dry_run.iter().map(|(p, _, _)| p).collect();
        assert!(!all_dry_run_names.contains(&&"no-setup".to_string()));
    }

    #[test]
    fn orchestrator_system_mode_filters_by_system_flag() {
        let dotfiles = TempDir::new().unwrap();
        write_system_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch.run("test-host", None, false, false).unwrap();
        assert_eq!(pkg_names(&report.succeeded), vec!["user-pkg".to_string()]);

        let loader2 = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch2 = SetupOrchestrator::new(
            loader2,
            state_dir.path().to_path_buf(),
            true,
            PathBuf::from("/tmp/dotm-setup-test-target"),
        );
        let report2 = orch2.run("test-host", None, false, false).unwrap();
        assert_eq!(pkg_names(&report2.succeeded), vec!["sys-pkg".to_string()]);
    }

    // --- List tests ---

    #[test]
    fn list_shows_not_run_for_fresh_packages() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let entries = orch.list("test-host", None).unwrap();

        let names: Vec<&String> = entries.iter().map(|e| &e.package).collect();
        assert!(names.contains(&&"a".to_string()));
        assert!(!names.contains(&&"no-setup".to_string()));

        let a_entry = entries.iter().find(|e| e.package == "a").unwrap();
        assert!(matches!(a_entry.status, SetupListStatus::NotRun));
    }

    #[test]
    fn list_shows_success_after_run() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        orch.run("test-host", Some(vec!["a".to_string()]), false, false)
            .unwrap();

        let loader2 = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch2 = test_orch(loader2, state_dir.path());
        let entries = orch2.list("test-host", None).unwrap();
        let a_entry = entries.iter().find(|e| e.package == "a").unwrap();
        assert!(matches!(a_entry.status, SetupListStatus::Success(_)));
    }

    #[test]
    fn list_shows_changed_when_script_hash_differs() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        orch.run("test-host", Some(vec!["a".to_string()]), false, false)
            .unwrap();

        let toml_path = dotfiles.path().join("dotm.toml");
        let content = std::fs::read_to_string(&toml_path).unwrap();
        let content = content.replace("echo setup-a", "echo setup-a-changed");
        std::fs::write(&toml_path, content).unwrap();

        let loader2 = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch2 = test_orch(loader2, state_dir.path());
        let entries = orch2.list("test-host", None).unwrap();
        let a_entry = entries.iter().find(|e| e.package == "a").unwrap();
        assert!(matches!(a_entry.status, SetupListStatus::Changed));
    }

    #[test]
    fn list_shows_failed_with_error() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        orch.run("test-host", Some(vec!["b".to_string()]), false, false)
            .unwrap();

        let loader2 = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch2 = test_orch(loader2, state_dir.path());
        let entries = orch2.list("test-host", None).unwrap();
        let b_entry = entries.iter().find(|e| e.package == "b").unwrap();
        assert!(matches!(b_entry.status, SetupListStatus::Failed(_)));
        assert!(b_entry.error.is_some());
    }

    #[test]
    fn report_carries_captured_output_for_failures() {
        let dotfiles = TempDir::new().unwrap();
        // Use a command that produces stderr output before failing,
        // so we can assert the output field is actually populated.
        std::fs::write(
            dotfiles.path().join("dotm.toml"),
            r#"
[dotm]
target = "~"

[packages.noisy-fail]
description = "fails with output"
setup = "echo diagnostics-here >&2; exit 1"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dotfiles.path().join("packages/noisy-fail")).unwrap();
        std::fs::create_dir_all(dotfiles.path().join("hosts")).unwrap();
        std::fs::create_dir_all(dotfiles.path().join("roles")).unwrap();
        std::fs::write(
            dotfiles.path().join("hosts/test-host.toml"),
            "hostname = \"test-host\"\nroles = [\"all\"]\n",
        )
        .unwrap();
        std::fs::write(
            dotfiles.path().join("roles/all.toml"),
            "packages = [\"noisy-fail\"]\n",
        )
        .unwrap();
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch.run("test-host", None, false, false).unwrap();

        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0]
            .output
            .as_deref()
            .unwrap()
            .contains("diagnostics-here"));
    }

    #[test]
    fn report_carries_captured_output_for_success() {
        let dotfiles = TempDir::new().unwrap();
        write_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let report = orch
            .run("test-host", Some(vec!["a".to_string()]), false, false)
            .unwrap();

        assert_eq!(report.succeeded.len(), 1);
        assert!(report.succeeded[0]
            .output
            .as_deref()
            .unwrap()
            .contains("setup-a"));
    }

    #[test]
    fn list_respects_system_mode() {
        let dotfiles = TempDir::new().unwrap();
        write_system_fixture(dotfiles.path());
        let state_dir = TempDir::new().unwrap();

        let loader = ConfigLoader::new(dotfiles.path()).unwrap();
        let orch = test_orch(loader, state_dir.path());
        let entries = orch.list("test-host", None).unwrap();
        let names: Vec<&String> = entries.iter().map(|e| &e.package).collect();
        assert!(names.contains(&&"user-pkg".to_string()));
        assert!(!names.contains(&&"sys-pkg".to_string()));
    }
}
