use dotm::deployer::{DeployResult, apply_permission_override, deploy_copy, deploy_symlink};
use dotm::scanner::{EntryKind, FileAction};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::TempDir;

// --- deploy_symlink tests ---

#[test]
fn symlink_base_file_to_source() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "base config content").unwrap();

    let action = FileAction {
        source: source_path.clone(),
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Created));

    // Target should be a symlink pointing directly to the source
    let target = target_dir.path().join(".config/app.conf");
    assert!(target.is_symlink());
    assert_eq!(
        std::fs::read_link(&target).unwrap(),
        std::fs::canonicalize(&source_path).unwrap()
    );

    // Content should match source
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "base config content"
    );
}

#[test]
fn symlink_replaces_existing_symlink() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("test.conf");
    std::fs::write(&source_path, "content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("test.conf"),
        kind: EntryKind::Base,
    };

    // First deploy — should be Created
    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Created));

    // Second deploy — should be Updated
    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Updated));

    // Target should still be a symlink
    let target = target_dir.path().join("test.conf");
    assert!(target.is_symlink());
}

#[test]
fn symlink_conflicts_with_unmanaged_regular_file() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Place an unmanaged regular file at the target path
    std::fs::write(target_dir.path().join("conflict.conf"), "I was here first").unwrap();

    let source_path = source_dir.path().join("conflict.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("conflict.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));

    // Original file should still exist
    let target = target_dir.path().join("conflict.conf");
    assert!(target.exists());
    assert!(!target.is_symlink());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "I was here first"
    );
}

#[test]
fn symlink_force_overwrites_regular_file() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Place an unmanaged regular file at the target path
    std::fs::write(target_dir.path().join("conflict.conf"), "I was here first").unwrap();

    let source_path = source_dir.path().join("conflict.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path.clone(),
        target_rel_path: PathBuf::from("conflict.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, true).unwrap();
    assert!(matches!(result, DeployResult::Updated));

    // Target should now be a symlink to the source
    let target = target_dir.path().join("conflict.conf");
    assert!(target.is_symlink());
    assert_eq!(
        std::fs::read_link(&target).unwrap(),
        std::fs::canonicalize(&source_path).unwrap()
    );
}

#[test]
fn symlink_errors_on_directory_target() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Create a directory at the target path
    std::fs::create_dir_all(target_dir.path().join(".config")).unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    match result {
        DeployResult::Conflict(msg) => {
            assert!(
                msg.contains("directory"),
                "expected 'directory' in error message, got: {}",
                msg
            );
        }
        _ => panic!("expected Conflict result for directory target"),
    }
}

#[test]
fn symlink_dry_run_creates_nothing() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "some content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), true, false).unwrap();
    assert!(matches!(result, DeployResult::DryRun));

    assert!(
        !target_dir.path().join(".config/app.conf").exists(),
        "dry run should not create target symlink"
    );
}

// --- deploy_copy tests ---

#[test]
fn copy_writes_rendered_template() {
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
    let result = deploy_copy(&action, target_dir.path(), false, false, Some(rendered)).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let target = target_dir.path().join(".config/app.conf");
    assert!(target.exists());
    assert!(
        !target.is_symlink(),
        "template should be a regular file, not a symlink"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "rendered template output"
    );
}

#[test]
fn copy_base_file_copies_from_source() {
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

    let result = deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let target = target_dir.path().join("script.sh");
    assert!(target.exists());
    assert!(
        !target.is_symlink(),
        "deploy_copy should create a regular file, not a symlink"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "#!/bin/sh\necho hello"
    );

    // Check permissions were preserved
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "copy should preserve source permissions");
}

#[test]
fn copy_errors_on_directory_target() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Create a directory at the target path
    std::fs::create_dir_all(target_dir.path().join(".config")).unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    match result {
        DeployResult::Conflict(msg) => {
            assert!(
                msg.contains("directory"),
                "expected 'directory' in error message, got: {}",
                msg
            );
        }
        _ => panic!("expected Conflict result for directory target"),
    }
}

#[test]
fn copy_replaces_existing_symlink() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Create a symlink at the target path (pointing to something else)
    let dummy_file = source_dir.path().join("dummy");
    std::fs::write(&dummy_file, "dummy content").unwrap();
    let target_path = target_dir.path().join("test.conf");
    std::os::unix::fs::symlink(&dummy_file, &target_path).unwrap();

    let source_path = source_dir.path().join("test.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("test.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Updated));

    // Target should now be a regular file (not symlink)
    assert!(!target_path.is_symlink());
    assert_eq!(
        std::fs::read_to_string(&target_path).unwrap(),
        "new content"
    );
}

#[test]
fn copy_conflicts_with_unmanaged_regular_file() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Place an unmanaged regular file at the target path
    std::fs::write(target_dir.path().join("conflict.conf"), "I was here first").unwrap();

    let source_path = source_dir.path().join("conflict.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("conflict.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));

    // Original file should still exist
    let target = target_dir.path().join("conflict.conf");
    assert!(target.exists());
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "I was here first"
    );
}

#[test]
fn copy_force_overwrites_regular_file() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    // Place an unmanaged regular file at the target path
    std::fs::write(target_dir.path().join("conflict.conf"), "I was here first").unwrap();

    let source_path = source_dir.path().join("conflict.conf");
    std::fs::write(&source_path, "new content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from("conflict.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), false, true, None).unwrap();
    assert!(matches!(result, DeployResult::Updated));

    // Target should now contain new content
    let target = target_dir.path().join("conflict.conf");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new content");
}

#[test]
fn copy_dry_run_creates_nothing() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("app.conf");
    std::fs::write(&source_path, "some content").unwrap();

    let action = FileAction {
        source: source_path,
        target_rel_path: PathBuf::from(".config/app.conf"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), true, false, None).unwrap();
    assert!(matches!(result, DeployResult::DryRun));

    assert!(
        !target_dir.path().join(".config/app.conf").exists(),
        "dry run should not create target file"
    );
}

// --- apply_permission_override tests ---

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
