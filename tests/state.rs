use dotm::scanner::EntryKind;
use dotm::state::{DeployEntry, DeployState};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn save_and_load_new_state() {
    let dir = TempDir::new().unwrap();
    let mut state = DeployState::new(dir.path());
    state.record(DeployEntry {
        target: PathBuf::from("/home/user/.bashrc"),
        staged: PathBuf::from("/home/user/dotfiles/.staged/.bashrc"),
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
        staged: PathBuf::from("/home/user/dotfiles/.staged/.config/app.conf"),
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
fn undeploy_removes_target_and_staged() {
    let target_dir = TempDir::new().unwrap();
    let staged_dir = TempDir::new().unwrap();

    let staged_path = staged_dir.path().join(".bashrc");
    std::fs::write(&staged_path, "content").unwrap();

    let target_path = target_dir.path().join(".bashrc");
    std::os::unix::fs::symlink(&staged_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: staged_path.clone(),
        source: PathBuf::from("irrelevant"),
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
    assert!(!staged_path.exists());
}

#[test]
fn check_entry_status_detects_modified() {
    let staged_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let staged_path = staged_dir.path().join("test.conf");
    std::fs::write(&staged_path, "original content").unwrap();
    let original_hash = dotm::hash::hash_content(b"original content");

    let target_path = target_dir.path().join("test.conf");
    std::os::unix::fs::symlink(&staged_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let state = DeployState::new(state_dir.path());

    let entry = DeployEntry {
        target: target_path,
        staged: staged_path.clone(),
        source: PathBuf::from("irrelevant"),
        content_hash: original_hash,
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

    // Modify the staged file
    std::fs::write(&staged_path, "modified content").unwrap();
    assert!(state.check_entry_status(&entry).is_modified());
}

#[test]
fn check_entry_status_detects_missing() {
    let state_dir = TempDir::new().unwrap();
    let state = DeployState::new(state_dir.path());

    let entry = DeployEntry {
        target: PathBuf::from("/nonexistent/target"),
        staged: PathBuf::from("/nonexistent/staged"),
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
fn undeploy_cleans_empty_staged_directories() {
    let staged_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let staged_parent = staged_dir.path().join(".config/nested");
    std::fs::create_dir_all(&staged_parent).unwrap();
    let staged_path = staged_parent.join("file.conf");
    std::fs::write(&staged_path, "content").unwrap();

    let target_parent = target_dir.path().join(".config/nested");
    std::fs::create_dir_all(&target_parent).unwrap();
    let target_path = target_parent.join("file.conf");
    std::os::unix::fs::symlink(&staged_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: staged_path.clone(),
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
    });
    state.save().unwrap();

    state.undeploy().unwrap();
    assert!(!staged_path.exists());
    assert!(
        !staged_parent.exists(),
        "empty staged parent should be cleaned up"
    );
}
