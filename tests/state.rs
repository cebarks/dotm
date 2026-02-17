use dotm::state::DeployState;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn save_and_load_state() {
    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());

    state.record_symlink(
        PathBuf::from("/home/user/.bashrc"),
        PathBuf::from("/dotfiles/packages/shell/.bashrc"),
    );
    state.record_copy(PathBuf::from("/home/user/.config/app.conf"));

    state.save().unwrap();

    let loaded = DeployState::load(state_dir.path()).unwrap();
    assert_eq!(loaded.symlinks().len(), 1);
    assert_eq!(loaded.copies().len(), 1);
}

#[test]
fn load_nonexistent_returns_empty() {
    let state_dir = TempDir::new().unwrap();
    let loaded = DeployState::load(state_dir.path()).unwrap();
    assert!(loaded.symlinks().is_empty());
    assert!(loaded.copies().is_empty());
}
