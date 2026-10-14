use dotm::config::RootConfig;
use dotm::orchestrator::Orchestrator;
use std::path::{Path, PathBuf};
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
fn resolved_target_roots_use_injected_default_and_package_override() {
    let package_target = TempDir::new().unwrap();
    let config = format!(
        "[dotm]\ntarget = \"~\"\n[packages.shell]\ntarget = \"{}\"\n",
        package_target.path().display()
    );
    let root: RootConfig = toml::from_str(&config).unwrap();
    let default_root = PathBuf::from("/injected/default-target");

    let roots = dotm::orchestrator::resolve_target_roots(&root, &default_root).unwrap();

    assert_eq!(roots.root_for("shell"), Some(package_target.path()));
    assert_eq!(roots.root_for("unconfigured"), Some(default_root.as_path()));
}

#[test]
fn auto_prune_keeps_package_target_root() {
    let target = TempDir::new().unwrap();
    let editor_root_parent = TempDir::new().unwrap();
    let editor_root = editor_root_parent.path().join("editor-target");
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    let config = std::fs::read_to_string(dotfiles.path().join("dotm.toml")).unwrap();
    let config = config
        .replace(
            "[dotm]\ntarget = \"~\"",
            "[dotm]\ntarget = \"~\"\nauto_prune = true",
        )
        .replace(
            "suggests = [\"theme\"]",
            &format!(
                "suggests = [\"theme\"]\ntarget = \"{}\"",
                editor_root.display()
            ),
        );
    std::fs::write(dotfiles.path().join("dotm.toml"), config).unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();
    let orphan = editor_root.join(".config/nvim/init.lua");
    assert!(orphan.exists());

    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();
    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.pruned.contains(&orphan));
    assert!(!orphan.exists());
    assert!(
        editor_root.is_dir(),
        "configured package target root must remain"
    );
}

#[test]
fn removed_package_override_skips_parent_cleanup_outside_default_root() {
    let target = TempDir::new().unwrap();
    let editor_root_parent = TempDir::new().unwrap();
    let editor_root = editor_root_parent.path().join("editor-target");
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    let config_path = dotfiles.path().join("dotm.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    let config = config
        .replace(
            "[dotm]\ntarget = \"~\"",
            "[dotm]\ntarget = \"~\"\nauto_prune = true",
        )
        .replace(
            "suggests = [\"theme\"]",
            &format!(
                "suggests = [\"theme\"]\ntarget = \"{}\"",
                editor_root.display()
            ),
        );
    std::fs::write(&config_path, config).unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();
    let orphan = editor_root.join(".config/nvim/init.lua");
    assert!(orphan.exists());

    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replace(&format!("target = \"{}\"\n", editor_root.display()), ""),
    )
    .unwrap();
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();
    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.pruned.contains(&orphan));
    assert!(!orphan.exists());
    assert!(
        orphan.parent().unwrap().is_dir(),
        "unknown external roots are not cleaned"
    );
}

#[test]
fn auto_prune_preserves_missing_package_and_prunes_other_orphans() {
    let target = TempDir::new().unwrap();
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    let config_path = dotfiles.path().join("dotm.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    let config = config
        .replace(
            "[dotm]\ntarget = \"~\"",
            "[dotm]\ntarget = \"~\"\nauto_prune = true",
        )
        .replace(
            "[packages.editor]",
            "[packages.extra]\ndescription = \"Extra package\"\n\n[packages.editor]",
        );
    std::fs::write(config_path, config).unwrap();
    let extra_source = dotfiles.path().join("packages/extra/extra.conf");
    std::fs::create_dir_all(extra_source.parent().unwrap()).unwrap();
    std::fs::write(&extra_source, "extra").unwrap();
    std::fs::write(
        dotfiles.path().join("roles/dev.toml"),
        "packages = [\"editor\", \"extra\"]\n",
    )
    .unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();
    let editor_target = target.path().join(".config/nvim/init.lua");
    let extra_target = target.path().join("extra.conf");
    assert!(editor_target.exists());
    assert!(extra_target.exists());

    std::fs::remove_dir_all(dotfiles.path().join("packages/editor")).unwrap();
    std::fs::write(
        dotfiles.path().join("roles/dev.toml"),
        "packages = [\"editor\"]\n",
    )
    .unwrap();
    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.orphaned.contains(&extra_target));
    assert!(!report.orphaned.contains(&editor_target));
    assert!(report.pruned.contains(&extra_target));
    assert!(
        editor_target.is_symlink(),
        "missing source leaves the target link tracked"
    );
    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(
        state
            .entries()
            .iter()
            .any(|entry| entry.target == editor_target)
    );
    assert!(
        state
            .entries()
            .iter()
            .all(|entry| entry.target != extra_target)
    );
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
    assert!(report.pruned.is_empty());
    assert_eq!(report.prune_failed, vec![orphan.clone()]);
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
