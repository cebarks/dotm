use dotm::scanner::EntryKind;
use dotm::state::{DeployEntry, DeployState};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn state_save_includes_version() {
    let dir = TempDir::new().unwrap();
    let mut state = DeployState::new(dir.path());
    state.record(DeployEntry {
        target: PathBuf::from("/home/user/.bashrc"),
        staged: None,
        source: PathBuf::from("/source/.bashrc"),
        content_hash: "abc".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "shell".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.save().unwrap();

    let raw = std::fs::read_to_string(dir.path().join("dotm-state.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["version"], 3);
}

#[test]
fn state_loads_unversioned_as_v1_and_migrates() {
    let dir = TempDir::new().unwrap();
    let v1_json = r#"{"entries":[]}"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join("dotm-state.json"), v1_json).unwrap();

    let state = DeployState::load(dir.path()).unwrap();
    assert!(state.entries().is_empty());
    state.save().unwrap();
    let raw = std::fs::read_to_string(dir.path().join("dotm-state.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["version"], 3);
}

#[test]
fn state_errors_on_future_version() {
    let dir = TempDir::new().unwrap();
    let future_json = r#"{"version":999,"entries":[]}"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join("dotm-state.json"), future_json).unwrap();

    let result = DeployState::load(dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("newer version"));
}

#[test]
fn update_entry_hash_changes_hash() {
    let dir = TempDir::new().unwrap();
    let mut state = DeployState::new(dir.path());
    state.record(DeployEntry {
        target: PathBuf::from("/t"),
        staged: None,
        source: PathBuf::from("/src"),
        content_hash: "old_hash".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.update_entry_hash(0, "new_hash".to_string());
    assert_eq!(state.entries()[0].content_hash, "new_hash");
}

#[test]
fn save_and_load_new_state() {
    let dir = TempDir::new().unwrap();
    let mut state = DeployState::new(dir.path());
    state.record(DeployEntry {
        target: PathBuf::from("/home/user/.bashrc"),
        staged: None,
        source: PathBuf::from("/home/user/dotfiles/packages/shell/.bashrc"),
        content_hash: "abc123".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "shell".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.record(DeployEntry {
        target: PathBuf::from("/home/user/.config/app.conf"),
        staged: None,
        source: PathBuf::from("/home/user/dotfiles/packages/configs/.config/app.conf##host.myhost"),
        content_hash: "def456".to_string(),
        original_hash: None,
        kind: EntryKind::Override,
        package: "configs".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.save().unwrap();

    let loaded = DeployState::load(dir.path()).unwrap();
    let entries = loaded.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].package, "shell");
    assert_eq!(entries[0].kind, EntryKind::Base);
    assert_eq!(entries[0].content_hash, "abc123");
    assert_eq!(entries[1].package, "configs");
    assert_eq!(entries[1].kind, EntryKind::Override);
}

#[test]
fn load_nonexistent_returns_empty() {
    let dir = TempDir::new().unwrap();
    let state = DeployState::load(dir.path()).unwrap();
    assert!(state.entries().is_empty());
}

#[test]
fn undeploy_removes_target() {
    let target_dir = TempDir::new().unwrap();
    let source_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join(".bashrc");
    std::fs::write(&source_path, "content").unwrap();

    let target_path = target_dir.path().join(".bashrc");
    std::os::unix::fs::symlink(&source_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: None,
        source: source_path.clone(),
        content_hash: "hash".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "shell".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.save().unwrap();

    let removed = state.undeploy().unwrap();
    assert_eq!(removed, 1);
    assert!(!target_path.exists());
    // Source should still exist (it's the dotfile source, not staged)
    assert!(source_path.exists());
}

#[test]
fn check_entry_status_detects_modified_copy() {
    let target_dir = TempDir::new().unwrap();

    let target_path = target_dir.path().join("test.conf");
    std::fs::write(&target_path, "original content").unwrap();
    let original_hash = dotm::hash::hash_content(b"original content");

    let state_dir = TempDir::new().unwrap();
    let state = DeployState::new(state_dir.path());

    let entry = DeployEntry {
        target: target_path.clone(),
        staged: None,
        source: PathBuf::from("/source/test.conf"),
        content_hash: original_hash,
        original_hash: None,
        kind: EntryKind::Template,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    };

    assert!(state.check_entry_status(&entry).is_ok());

    // Modify the target file
    std::fs::write(&target_path, "modified content").unwrap();
    assert!(state.check_entry_status(&entry).is_modified());
}

#[test]
fn check_entry_status_symlink_ok_when_pointing_to_source() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("test.conf");
    std::fs::write(&source_path, "content").unwrap();

    let target_path = target_dir.path().join("test.conf");
    std::os::unix::fs::symlink(&source_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let state = DeployState::new(state_dir.path());

    let entry = DeployEntry {
        target: target_path,
        staged: None,
        source: source_path,
        content_hash: dotm::hash::hash_content(b"content"),
        original_hash: None,
        kind: EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    };

    assert!(state.check_entry_status(&entry).is_ok());
}

#[test]
fn check_entry_status_symlink_missing_when_pointing_elsewhere() {
    let source_dir = TempDir::new().unwrap();
    let other_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("test.conf");
    std::fs::write(&source_path, "content").unwrap();

    let other_path = other_dir.path().join("other.conf");
    std::fs::write(&other_path, "other").unwrap();

    let target_path = target_dir.path().join("test.conf");
    std::os::unix::fs::symlink(&other_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let state = DeployState::new(state_dir.path());

    let entry = DeployEntry {
        target: target_path,
        staged: None,
        source: source_path,
        content_hash: dotm::hash::hash_content(b"content"),
        original_hash: None,
        kind: EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    };

    assert!(state.check_entry_status(&entry).is_missing());
}

#[test]
fn check_entry_status_detects_missing() {
    let state_dir = TempDir::new().unwrap();
    let state = DeployState::new(state_dir.path());

    let entry = DeployEntry {
        target: PathBuf::from("/nonexistent/target"),
        staged: None,
        source: PathBuf::from("irrelevant"),
        content_hash: "hash".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    };

    assert!(state.check_entry_status(&entry).is_missing());
}

#[test]
fn undeploy_cleans_empty_target_directories() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source_path = source_dir.path().join("file.conf");
    std::fs::write(&source_path, "content").unwrap();

    let target_parent = target_dir.path().join(".config/nested");
    std::fs::create_dir_all(&target_parent).unwrap();
    let target_path = target_parent.join("file.conf");
    std::os::unix::fs::symlink(&source_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: None,
        source: source_path,
        content_hash: "hash".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    state.save().unwrap();

    state.undeploy().unwrap();
    assert!(!target_path.exists());
    assert!(
        !target_parent.exists(),
        "empty target parent should be cleaned up"
    );
}

#[test]
fn restore_error_propagates_over_save_error() {
    let state_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    // Create a target that will fail to restore (parent dir doesn't exist
    // and we'll make it so it can't be created)
    let impossible_target = PathBuf::from("/nonexistent-root-dir/impossible/target");

    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: impossible_target.clone(),
        staged: None,
        source: PathBuf::from("/source"),
        content_hash: "hash".to_string(),
        original_hash: Some("orig_hash".to_string()),
        kind: EntryKind::Override,
        package: "failing_pkg".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    // Add another package so filter is meaningful
    state.record(DeployEntry {
        target: target_dir.path().join("other.conf"),
        staged: None,
        source: PathBuf::from("/source2"),
        content_hash: "hash2".to_string(),
        original_hash: None,
        kind: EntryKind::Base,
        package: "other_pkg".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });
    // Store an "original" so restore tries to write it back
    state
        .store_original("orig_hash", b"original content")
        .unwrap();
    state.save().unwrap();

    // Reload and attempt filtered restore — the restore will fail because
    // it can't write to /nonexistent-root-dir/...
    let mut loaded = DeployState::load(state_dir.path()).unwrap();

    // Make state dir read-only (no write) so save() fails, but keep execute for reading subdirs
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(state_dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = loaded.restore(Some("failing_pkg"));

    // Restore permissions for cleanup
    let _ = std::fs::set_permissions(state_dir.path(), std::fs::Permissions::from_mode(0o755));

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    // The error should be about restore failure (loading original or writing target),
    // not the save failure (temp state file)
    assert!(
        err_msg.contains("original content") || err_msg.contains("failed to restore"),
        "expected restore error, got: {err_msg}"
    );
}
