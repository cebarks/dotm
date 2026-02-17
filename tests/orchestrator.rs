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
    assert!(!report.created.is_empty(), "expected some files to be created");

    // Check that .bashrc was symlinked through staging
    assert!(target_dir.path().join(".bashrc").is_symlink());
    let bashrc_link = std::fs::read_link(target_dir.path().join(".bashrc")).unwrap();
    assert!(
        bashrc_link.to_str().unwrap().contains(".staged"),
        "symlink should point into .staged/, got: {}",
        bashrc_link.display()
    );
    // Check that editor config was deployed (editor package, pulled in by dev role, depends on shell)
    assert!(target_dir.path().join(".config/nvim/init.lua").is_symlink());
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
