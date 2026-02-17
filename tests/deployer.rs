use dotm::deployer::{deploy_file, DeployResult};
use dotm::scanner::FileAction;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn deploy_symlink() {
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.profile");

    let action = FileAction {
        source: source.clone(),
        target_rel_path: PathBuf::from(".profile"),
        is_copy: false,
        is_template: false,
    };

    let result = deploy_file(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let deployed = target_dir.path().join(".profile");
    assert!(deployed.is_symlink());
    assert_eq!(
        std::fs::read_link(&deployed).unwrap(),
        std::fs::canonicalize(&source).unwrap()
    );
}

#[test]
fn deploy_copy() {
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.profile");

    let action = FileAction {
        source: source.clone(),
        target_rel_path: PathBuf::from(".profile"),
        is_copy: true,
        is_template: false,
    };

    let result = deploy_file(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let deployed = target_dir.path().join(".profile");
    assert!(deployed.exists());
    assert!(!deployed.is_symlink());
}

#[test]
fn deploy_creates_parent_dirs() {
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.config/theme.conf");

    let action = FileAction {
        source,
        target_rel_path: PathBuf::from(".config/theme.conf"),
        is_copy: false,
        is_template: false,
    };

    let result = deploy_file(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));
    assert!(target_dir.path().join(".config/theme.conf").is_symlink());
}

#[test]
fn deploy_skips_existing_unmanaged_file() {
    let target_dir = TempDir::new().unwrap();
    std::fs::write(target_dir.path().join(".profile"), "existing content").unwrap();

    let action = FileAction {
        source: PathBuf::from("tests/fixtures/overrides/packages/configs/.profile"),
        target_rel_path: PathBuf::from(".profile"),
        is_copy: false,
        is_template: false,
    };

    let result = deploy_file(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));
}

#[test]
fn deploy_force_overwrites_existing() {
    let target_dir = TempDir::new().unwrap();
    std::fs::write(target_dir.path().join(".profile"), "existing content").unwrap();

    let action = FileAction {
        source: PathBuf::from("tests/fixtures/overrides/packages/configs/.profile"),
        target_rel_path: PathBuf::from(".profile"),
        is_copy: false,
        is_template: false,
    };

    let result = deploy_file(&action, target_dir.path(), false, true, None).unwrap();
    assert!(matches!(result, DeployResult::Created));
    assert!(target_dir.path().join(".profile").is_symlink());
}

#[test]
fn deploy_template_renders_and_copies() {
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.config/templated.conf.tera");

    let action = FileAction {
        source,
        target_rel_path: PathBuf::from(".config/templated.conf"),
        is_copy: true,
        is_template: true,
    };

    let rendered = "rendered content here".to_string();

    let result = deploy_file(&action, target_dir.path(), false, false, Some(&rendered)).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let deployed = target_dir.path().join(".config/templated.conf");
    assert!(!deployed.is_symlink());
    assert_eq!(std::fs::read_to_string(&deployed).unwrap(), "rendered content here");
}

#[test]
fn deploy_dry_run_creates_nothing() {
    let target_dir = TempDir::new().unwrap();

    let action = FileAction {
        source: PathBuf::from("tests/fixtures/overrides/packages/configs/.profile"),
        target_rel_path: PathBuf::from(".profile"),
        is_copy: false,
        is_template: false,
    };

    let result = deploy_file(&action, target_dir.path(), true, false, None).unwrap();
    assert!(matches!(result, DeployResult::DryRun));
    assert!(!target_dir.path().join(".profile").exists());
}
