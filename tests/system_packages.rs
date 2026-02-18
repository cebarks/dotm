use dotm::orchestrator::Orchestrator;
use tempfile::TempDir;

fn setup_mixed_fixture(dir: &std::path::Path, system_target: &std::path::Path) {
    let system_target_str = system_target.display();

    // dotm.toml with both a system and user package
    std::fs::write(
        dir.join("dotm.toml"),
        format!(
            r#"
[dotm]
target = "~"

[packages.myservice]
description = "System service config"
system = true
target = "{system_target_str}"
strategy = "copy"

[packages.shell]
description = "Shell config"
"#
        ),
    )
    .unwrap();

    // packages/myservice/etc/myservice.conf
    let svc_dir = dir.join("packages/myservice/etc");
    std::fs::create_dir_all(&svc_dir).unwrap();
    std::fs::write(svc_dir.join("myservice.conf"), "key=value").unwrap();

    // packages/shell/.bashrc
    let shell_dir = dir.join("packages/shell");
    std::fs::create_dir_all(&shell_dir).unwrap();
    std::fs::write(shell_dir.join(".bashrc"), "# bashrc").unwrap();

    // host config
    let hosts_dir = dir.join("hosts");
    std::fs::create_dir_all(&hosts_dir).unwrap();
    std::fs::write(
        hosts_dir.join("testhost.toml"),
        r#"
hostname = "testhost"
roles = ["all"]
"#,
    )
    .unwrap();

    // role config
    let roles_dir = dir.join("roles");
    std::fs::create_dir_all(&roles_dir).unwrap();
    std::fs::write(
        roles_dir.join("all.toml"),
        r#"
packages = ["myservice", "shell"]
"#,
    )
    .unwrap();
}

#[test]
fn system_mode_only_deploys_system_packages() {
    let dotfiles = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let system_target = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();

    setup_mixed_fixture(dotfiles.path(), system_target.path());

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state.path())
        .with_system_mode(true);

    let report = orch.deploy("testhost", false, false).unwrap();

    // System package should be deployed to system_target
    let service_conf = system_target.path().join("etc/myservice.conf");
    assert!(
        service_conf.exists(),
        "system package file should be deployed in system mode"
    );
    assert_eq!(
        std::fs::read_to_string(&service_conf).unwrap(),
        "key=value"
    );

    // User package should NOT be deployed (to either target)
    let bashrc = target.path().join(".bashrc");
    assert!(
        !bashrc.exists(),
        "user packages should be skipped in system mode"
    );

    assert!(!report.created.is_empty());
}

#[test]
fn user_mode_skips_system_packages() {
    let dotfiles = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let system_target = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();

    setup_mixed_fixture(dotfiles.path(), system_target.path());

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state.path())
        .with_system_mode(false);

    let report = orch.deploy("testhost", false, false).unwrap();

    // User package should be deployed
    let bashrc = target.path().join(".bashrc");
    assert!(bashrc.exists(), "user packages should deploy in user mode");

    // System package should NOT be deployed
    let service_conf = system_target.path().join("etc/myservice.conf");
    assert!(
        !service_conf.exists(),
        "system packages should be skipped in user mode"
    );

    assert!(!report.created.is_empty());
}

#[test]
fn system_deploy_uses_separate_staging() {
    let dotfiles = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let system_target = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();

    // Create a system package with stage strategy
    std::fs::write(
        dotfiles.path().join("dotm.toml"),
        format!(
            r#"
[dotm]
target = "~"

[packages.sysconfig]
system = true
target = "{}"
strategy = "stage"
"#,
            system_target.path().display()
        ),
    )
    .unwrap();

    let pkg_dir = dotfiles.path().join("packages/sysconfig/etc");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("test.conf"), "staged content").unwrap();

    let hosts_dir = dotfiles.path().join("hosts");
    std::fs::create_dir_all(&hosts_dir).unwrap();
    std::fs::write(
        hosts_dir.join("testhost.toml"),
        r#"
hostname = "testhost"
roles = ["sys"]
"#,
    )
    .unwrap();

    let roles_dir = dotfiles.path().join("roles");
    std::fs::create_dir_all(&roles_dir).unwrap();
    std::fs::write(
        roles_dir.join("sys.toml"),
        r#"
packages = ["sysconfig"]
"#,
    )
    .unwrap();

    let mut orch = Orchestrator::new(dotfiles.path(), target.path())
        .unwrap()
        .with_state_dir(state.path())
        .with_system_mode(true);

    let report = orch.deploy("testhost", false, false).unwrap();
    assert!(!report.created.is_empty());

    // Staging should be in state dir, NOT in dotfiles dir
    let dotfiles_staged = dotfiles.path().join(".staged");
    assert!(
        !dotfiles_staged.exists()
            || std::fs::read_dir(&dotfiles_staged)
                .unwrap()
                .next()
                .is_none(),
        "system packages should not stage in the dotfiles .staged/ directory"
    );

    let state_staged = state.path().join(".staged");
    assert!(
        state_staged.exists(),
        "system packages should stage in the state dir .staged/"
    );
}
