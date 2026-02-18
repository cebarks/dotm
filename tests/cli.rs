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
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    Command::cargo_bin("dotm")
        .unwrap()
        .args([
            "-d",
            dotfiles.path().to_str().unwrap(),
            "deploy",
            "--host",
            "testhost",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
}

#[test]
fn cli_status_no_state() {
    let dotfiles = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    copy_dir_recursive(Path::new("tests/fixtures/basic"), dotfiles.path());

    // Override XDG_STATE_HOME so status doesn't pick up the real user's state
    Command::cargo_bin("dotm")
        .unwrap()
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
