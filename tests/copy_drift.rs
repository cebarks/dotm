use dotm::hash;
use dotm::scanner::EntryKind;
use dotm::state::{DeployEntry, DeployState};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn copy_strategy_status_detects_drift() {
    let state_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let target_path = target_dir.path().join("config.conf");
    std::fs::write(&target_path, "original content").unwrap();
    let original_hash = hash::hash_file(&target_path).unwrap();

    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: target_path.clone(),
        source: PathBuf::from("/source/config.conf"),
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
    });

    // File not modified yet
    let status = state.check_entry_status(&state.entries()[0]);
    assert!(status.is_ok());

    // Modify the target file
    std::fs::write(&target_path, "modified by user").unwrap();

    let status = state.check_entry_status(&state.entries()[0]);
    assert!(status.is_modified());
}
