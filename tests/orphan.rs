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
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    // Redeploy — editor's files should be detected as orphans
    let mut orch2 = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch2.deploy("testhost", false, false).unwrap();

    assert!(!report.orphaned.is_empty(), "should detect orphaned files");
}

#[test]
fn deploy_preserves_orphaned_entries_for_later_prune() {
    let target = TempDir::new().unwrap();
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();

    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();

    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();
    let orphan = target.path().join(".config/nvim/init.lua");
    assert!(report.orphaned.contains(&orphan));
    assert!(
        orphan.exists(),
        "deploy without auto_prune must leave the file"
    );

    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(
        state.entries().iter().any(|entry| entry.target == orphan),
        "the orphan must remain tracked so dotm prune can find it"
    );
    assert_eq!(
        state.entries().len(),
        2,
        "merging retained orphan entries must not duplicate existing state"
    );

    let mut prune_scan = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let prune_report = prune_scan.deploy("testhost", true, false).unwrap();
    assert!(
        prune_report.orphaned.contains(&orphan),
        "a later dry-run scan must still identify the orphan"
    );
}

#[test]
fn deploy_drops_orphaned_entries_when_target_is_already_missing() {
    let target = TempDir::new().unwrap();
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();

    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();

    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();
    let orphan = target.path().join(".config/nvim/init.lua");
    std::fs::remove_file(&orphan).unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();
    assert!(report.orphaned.contains(&orphan));

    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(
        state.entries().iter().all(|entry| entry.target != orphan),
        "a missing orphan must not remain tracked and keep status failing"
    );
}

#[test]
fn auto_prune_retains_state_when_target_cannot_be_removed() {
    let target = TempDir::new().unwrap();
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

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

    let orphan = target.path().join(".config/nvim/init.lua");
    std::fs::remove_file(&orphan).unwrap();
    std::fs::create_dir(&orphan).unwrap();
    std::fs::write(orphan.join("blocker"), "keep directory non-empty").unwrap();
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.orphaned.contains(&orphan));
    assert!(
        report.pruned.is_empty(),
        "a target that could not be removed must not be reported as pruned"
    );
    assert!(orphan.is_dir());
    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(
        state.entries().iter().any(|entry| entry.target == orphan),
        "a target that could not be removed must remain tracked"
    );
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
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

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

#[test]
fn auto_prune_removes_orphaned_entries_from_state() {
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

    // Remove editor from dev role
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    // Redeploy with auto_prune — orphan should be pruned, not resurrected in state
    let mut orch2 = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch2.deploy("testhost", false, false).unwrap();
    assert!(!report.pruned.is_empty(), "should prune orphaned files");

    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(
        !state.entries().iter().any(|e| e.package == "editor"),
        "pruned editor entry must not be resurrected in persisted state"
    );

    // Redeploying again must not re-detect or re-prune the same (already-gone) file
    let mut orch3 = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report3 = orch3.deploy("testhost", false, false).unwrap();
    assert!(
        report3.orphaned.is_empty(),
        "already-pruned entry must not resurface as orphaned on the next deploy"
    );
}
