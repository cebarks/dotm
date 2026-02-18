use dotm::orchestrator::Orchestrator;
use std::path::Path;
use tempfile::TempDir;

fn copy_dir_recursive(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            if src_path.file_name().unwrap() == ".staged" {
                continue;
            }
            std::fs::create_dir_all(&dst_path).unwrap();
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}

#[test]
fn deploy_detects_orphaned_files() {
    let target = TempDir::new().unwrap();
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();

    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();

    // Both .bashrc and .config/nvim/init.lua should be deployed
    assert!(target.path().join(".bashrc").exists());
    assert!(target.path().join(".config/nvim/init.lua").exists());

    // Now remove editor from the dev role config
    std::fs::write(
        dotfiles.path().join("roles/dev.toml"),
        "packages = []\n",
    )
    .unwrap();

    // Redeploy — editor's files should be detected as orphans
    let mut orch2 = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch2.deploy("testhost", false, false).unwrap();

    assert!(!report.orphaned.is_empty(), "should detect orphaned files");
}

#[test]
fn auto_prune_removes_orphaned_files() {
    let target = TempDir::new().unwrap();
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();

    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    // Enable auto_prune in config
    let config = std::fs::read_to_string(dotfiles.path().join("dotm.toml")).unwrap();
    let config = config.replace(
        "[dotm]\ntarget = \"~\"",
        "[dotm]\ntarget = \"~\"\nauto_prune = true",
    );
    std::fs::write(dotfiles.path().join("dotm.toml"), config).unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();

    assert!(target.path().join(".config/nvim/init.lua").exists());

    // Remove editor from dev role
    std::fs::write(
        dotfiles.path().join("roles/dev.toml"),
        "packages = []\n",
    )
    .unwrap();

    // Redeploy with auto_prune — orphans should be removed
    let mut orch2 = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch2.deploy("testhost", false, false).unwrap();

    assert!(!report.orphaned.is_empty(), "should detect orphaned files");
    assert!(!report.pruned.is_empty(), "should prune orphaned files");
    // The orphaned symlink should be gone
    assert!(!target.path().join(".config/nvim/init.lua").exists());
}
