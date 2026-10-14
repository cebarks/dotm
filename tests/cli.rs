use assert_cmd::Command;
use predicates::prelude::*;
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

fn isolated_dotm_command(dotfiles: &Path, home: &Path) -> Command {
    let mut command = Command::cargo_bin("dotm").unwrap();
    command
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join(".xdg-state"))
        .args(["-d", dotfiles.to_str().unwrap()]);
    command
}

fn set_editor_target(dotfiles: &Path, target: &Path, auto_prune: bool) {
    let config_path = dotfiles.join("dotm.toml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    let config = if auto_prune {
        config.replace(
            "[dotm]\ntarget = \"~\"",
            "[dotm]\ntarget = \"~\"\nauto_prune = true",
        )
    } else {
        config
    };
    let target = format!("{:?}", target.display());
    let config = config.replace(
        "suggests = [\"theme\"]",
        &format!("suggests = [\"theme\"]\ntarget = {target}"),
    );
    std::fs::write(config_path, config).unwrap();
}

fn run_git(directory: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_version() {
    Command::cargo_bin("dotm")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("dotm"));
}

#[test]
fn cli_help() {
    Command::cargo_bin("dotm")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dotfile manager"));
}

#[test]
fn cli_check_valid_config() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", dotfiles.path().to_str().unwrap(), "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));
}

#[test]
fn cli_check_missing_config() {
    let empty = TempDir::new().unwrap();

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", empty.path().to_str().unwrap(), "check"])
        .assert()
        .failure();
}

#[test]
fn cli_init_creates_package_dir() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", dotfiles.path().to_str().unwrap(), "init", "newpkg"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    assert!(dotfiles.path().join("packages/newpkg").is_dir());
}

#[test]
fn cli_deploy_dry_run() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
}

#[test]
fn cli_restore_keeps_package_target_root() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let package_parent = TempDir::new().unwrap();
    let package_root = package_parent.path().join("editor-target");
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());
    set_editor_target(dotfiles.path(), &package_root, false);

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();
    let target = package_root.join(".config/nvim/init.lua");
    assert!(target.exists());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["restore", "--package", "editor"])
        .assert()
        .success();

    assert!(!target.exists());
    assert!(package_root.is_dir());
}

#[test]
fn cli_undeploy_keeps_package_target_root() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let package_parent = TempDir::new().unwrap();
    let package_root = package_parent.path().join("editor-target");
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());
    set_editor_target(dotfiles.path(), &package_root, false);

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();
    let target = package_root.join(".config/nvim/init.lua");
    assert!(target.exists());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["undeploy", "--package", "editor", "--no-hooks"])
        .assert()
        .success();

    assert!(!target.exists());
    assert!(package_root.is_dir());
}

#[test]
fn cli_prune_keeps_package_target_root() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let package_parent = TempDir::new().unwrap();
    let package_root = package_parent.path().join("editor-target");
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());
    set_editor_target(dotfiles.path(), &package_root, false);

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();
    let target = package_root.join(".config/nvim/init.lua");
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pruned 1 orphaned files"));

    assert!(!target.exists());
    assert!(package_root.is_dir());
}

#[test]
fn cli_deploy_dry_run_reports_orphans() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"))
        .stderr(predicate::str::contains("orphaned files remain"))
        .stderr(predicate::str::contains("Run 'dotm prune'"));
    assert!(home.path().join(".config/nvim/init.lua").exists());
}

#[test]
fn cli_prune_removes_orphan_after_deploy_warning() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();

    let orphan = home.path().join(".config/nvim/init.lua");
    assert!(orphan.exists());
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Run 'dotm prune'"));

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would prune 1 orphaned files"));
    assert!(
        orphan.exists(),
        "prune --dry-run must not delete the orphan"
    );

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pruned 1 orphaned files"));
    assert!(!orphan.exists() && !orphan.is_symlink());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No orphaned files to prune"));
}

