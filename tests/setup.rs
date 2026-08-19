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
            std::fs::create_dir_all(&dst_path).unwrap();
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
            #[cfg(unix)]
            if let Ok(meta) = std::fs::metadata(&src_path) {
                let _ = std::fs::set_permissions(&dst_path, meta.permissions());
            }
        }
    }
}

fn use_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/setup"), tmp.path());
    tmp
}

/// Isolate dotm_state_dir() (which resolves off $HOME) to a fresh tempdir.
fn isolated_cmd(dotfiles: &Path, state_dir: &Path, extra_args: &[&str]) -> Command {
    let mut cmd = Command::cargo_bin("dotm").unwrap();
    cmd.env("HOME", state_dir)
        .env("XDG_STATE_HOME", state_dir)
        .args(["-d", dotfiles.to_str().unwrap()])
        .args(extra_args);
    cmd
}

#[test]
fn setup_dry_run_does_not_execute() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "alpha", "--dry-run"],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Would execute"));
}

#[test]
fn setup_runs_and_reports_success() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "alpha"],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Setup succeeded: alpha"));
}

#[test]
fn setup_reports_failure_with_nonzero_exit() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "beta"],
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("Setup failed: beta"));
}

#[test]
fn setup_script_based_command_executes() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &[
            "setup", "--host", "test-host", "--package", "gamma", "--verbose",
        ],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("gamma script ran"));
}

#[test]
fn setup_second_run_skips_already_successful() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "alpha"],
    )
    .assert()
    .success();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "alpha"],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Setup skipped: alpha"));
}

#[test]
fn setup_force_reruns() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "alpha"],
    )
    .assert()
    .success();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &[
            "setup", "--host", "test-host", "--package", "alpha", "--force",
        ],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Setup succeeded: alpha"));
}

#[test]
fn setup_list_shows_not_run() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--list"],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Not run"));
}

#[test]
fn setup_respects_setup_after_ordering() {
    let dotfiles = use_fixture();
    let state_dir = TempDir::new().unwrap();

    // Running just "delta" (setup_after alpha) must run alpha first.
    isolated_cmd(
        dotfiles.path(),
        state_dir.path(),
        &["setup", "--host", "test-host", "--package", "delta"],
    )
    .assert()
    .success()
    .stdout(
        predicate::str::contains("Setup succeeded: alpha")
            .and(predicate::str::contains("Setup succeeded: delta")),
    );
}
