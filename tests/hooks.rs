use dotm::hooks::run_hook;
use tempfile::TempDir;

#[test]
fn run_hook_success() {
    let dir = TempDir::new().unwrap();
    let result = run_hook("true", dir.path(), "test-pkg", "deploy");
    assert!(result.is_ok());
}

#[test]
fn run_hook_failure_returns_error() {
    let dir = TempDir::new().unwrap();
    let result = run_hook("false", dir.path(), "test-pkg", "deploy");
    assert!(result.is_err());
}

#[test]
fn run_hook_sets_env_vars() {
    let dir = TempDir::new().unwrap();
    let out_file = dir.path().join("env_out");
    let cmd = format!(
        "echo $DOTM_PACKAGE,$DOTM_TARGET,$DOTM_ACTION > {}",
        out_file.display()
    );
    run_hook(&cmd, dir.path(), "mypkg", "deploy").unwrap();
    let content = std::fs::read_to_string(&out_file).unwrap();
    assert!(content.contains("mypkg"));
    assert!(content.contains("deploy"));
}

#[test]
fn empty_hook_is_noop() {
    let dir = TempDir::new().unwrap();
    let result = run_hook("", dir.path(), "test-pkg", "deploy");
    assert!(result.is_ok());
}

#[test]
fn undeploy_package_runs_hooks() {
    use dotm::scanner::EntryKind;
    use dotm::state::{DeployEntry, DeployState};
    use std::path::PathBuf;

    let target_dir = tempfile::TempDir::new().unwrap();
    let state_dir = tempfile::TempDir::new().unwrap();
    let marker = target_dir.path().join("hook_ran");

    // Create target file so undeploy has something to remove
    let target_path = target_dir.path().join("test.conf");
    std::fs::write(&target_path, "content").unwrap();

    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: None,
        source: PathBuf::from("/source/test.conf"),
        content_hash: "hash".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "mypkg".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.save().unwrap();

    // Build package config with hooks
    let mut pkg_config = dotm::config::PackageConfig::default();
    pkg_config.pre_undeploy = Some(format!("touch {}", marker.display()));

    let mut packages = std::collections::HashMap::new();
    packages.insert("mypkg".to_string(), pkg_config);

    let mut loaded = DeployState::load(state_dir.path()).unwrap();
    loaded
        .undeploy_package("mypkg", Some(&packages), target_dir.path())
        .unwrap();

    assert!(marker.exists(), "pre_undeploy hook should have run");
    assert!(!target_path.exists(), "target file should be removed");
}
