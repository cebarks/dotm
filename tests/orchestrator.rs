use dotm::orchestrator::Orchestrator;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn full_deploy_basic_fixture() {
    let target_dir = TempDir::new().unwrap();
    let dotfiles_dir = Path::new("tests/fixtures/basic");

    let mut orch = Orchestrator::new(dotfiles_dir, target_dir.path()).unwrap();
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(
        report.conflicts.is_empty(),
        "unexpected conflicts: {:?}",
        report.conflicts
    );
    assert!(
        !report.created.is_empty(),
        "expected some files to be created"
    );

    // Check that .bashrc is a symlink pointing into packages/ (not .staged/)
    assert!(target_dir.path().join(".bashrc").is_symlink());
    let bashrc_link = std::fs::read_link(target_dir.path().join(".bashrc")).unwrap();
    assert!(
        bashrc_link.to_str().unwrap().contains("packages/"),
        "symlink should point into packages/, got: {}",
        bashrc_link.display()
    );
    assert!(
        !bashrc_link.to_str().unwrap().contains(".staged"),
        "symlink should NOT point into .staged/, got: {}",
        bashrc_link.display()
    );
    // Check that editor config was deployed (editor package, pulled in by dev role, depends on shell)
    assert!(target_dir.path().join(".config/nvim/init.lua").is_symlink());
    let nvim_link = std::fs::read_link(target_dir.path().join(".config/nvim/init.lua")).unwrap();
    assert!(
        nvim_link.to_str().unwrap().contains("packages/"),
        "symlink should point into packages/, got: {}",
        nvim_link.display()
    );
}

#[test]
fn full_deploy_dry_run_creates_nothing() {
    let target_dir = TempDir::new().unwrap();
    let dotfiles_dir = Path::new("tests/fixtures/basic");

    let mut orch = Orchestrator::new(dotfiles_dir, target_dir.path()).unwrap();
    let report = orch.deploy("testhost", true, false).unwrap();

    assert!(!report.dry_run_actions.is_empty());
    // Nothing should actually exist
    assert!(!target_dir.path().join(".bashrc").exists());
}

#[test]
fn partial_deploy_error_preserves_unreached_state_entries() {
    let target_dir = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    let dotfiles_dir = Path::new("tests/fixtures/basic");

    // First deploy: succeeds, state tracks all files
    let mut orch = Orchestrator::new(dotfiles_dir, target_dir.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();
    assert!(report.conflicts.is_empty());
    let initial_count = report.created.len();
    assert!(initial_count >= 2, "need at least 2 deployed files");

    // Verify state has all entries
    let state_before = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert_eq!(state_before.entries().len(), initial_count);

    // Make the nvim directory unwritable so deploy errors mid-way
    let nvim_dir = target_dir.path().join(".config/nvim");
    if nvim_dir.exists() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&nvim_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    // Second deploy: should error mid-way
    let mut orch2 = Orchestrator::new(dotfiles_dir, target_dir.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let result = orch2.deploy("testhost", false, true);

    // Restore permissions for cleanup
    if nvim_dir.exists() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&nvim_dir, std::fs::Permissions::from_mode(0o755));
    }

    // Deploy should have errored
    assert!(result.is_err(), "expected deploy to fail");

    // State should still have ALL entries (merged old + new)
    let state_after = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert_eq!(
        state_after.entries().len(),
        initial_count,
        "partial save must preserve unreached entries from previous state"
    );
}
