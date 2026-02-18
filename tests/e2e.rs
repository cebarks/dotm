use dotm::orchestrator::Orchestrator;
use std::path::Path;
use tempfile::TempDir;

/// Copy a fixture directory to a temp dir so tests don't race on .staged/
fn use_fixture(fixture: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let src = Path::new("tests/fixtures").join(fixture);
    copy_dir_recursive(&src, tmp.path());
    tmp
}

#[test]
fn e2e_deploy_and_undeploy() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("basic");

    let mut orch = Orchestrator::new(dotfiles.path(), target.path()).unwrap();
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());
    assert!(target.path().join(".bashrc").exists());
    assert!(target.path().join(".config/nvim/init.lua").exists());

    // Symlink should point into .staged/, not packages/
    let bashrc_link = std::fs::read_link(target.path().join(".bashrc")).unwrap();
    assert!(
        bashrc_link.to_str().unwrap().contains(".staged"),
        "symlink should point into .staged/, got: {}",
        bashrc_link.display()
    );
}

#[test]
fn e2e_deploy_with_overrides() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("overrides");

    let mut orch = Orchestrator::new(dotfiles.path(), target.path()).unwrap();
    let report = orch.deploy("myhost", false, false).unwrap();

    assert!(
        report.conflicts.is_empty(),
        "unexpected conflicts: {:?}",
        report.conflicts
    );
    // app.conf is now a symlink (via staging), not a direct copy
    let app_conf = target.path().join(".config/app.conf");
    assert!(app_conf.exists());
    assert!(app_conf.is_symlink());

    // The host override content should be used
    let content = std::fs::read_to_string(&app_conf).unwrap();
    assert!(
        content.contains("myhost"),
        "expected host override content, got: {content}"
    );
}

#[test]
fn e2e_deploy_with_template_rendering() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("overrides");

    let mut orch = Orchestrator::new(dotfiles.path(), target.path()).unwrap();
    let report = orch.deploy("myhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());

    // templated.conf is now a symlink (via staging), content still readable through it
    let templated = target.path().join(".config/templated.conf");
    assert!(templated.exists());
    assert!(templated.is_symlink());
    let content = std::fs::read_to_string(&templated).unwrap();
    assert!(
        content.contains("blue"),
        "expected rendered template with color=blue, got: {content}"
    );
}

#[test]
fn e2e_idempotent_deploy() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("basic");

    let mut orch = Orchestrator::new(dotfiles.path(), target.path()).unwrap();
    orch.deploy("testhost", false, false).unwrap();

    // Deploy again — should succeed without conflicts (symlinks get replaced)
    let mut orch2 = Orchestrator::new(dotfiles.path(), target.path()).unwrap();
    let report2 = orch2.deploy("testhost", false, false).unwrap();
    assert!(
        report2.conflicts.is_empty(),
        "idempotent deploy had conflicts: {:?}",
        report2.conflicts
    );
}

#[test]
fn e2e_role_override_when_no_host_match() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("overrides");

    // Create a new host that uses the desktop role but isn't "myhost"
    std::fs::write(
        dotfiles.path().join("hosts/althost.toml"),
        "hostname = \"althost\"\nroles = [\"desktop\"]\n\n[vars]\napp.color = \"red\"\n",
    )
    .unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path()).unwrap();
    let report = orch.deploy("althost", false, false).unwrap();

    assert!(report.conflicts.is_empty());

    // app.conf should use the role override (desktop), not the host override (myhost)
    let app_conf = target.path().join(".config/app.conf");
    let content = std::fs::read_to_string(&app_conf).unwrap();
    assert!(
        content.contains("desktop"),
        "expected role override content, got: {content}"
    );
}

#[test]
fn e2e_deploy_stages_all_files() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("basic");
    let state_dir = TempDir::new().unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());

    // All target files should be symlinks pointing into .staged/
    let bashrc = target.path().join(".bashrc");
    assert!(bashrc.is_symlink());
    let link = std::fs::read_link(&bashrc).unwrap();
    assert!(link.to_str().unwrap().contains(".staged"));

    // State should have entries with content hashes
    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(!state.entries().is_empty());
    for entry in state.entries() {
        assert!(
            !entry.content_hash.is_empty(),
            "content hash should be populated"
        );
    }
}

#[test]
fn e2e_collision_detection() {
    let dotfiles_tmp = TempDir::new().unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("dotm.toml"),
        "[dotm]\ntarget = \"~\"\n\n[packages.pkg_a]\ndescription = \"A\"\n\n[packages.pkg_b]\ndescription = \"B\"\n",
    )
    .unwrap();

    let pkg_a = dotfiles_tmp.path().join("packages/pkg_a/.config");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::write(pkg_a.join("collision.conf"), "from a").unwrap();

    let pkg_b = dotfiles_tmp.path().join("packages/pkg_b/.config");
    std::fs::create_dir_all(&pkg_b).unwrap();
    std::fs::write(pkg_b.join("collision.conf"), "from b").unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("hosts")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("hosts/testhost.toml"),
        "hostname = \"testhost\"\nroles = [\"all\"]\n",
    )
    .unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("roles")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("roles/all.toml"),
        "packages = [\"pkg_a\", \"pkg_b\"]\n",
    )
    .unwrap();

    let target = TempDir::new().unwrap();
    let mut orch = Orchestrator::new(dotfiles_tmp.path(), target.path()).unwrap();
    let result = orch.deploy("testhost", false, false);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("collision"));
}

#[test]
fn e2e_permission_override_applied() {
    use std::os::unix::fs::PermissionsExt;

    let dotfiles_tmp = TempDir::new().unwrap();

    std::fs::write(
        dotfiles_tmp.path().join("dotm.toml"),
        r#"
[dotm]
target = "~"

[packages.scripts]
description = "Scripts"

[packages.scripts.permissions]
"bin/myscript" = "755"
"#,
    )
    .unwrap();

    let pkg_dir = dotfiles_tmp.path().join("packages/scripts/bin");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("myscript"), "#!/bin/bash\necho hi").unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("hosts")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("hosts/testhost.toml"),
        "hostname = \"testhost\"\nroles = [\"all\"]\n",
    )
    .unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("roles")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("roles/all.toml"),
        "packages = [\"scripts\"]\n",
    )
    .unwrap();

    let target = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    let mut orch = Orchestrator::new(dotfiles_tmp.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();

    let staged = dotfiles_tmp.path().join(".staged/bin/myscript");
    let mode = staged.metadata().unwrap().permissions().mode();
    assert_eq!(
        mode & 0o777,
        0o755,
        "staged file should have 755 permissions"
    );
}

#[test]
fn e2e_deploy_single_package() {
    let target = TempDir::new().unwrap();
    let dotfiles = use_fixture("basic");
    let state_dir = TempDir::new().unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state_dir.path())
        .with_package_filter(Some("shell".to_string()));
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());
    // shell should be deployed
    assert!(target.path().join(".bashrc").exists());
    // editor should NOT be deployed (it's not in the filter)
    assert!(!target.path().join(".config/nvim/init.lua").exists());
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            // Skip .staged directories — they're test artifacts
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
