use crate::hash;
use crate::scanner::EntryKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum FileStatus {
    Ok,
    Modified,
    Missing,
}

const STATE_FILE: &str = "dotm-state.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeployState {
    #[serde(skip)]
    state_dir: PathBuf,
    entries: Vec<DeployEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeployEntry {
    pub target: PathBuf,
    pub staged: PathBuf,
    pub source: PathBuf,
    pub content_hash: String,
    pub kind: EntryKind,
    pub package: String,
}

impl DeployState {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            state_dir: state_dir.to_path_buf(),
            ..Default::default()
        }
    }

    pub fn load(state_dir: &Path) -> Result<Self> {
        let path = state_dir.join(STATE_FILE);
        if !path.exists() {
            return Ok(Self::new(state_dir));
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read state file: {}", path.display()))?;
        let mut state: DeployState = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse state file: {}", path.display()))?;
        state.state_dir = state_dir.to_path_buf();
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir)
            .with_context(|| format!("failed to create state directory: {}", self.state_dir.display()))?;
        let path = self.state_dir.join(STATE_FILE);
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write state file: {}", path.display()))?;
        Ok(())
    }

    pub fn record(&mut self, entry: DeployEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[DeployEntry] {
        &self.entries
    }

    pub fn check_entry_status(&self, entry: &DeployEntry) -> FileStatus {
        if !entry.target.exists() && !entry.target.is_symlink() {
            return FileStatus::Missing;
        }
        if entry.staged.exists() {
            if let Ok(current_hash) = hash::hash_file(&entry.staged)
                && current_hash != entry.content_hash {
                    return FileStatus::Modified;
                }
        } else {
            return FileStatus::Missing;
        }
        FileStatus::Ok
    }

    pub fn originals_dir(&self) -> PathBuf {
        self.state_dir.join("originals")
    }

    pub fn store_original(&self, content_hash: &str, content: &[u8]) -> Result<()> {
        let dir = self.originals_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create originals directory: {}", dir.display()))?;
        let path = dir.join(content_hash);
        if !path.exists() {
            std::fs::write(&path, content)
                .with_context(|| format!("failed to store original: {}", path.display()))?;
        }
        Ok(())
    }

    pub fn load_original(&self, content_hash: &str) -> Result<Vec<u8>> {
        let path = self.originals_dir().join(content_hash);
        std::fs::read(&path)
            .with_context(|| format!("failed to load original content: {}", path.display()))
    }

    /// Remove all managed files and return a count of removed files.
    pub fn undeploy(&self) -> Result<usize> {
        let mut removed = 0;

        for entry in &self.entries {
            if entry.target.is_symlink() || entry.target.exists() {
                std::fs::remove_file(&entry.target)
                    .with_context(|| format!("failed to remove target: {}", entry.target.display()))?;
                cleanup_empty_parents(&entry.target);
                removed += 1;
            }

            if entry.staged.exists() {
                std::fs::remove_file(&entry.staged)
                    .with_context(|| format!("failed to remove staged file: {}", entry.staged.display()))?;
                cleanup_empty_parents(&entry.staged);
            }
        }

        // Clean up originals directory
        let originals = self.originals_dir();
        if originals.is_dir() {
            let _ = std::fs::remove_dir_all(&originals);
        }

        // Remove the state file itself
        let state_path = self.state_dir.join(STATE_FILE);
        if state_path.exists() {
            std::fs::remove_file(&state_path)?;
        }

        Ok(removed)
    }
}

fn cleanup_empty_parents(path: &Path) {
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent == Path::new("") || parent == Path::new("/") {
            break;
        }
        match std::fs::read_dir(parent) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    let _ = std::fs::remove_dir(parent);
                    current = parent.parent();
                } else {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
