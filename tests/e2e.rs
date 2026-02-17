use dotm::orchestrator::Orchestrator;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn e2e_deploy_and_undeploy() {
    let target = TempDir::new().unwrap();
    let dotfiles = Path::new("tests/fixtures/basic");

    let mut orch = Orchestrator::new(dotfiles, target.path()).unwrap();
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());
    assert!(target.path().join(".bashrc").exists());
    assert!(target.path().join(".config/nvim/init.lua").exists());

    // Verify symlinks point to the right place
    let bashrc_link = std::fs::read_link(target.path().join(".bashrc")).unwrap();
    assert!(bashrc_link
        .to_str()
        .unwrap()
        .contains("packages/shell/.bashrc"));
}

#[test]
fn e2e_deploy_with_overrides() {
    let target = TempDir::new().unwrap();
    let dotfiles = Path::new("tests/fixtures/overrides");

    let mut orch = Orchestrator::new(dotfiles, target.path()).unwrap();
    let report = orch.deploy("myhost", false, false).unwrap();

    assert!(
        report.conflicts.is_empty(),
        "unexpected conflicts: {:?}",
        report.conflicts
    );
    // app.conf should be a copy (host override), not a symlink
    let app_conf = target.path().join(".config/app.conf");
    assert!(app_conf.exists());
    assert!(!app_conf.is_symlink());

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
    let dotfiles = Path::new("tests/fixtures/overrides");

    let mut orch = Orchestrator::new(dotfiles, target.path()).unwrap();
    let report = orch.deploy("myhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());

    // templated.conf should be rendered with vars from host config
    let templated = target.path().join(".config/templated.conf");
    assert!(templated.exists());
    assert!(!templated.is_symlink());
    let content = std::fs::read_to_string(&templated).unwrap();
    assert!(
        content.contains("blue"),
        "expected rendered template with color=blue, got: {content}"
    );
}

#[test]
fn e2e_idempotent_deploy() {
    let target = TempDir::new().unwrap();
    let dotfiles = Path::new("tests/fixtures/basic");

    let mut orch = Orchestrator::new(dotfiles, target.path()).unwrap();
    orch.deploy("testhost", false, false).unwrap();

    // Deploy again — should succeed without conflicts (symlinks get replaced)
    let mut orch2 = Orchestrator::new(dotfiles, target.path()).unwrap();
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
    let dotfiles = Path::new("tests/fixtures/overrides");

    // Deploy as a host that doesn't have a host-specific override but has the desktop role
    // We need a host config for this — create one in a temp dotfiles dir
    let dotfiles_tmp = TempDir::new().unwrap();
    copy_dir_recursive(dotfiles, dotfiles_tmp.path());

    // Create a new host that uses the desktop role but isn't "myhost"
    std::fs::write(
        dotfiles_tmp.path().join("hosts/althost.toml"),
        "hostname = \"althost\"\nroles = [\"desktop\"]\n\n[vars]\napp.color = \"red\"\n",
    )
    .unwrap();

    let mut orch = Orchestrator::new(dotfiles_tmp.path(), target.path()).unwrap();
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

fn copy_dir_recursive(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path).unwrap();
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}