#[test]
fn cli_auto_prune_reports_partial_failure() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    std::fs::create_dir_all(dotfiles.path().join("packages/editor/.config/nvim")).unwrap();
    std::fs::write(
        dotfiles
            .path()
            .join("packages/editor/.config/nvim/extra.conf"),
        "extra",
    )
    .unwrap();
    let config = std::fs::read_to_string(dotfiles.path().join("dotm.toml")).unwrap();
    std::fs::write(
        dotfiles.path().join("dotm.toml"),
        config.replace(
            "[dotm]\ntarget = \"~\"",
            "[dotm]\ntarget = \"~\"\nauto_prune = true",
        ),
    )
    .unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();

    let failed_orphan = home.path().join(".config/nvim/init.lua");
    std::fs::remove_file(&failed_orphan).unwrap();
    std::fs::create_dir(&failed_orphan).unwrap();
    std::fs::write(failed_orphan.join("blocker"), "keep directory non-empty").unwrap();
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pruned 1 orphaned files"))
        .stderr(predicate::str::contains("failed to prune orphaned file"))
        .stderr(predicate::str::contains("orphaned files remain"))
        .stderr(predicate::str::contains("Remove these paths manually"))
        .stderr(predicate::str::contains("Run 'dotm prune'").not());
    assert!(failed_orphan.is_dir());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would prune 1 orphaned files"));
}

#[test]
fn cli_prune_keeps_state_when_deletion_fails() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();

    let orphan = home.path().join(".config/nvim/init.lua");
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();
    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();

    std::fs::remove_file(&orphan).unwrap();
    std::fs::create_dir(&orphan).unwrap();
    std::fs::write(orphan.join("blocker"), "keep directory non-empty").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("No orphaned files were pruned"))
        .stderr(predicate::str::contains("failed to remove orphaned target"))
        .stderr(predicate::str::contains("Remove this path manually"));
    assert!(orphan.is_dir());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would prune 1 orphaned files"));
}

#[test]
fn cli_prune_saves_successful_removals_before_reporting_failure() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());
    let extra_source = dotfiles
        .path()
        .join("packages/editor/.config/nvim/extra.conf");
    std::fs::write(&extra_source, "extra").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();
    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();

    let failed_orphan = home.path().join(".config/nvim/init.lua");
    let successful_orphan = home.path().join(".config/nvim/extra.conf");
    std::fs::remove_file(&failed_orphan).unwrap();
    std::fs::create_dir(&failed_orphan).unwrap();
    std::fs::write(failed_orphan.join("blocker"), "keep directory non-empty").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Pruned 1 orphaned files"))
        .stderr(predicate::str::contains("failed to remove orphaned target"))
        .stderr(predicate::str::contains("Remove this path manually"));

    assert!(failed_orphan.is_dir());
    assert!(!successful_orphan.exists());
    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would prune 1 orphaned files"));
}

#[test]
fn cli_prune_reclaims_original_backup_after_saving_state() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());
    let target = home.path().join(".config/nvim/init.lua");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "pre-dotm original").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost", "--force"])
        .assert()
        .success();
    let state_dir = home.path().join(".dotm");
    let state = dotm::state::DeployState::load(&state_dir).unwrap();
    let original_hash = state
        .entries()
        .iter()
        .find(|entry| entry.target == target)
        .unwrap()
        .original_hash
        .clone()
        .unwrap();
    let originals = state_dir.join("originals").join(&original_hash);
    let deployed = state_dir.join("deployed").join(&original_hash);
    assert!(originals.exists() || deployed.exists());
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .success();

    assert!(!originals.exists() && !deployed.exists());
    let final_state = dotm::state::DeployState::load(&state_dir).unwrap();
    assert!(
        final_state
            .entries()
            .iter()
            .all(|entry| entry.target != target)
    );
}

#[test]
fn cli_prune_preserves_targets_when_package_source_is_missing() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();
    let target = home.path().join(".config/nvim/init.lua");
    std::fs::remove_dir_all(dotfiles.path().join("packages/editor")).unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No orphaned files to prune"))
        .stderr(predicate::str::contains("cannot determine desired targets"));

    assert!(target.is_symlink(), "prune must preserve the target link");
    let state = dotm::state::DeployState::load(&home.path().join(".dotm")).unwrap();
    assert!(state.entries().iter().any(|entry| entry.target == target));
}

