use crate::config::RootConfig;
use crate::hash;
use crate::setup_state::{SetupState, SetupStatus};
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::Path;

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
    use crate::config::{DotmSettings, PackageConfig, RootConfig};
    use crate::setup_state::{SetupEntry, SetupState, SetupStatus};
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
    fn force_flag_overrides_skip() {
        let dir = TempDir::new().unwrap();
        let hash = compute_setup_hash(dir.path(), "echo hi").unwrap();
        let mut state = SetupState::new(dir.path());
        state.update("pkg".to_string(), make_success_entry(&hash));

        let (run, reason) = should_run_setup("pkg", &hash, &state, true);
        assert!(run);
        assert_eq!(reason, "forced re-run");
    }
}
