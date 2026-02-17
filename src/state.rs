use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const STATE_FILE: &str = "dotm-state.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeployState {
    #[serde(skip)]
    state_dir: PathBuf,
    symlinks: Vec<SymlinkEntry>,
    copies: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SymlinkEntry {
    pub target: PathBuf,
    pub source: PathBuf,
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

    pub fn record_symlink(&mut self, target: PathBuf, source: PathBuf) {
        self.symlinks.push(SymlinkEntry { target, source });
    }

    pub fn record_copy(&mut self, target: PathBuf) {
        self.copies.push(target);
    }

    pub fn symlinks(&self) -> &[SymlinkEntry] {
        &self.symlinks
    }

    pub fn copies(&self) -> &[PathBuf] {
        &self.copies
    }

    /// Remove all managed files and return a count of removed files.
    pub fn undeploy(&self) -> Result<usize> {
        let mut removed = 0;

        for entry in &self.symlinks {
            if entry.target.is_symlink() {
                std::fs::remove_file(&entry.target)
                    .with_context(|| format!("failed to remove symlink: {}", entry.target.display()))?;
                removed += 1;
            }
        }

        for path in &self.copies {
            if path.exists() {
                std::fs::remove_file(path)
                    .with_context(|| format!("failed to remove copy: {}", path.display()))?;
                removed += 1;
            }
        }

        // Remove the state file itself
        let state_path = self.state_dir.join(STATE_FILE);
        if state_path.exists() {
            std::fs::remove_file(&state_path)?;
        }

        Ok(removed)
    }
}
