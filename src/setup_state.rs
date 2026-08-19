use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SETUP_STATE_FILE: &str = "setup-state.json";
// NOTE: no version migration exists yet (v1 only). If CURRENT_VERSION is
// ever bumped, add upgrade logic here, mirroring DeployState::load's
// version check (state.rs).
const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SetupState {
    #[serde(default)]
    version: u32,
    #[serde(skip)]
    state_dir: PathBuf,
    #[serde(default)]
    entries: HashMap<String, SetupEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupEntry {
    pub last_run: String,
    pub script_hash: String,
    pub status: SetupStatus,
    pub exit_code: i32,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupStatus {
    Success,
    Failed,
}

impl SetupState {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            version: CURRENT_VERSION,
            state_dir: state_dir.to_path_buf(),
            entries: HashMap::new(),
        }
    }

    pub fn load(state_dir: &Path) -> Result<Self> {
        let path = state_dir.join(SETUP_STATE_FILE);
        if !path.exists() {
            return Ok(Self::new(state_dir));
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read setup state: {}", path.display()))?;
        let mut state: Self = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse setup state: {}", path.display()))?;
        state.state_dir = state_dir.to_path_buf();
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir).with_context(|| {
            format!(
                "failed to create state directory: {}",
                self.state_dir.display()
            )
        })?;
        let path = self.state_dir.join(SETUP_STATE_FILE);
        let tmp_path = self.state_dir.join(".setup-state.json.tmp");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp_path, &content)
            .with_context(|| format!("failed to write temp state file: {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &path).with_context(|| {
            format!(
                "failed to rename temp state file: {} -> {}",
                tmp_path.display(),
                path.display()
            )
        })?;
        Ok(())
    }

    pub fn get(&self, package: &str) -> Option<&SetupEntry> {
        self.entries.get(package)
    }

    pub fn update(&mut self, package: String, entry: SetupEntry) {
        self.entries.insert(package, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_entry() -> SetupEntry {
        SetupEntry {
            last_run: "2026-03-31T12:00:00+00:00".to_string(),
            script_hash: "abc123".to_string(),
            status: SetupStatus::Success,
            exit_code: 0,
            duration_ms: 100,
            error: None,
            output: None,
        }
    }

    #[test]
    fn new_state_is_empty() {
        let dir = TempDir::new().unwrap();
        let state = SetupState::new(dir.path());
        assert!(state.get("anything").is_none());
    }

    #[test]
    fn load_missing_file_returns_empty_state() {
        let dir = TempDir::new().unwrap();
        let state = SetupState::load(dir.path()).unwrap();
        assert!(state.get("anything").is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut state = SetupState::new(dir.path());
        state.update("test-pkg".to_string(), sample_entry());
        state.save().unwrap();

        let loaded = SetupState::load(dir.path()).unwrap();
        let entry = loaded.get("test-pkg").unwrap();
        assert_eq!(entry.status, SetupStatus::Success);
        assert_eq!(entry.script_hash, "abc123");
    }

    #[test]
    fn update_overwrites_existing_entry() {
        let dir = TempDir::new().unwrap();
        let mut state = SetupState::new(dir.path());
        state.update("pkg".to_string(), sample_entry());

        let mut second = sample_entry();
        second.status = SetupStatus::Failed;
        second.error = Some("boom".to_string());
        state.update("pkg".to_string(), second);

        let entry = state.get("pkg").unwrap();
        assert_eq!(entry.status, SetupStatus::Failed);
        assert_eq!(entry.error.as_deref(), Some("boom"));
    }
}
