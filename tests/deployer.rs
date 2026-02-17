use dotm::deployer::{apply_permission_override, deploy_copy, deploy_staged, DeployResult};
use dotm::scanner::{EntryKind, FileAction};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::TempDir;

// --- deploy_staged tests ---

#[test]
fn stage_base_file_copies_to_staging_and_symlinks_target() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "base config content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    // Staged file should be a real file with the right content
    let staged = staging_dir.path().join(".config/app.conf");
    assert!(staged.exists());
    assert!(!staged.is_symlink());
    assert_eq!(std::fs::read_to_string(&staged).unwrap(), "base config content");

    // Target should be a symlink pointing to the staged file
    let target = target_dir.path().join(".config/app.conf");
    assert!(target.is_symlink());
    assert_eq!(
        std::fs::read_link(&target).unwrap(),
        std::fs::canonicalize(&staged).unwrap()
    );
}

#[test]
fn stage_template_renders_to_staging_and_symlinks_target() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf.tera");
    std::fs::write(&source_path, "{{ raw_template }}").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Template,
    };

    let rendered = "rendered template output";
    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, Some(rendered)).unwrap();
    assert!(matches!(result, DeployResult::Created));

    // Staged file should contain the rendered content
    let staged = staging_dir.path().join(".config/app.conf");
    assert!(staged.exists());
    assert!(!staged.is_symlink());
    assert_eq!(std::fs::read_to_string(&staged).unwrap(), "rendered template output");

    // Target should be a symlink to the staged file
    let target = target_dir.path().join(".config/app.conf");
    assert!(target.is_symlink());
    assert_eq!(
        std::fs::read_link(&target).unwrap(),
        std::fs::canonicalize(&staged).unwrap()
    );
}

#[test]
fn stage_preserves_source_permissions() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("script.sh");
    std::fs::write(&source_path, "#!/bin/sh\necho hello").unwrap();
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("script.sh"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let staged = staging_dir.path().join("script.sh");
    let mode = std::fs::metadata(&staged).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "staged file should preserve source permissions");
}

#[test]
fn copy_strategy_copies_directly_to_target() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "copy strategy content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let target = target_dir.path().join(".config/app.conf");
    assert!(target.exists());
    assert!(!target.is_symlink(), "deploy_copy should create a real file, not a symlink");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "copy strategy content");
}

#[test]
fn stage_detects_conflict_with_unmanaged_file() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Place an unmanaged real file at the target path
    std::fs::write(target_dir.path().join("conflict.conf"), "I was here first").unwrap();

    let source_path = source_dir.path().join("conflict.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("conflict.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));

    // Nothing should have been staged
    assert!(!staging_dir.path().join("conflict.conf").exists());
}

#[test]
fn stage_force_overwrites_unmanaged_file() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Place an unmanaged real file at the target path
    std::fs::write(target_dir.path().join("conflict.conf"), "I was here first").unwrap();

    let source_path = source_dir.path().join("conflict.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("conflict.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, true, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    // Staged file should exist
    let staged = staging_dir.path().join("conflict.conf");
    assert!(staged.exists());
    assert_eq!(std::fs::read_to_string(&staged).unwrap(), "new content");

    // Target should be a symlink to the staged file
    let target = target_dir.path().join("conflict.conf");
    assert!(target.is_symlink());
}

#[test]
fn stage_dry_run_creates_nothing() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "some content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), true, false, None).unwrap();
    assert!(matches!(result, DeployResult::DryRun));

    assert!(!staging_dir.path().join(".config/app.conf").exists(), "dry run should not create staged file");
    assert!(!target_dir.path().join(".config/app.conf").exists(), "dry run should not create target symlink");
}

#[test]
fn apply_permission_override_sets_mode() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test_file");
    std::fs::write(&file_path, "content").unwrap();

    // Start with default permissions, then override to 755
    apply_permission_override(&file_path, "755").unwrap();
    let mode = std::fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);

    // Override to 600
    apply_permission_override(&file_path, "600").unwrap();
    let mode = std::fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    // Invalid octal string should error
    let err = apply_permission_override(&file_path, "xyz");
    assert!(err.is_err());
}