#[test]
fn cli_prune_does_not_redeploy_modified_managed_files() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());
    let template = dotfiles
        .path()
        .join("packages/shell/.config/dotm/test.conf.tera");
    std::fs::create_dir_all(template.parent().unwrap()).unwrap();
    std::fs::write(&template, "managed version").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["deploy", "--host", "testhost"])
        .assert()
        .success();

    let active_target = home.path().join(".config/dotm/test.conf");
    assert_eq!(
        std::fs::read_to_string(&active_target).unwrap(),
        "managed version"
    );
    std::fs::write(&active_target, "user edit").unwrap();
    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["prune", "--host", "testhost"])
        .assert()
        .success();

    assert_eq!(std::fs::read_to_string(active_target).unwrap(), "user edit");
}

#[test]
fn cli_sync_reports_orphan_files() {
    let dotfiles = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    run_git(dotfiles.path(), &["init", "--quiet"]);
    run_git(dotfiles.path(), &["config", "user.name", "Dotm Test"]);
    run_git(
        dotfiles.path(),
        &["config", "user.email", "dotm-test@example.com"],
    );
    run_git(dotfiles.path(), &["add", "-A"]);
    run_git(
        dotfiles.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "--no-verify",
            "-m",
            "fixture",
        ],
    );

    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["sync", "--host", "testhost", "--no-push"])
        .assert()
        .success();

    std::fs::write(dotfiles.path().join("roles/dev.toml"), "packages = []\n").unwrap();
    isolated_dotm_command(dotfiles.path(), home.path())
        .args(["sync", "--host", "testhost", "--no-push"])
        .assert()
        .success()
        .stderr(predicate::str::contains("orphaned files remain"))
        .stderr(predicate::str::contains("Run 'dotm prune'"));
}

#[test]
fn cli_status_no_state() {
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    // Override HOME and XDG_STATE_HOME so status doesn't pick up the real user's state.
    // HOME is needed because dotm checks ~/.dotm/ before the XDG_STATE_HOME fallback.
    let fake_home = TempDir::new().unwrap();
    Command::cargo_bin("dotm")
        .unwrap()
        .env("HOME", fake_home.path())
        .env("XDG_STATE_HOME", state_dir.path())
        .args(["-d", dotfiles.path().to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No files"));
}

#[test]
fn cli_completions_bash() {
    Command::cargo_bin("dotm")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn cli_completions_zsh() {
    Command::cargo_bin("dotm")
        .unwrap()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("compdef"));
}

#[test]
fn cli_list_packages() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", dotfiles.path().to_str().unwrap(), "list", "packages"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shell"))
        .stdout(predicate::str::contains("editor"));
}

#[test]
fn cli_list_roles() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", dotfiles.path().to_str().unwrap(), "list", "roles"])
        .assert()
        .success()
        .stdout(predicate::str::contains("desktop"))
        .stdout(predicate::str::contains("dev"));
}

#[test]
fn cli_list_hosts() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", dotfiles.path().to_str().unwrap(), "list", "hosts"])
        .assert()
        .success()
        .stdout(predicate::str::contains("testhost"));
}

#[test]
fn cli_dotm_dir_env_var() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    // Without --dir, DOTM_DIR env var should be used
    Command::cargo_bin("dotm")
        .unwrap()
        .env("DOTM_DIR", dotfiles.path().to_str().unwrap())
        .args(["list", "packages"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shell"));
}

#[test]
fn cli_dir_flag_overrides_dotm_dir_env() {
    let dotfiles = TempDir::new().unwrap();
    let empty = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    // --dir flag should take precedence over DOTM_DIR
    Command::cargo_bin("dotm")
        .unwrap()
        .env("DOTM_DIR", empty.path().to_str().unwrap())
        .args(["-d", dotfiles.path().to_str().unwrap(), "list", "packages"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shell"));
}

#[test]
fn cli_list_hosts_tree() {
    let dotfiles = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args([
            "-d",
            dotfiles.path().to_str().unwrap(),
            "list",
            "hosts",
            "--tree",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("testhost"))
        .stdout(predicate::str::contains("shell"));
}

#[test]
fn cli_check_rejects_bad_setup_after() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("dotm.toml"),
        r#"
[dotm]
target = "~"

[packages.a]
setup = "echo a"
setup_after = ["missing"]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("packages/a")).unwrap();

    Command::cargo_bin("dotm")
        .unwrap()
        .args(["-d", dir.path().to_str().unwrap(), "check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("setup_after"));
}
