use crate::hash;
use crate::scanner::EntryKind;
use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FileStatus {
    pub exists: bool,
    pub content_modified: bool,
    pub owner_changed: bool,
    pub group_changed: bool,
    pub mode_changed: bool,
}

impl FileStatus {
    pub fn ok() -> Self {
        Self {
            exists: true,
            content_modified: false,
            owner_changed: false,
            group_changed: false,
            mode_changed: false,
        }
    }

    pub fn missing() -> Self {
        Self {
            exists: false,
            content_modified: false,
            owner_changed: false,
            group_changed: false,
            mode_changed: false,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.exists
            && !self.content_modified
            && !self.owner_changed
            && !self.group_changed
            && !self.mode_changed
    }

    pub fn is_missing(&self) -> bool {
        !self.exists
    }

    pub fn is_modified(&self) -> bool {
        self.content_modified
    }

    pub fn has_metadata_drift(&self) -> bool {
        self.owner_changed || self.group_changed || self.mode_changed
    }
}

const STATE_FILE: &str = "dotm-state.json";
const CURRENT_VERSION: u32 = 3;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DeployState {
    #[serde(default)]
    version: u32,
    #[serde(skip)]
    state_dir: PathBuf,
    #[serde(skip)]
    lock: Option<std::fs::File>,
    entries: Vec<DeployEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployEntry {
    pub target: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<PathBuf>,
    pub source: PathBuf,
    pub content_hash: String,
    #[serde(default)]
    pub original_hash: Option<String>,
    pub kind: EntryKind,
    pub package: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub original_owner: Option<String>,
    #[serde(default)]
    pub original_group: Option<String>,
    #[serde(default)]
    pub original_mode: Option<String>,
}

impl DeployState {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            version: CURRENT_VERSION,
            state_dir: state_dir.to_path_buf(),
            ..Default::default()
        }
    }

    /// Acquire an exclusive file lock without loading existing state.
    pub fn lock(&mut self) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir).with_context(|| {
            format!(
                "failed to create state directory: {}",
                self.state_dir.display()
            )
        })?;
        let lock_path = self.state_dir.join("dotm.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file: {}", lock_path.display()))?;

        lock_file.try_lock_exclusive().map_err(|_| {
            anyhow::anyhow!(
                "another dotm process is running (could not acquire lock on {})",
                lock_path.display()
            )
        })?;

        self.lock = Some(lock_file);
        Ok(())
    }

    pub fn load(state_dir: &Path) -> Result<Self> {
        // Only run v1->v2 storage migration for legacy state dirs (not ~/.dotm/)
        let is_legacy = state_dir.file_name().map(|n| n != ".dotm").unwrap_or(true);
        if is_legacy {
            Self::migrate_storage(state_dir)?;
        }
        let path = state_dir.join(STATE_FILE);
        if !path.exists() {
            return Ok(Self::new(state_dir));
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read state file: {}", path.display()))?;
        let mut state: DeployState = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse state file: {}", path.display()))?;
        if state.version > CURRENT_VERSION {
            anyhow::bail!(
                "state file was created by a newer version of dotm (state version {}, max supported {})",
                state.version,
                CURRENT_VERSION
            );
        }
        if state.version < CURRENT_VERSION {
            state.version = CURRENT_VERSION;
        }
        state.state_dir = state_dir.to_path_buf();
        Ok(state)
    }

    /// Load state with an exclusive file lock to prevent concurrent access.
    /// The lock is held until the DeployState is dropped.
    pub fn load_locked(state_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(state_dir).with_context(|| {
            format!("failed to create state directory: {}", state_dir.display())
        })?;
        let lock_path = state_dir.join("dotm.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file: {}", lock_path.display()))?;

        lock_file.try_lock_exclusive().map_err(|_| {
            anyhow::anyhow!(
                "another dotm process is running (could not acquire lock on {})",
                lock_path.display()
            )
        })?;

        let mut state = Self::load(state_dir)?;
        state.lock = Some(lock_file);
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir).with_context(|| {
            format!(
                "failed to create state directory: {}",
                self.state_dir.display()
            )
        })?;
        let path = self.state_dir.join(STATE_FILE);
        let tmp_path = self.state_dir.join(".dotm-state.json.tmp");
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

    pub fn record(&mut self, entry: DeployEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[DeployEntry] {
        &self.entries
    }

    pub fn remove_targets(&mut self, targets: &[PathBuf]) {
        self.entries.retain(|e| !targets.contains(&e.target));
    }

    pub fn entries_mut(&mut self) -> &mut [DeployEntry] {
        &mut self.entries
    }

    pub fn update_entry_hash(&mut self, index: usize, new_hash: String) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.content_hash = new_hash;
        }
    }

    pub fn check_entry_status(&self, entry: &DeployEntry) -> FileStatus {
        if !entry.target.exists() && !entry.target.is_symlink() {
            return FileStatus::missing();
        }

        let mut status = FileStatus::ok();

        if entry.target.is_symlink() {
            // Symlink path: check that it points to the expected source
            let link_dest = match std::fs::read_link(&entry.target) {
                Ok(dest) => dest,
                Err(_) => return FileStatus::missing(),
            };

            let canon_link = std::fs::canonicalize(&link_dest)
                .or_else(|_| std::fs::canonicalize(&entry.target))
                .unwrap_or(link_dest.clone());
            let canon_source =
                std::fs::canonicalize(&entry.source).unwrap_or_else(|_| entry.source.clone());

            if canon_link == canon_source {
                // Direct symlink to source — no content drift possible
            } else if let Some(ref staged) = entry.staged {
                // Transitional v2 compatibility: symlink points to staged path
                let canon_staged = std::fs::canonicalize(staged).unwrap_or_else(|_| staged.clone());
                if canon_link == canon_staged {
                    // v2 staged symlink — check hash for drift
                    if let Ok(current_hash) = hash::hash_file(staged)
                        && current_hash != entry.content_hash
                    {
                        status.content_modified = true;
                    }
                } else {
                    return FileStatus::missing();
                }
            } else {
                return FileStatus::missing();
            }
        } else {
            // Regular file (copy/template): hash the target
            if let Ok(current_hash) = hash::hash_file(&entry.target)
                && current_hash != entry.content_hash
            {
                status.content_modified = true;
            }
        }

        // Metadata checks (only if we recorded what we set)
        if let Ok((current_owner, current_group, current_mode)) =
            crate::metadata::read_file_metadata(&entry.target)
        {
            if let Some(ref expected_owner) = entry.owner {
                if current_owner != *expected_owner {
                    status.owner_changed = true;
                }
            }
            if let Some(ref expected_group) = entry.group {
                if current_group != *expected_group {
                    status.group_changed = true;
                }
            }
            if let Some(ref expected_mode) = entry.mode {
                if current_mode != *expected_mode {
                    status.mode_changed = true;
                }
            }
        }

        status
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

    pub fn migrate_storage(state_dir: &Path) -> Result<()> {
        let originals = state_dir.join("originals");
        let deployed = state_dir.join("deployed");
        if originals.is_dir() && !deployed.exists() {
            std::fs::rename(&originals, &deployed)
                .with_context(|| "failed to migrate originals/ to deployed/")?;
        }
        Ok(())
    }

    /// Restore files to their pre-dotm state.
    /// Files with original_hash get their original content written back with original metadata.
    /// Files without original_hash (dotm created them) get removed.
    /// Returns the count of restored files.
    pub fn restore(&mut self, package_filter: Option<&str>) -> Result<usize> {
        let mut restored = 0;
        let mut remaining = Vec::new();
        let mut restore_error: Option<anyhow::Error> = None;

        for entry in &self.entries {
            if let Some(filter) = package_filter {
                if entry.package != filter {
                    remaining.push(entry.clone());
                    continue;
                }
            }

            // On error, keep unprocessed entries in remaining so state stays consistent
            if restore_error.is_some() {
                remaining.push(entry.clone());
                continue;
            }

            let result: Result<()> = (|| {
                if let Some(ref orig_hash) = entry.original_hash {
                    let original_content = self.load_original(orig_hash)?;
                    std::fs::write(&entry.target, &original_content).with_context(|| {
                        format!("failed to restore: {}", entry.target.display())
                    })?;

                    if entry.original_owner.is_some() || entry.original_group.is_some() {
                        let _ = crate::metadata::apply_ownership(
                            &entry.target,
                            entry.original_owner.as_deref(),
                            entry.original_group.as_deref(),
                        );
                    }
                    if let Some(ref orig_mode) = entry.original_mode {
                        let _ =
                            crate::deployer::apply_permission_override(&entry.target, orig_mode);
                    }
                } else if entry.target.exists() || entry.target.is_symlink() {
                    std::fs::remove_file(&entry.target)
                        .with_context(|| format!("failed to remove: {}", entry.target.display()))?;
                    cleanup_empty_parents(&entry.target);
                }
                Ok(())
            })();

            match result {
                Ok(()) => restored += 1,
                Err(e) => {
                    remaining.push(entry.clone());
                    restore_error = Some(e);
                }
            }
        }

        if package_filter.is_some() {
            self.entries = remaining;
            if let Err(save_err) = self.save() {
                eprintln!("warning: failed to save state after partial restore: {save_err}");
            }
        } else if restore_error.is_none() {
            let originals = self.originals_dir();
            if originals.is_dir() {
                let _ = std::fs::remove_dir_all(&originals);
            }
            let state_path = self.state_dir.join(STATE_FILE);
            if state_path.exists() {
                std::fs::remove_file(&state_path)?;
            }
        }

        if let Some(e) = restore_error {
            return Err(e);
        }

        Ok(restored)
    }

    /// Remove managed files for a single package and save updated state.
    pub fn undeploy_package(&mut self, package: &str) -> Result<usize> {
        let mut removed = 0;
        let mut remaining = Vec::new();

        for entry in &self.entries {
            if entry.package == package {
                if entry.target.is_symlink() || entry.target.exists() {
                    std::fs::remove_file(&entry.target).with_context(|| {
                        format!("failed to remove target: {}", entry.target.display())
                    })?;
                    cleanup_empty_parents(&entry.target);
                    removed += 1;
                }
            } else {
                remaining.push(entry.clone());
            }
        }

        self.entries = remaining;
        self.save()?;

        Ok(removed)
    }

    /// Remove all managed files and return a count of removed files.
    pub fn undeploy(&self) -> Result<usize> {
        let mut removed = 0;

        for entry in &self.entries {
            if entry.target.is_symlink() || entry.target.exists() {
                std::fs::remove_file(&entry.target).with_context(|| {
                    format!("failed to remove target: {}", entry.target.display())
                })?;
                cleanup_empty_parents(&entry.target);
                removed += 1;
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

pub fn cleanup_empty_parents(path: &Path) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn store_and_load_original_content() {
        let dir = TempDir::new().unwrap();
        let state = DeployState::new(dir.path());
        state
            .store_original("orig456", b"original pre-existing content")
            .unwrap();
        let loaded = state.load_original("orig456").unwrap();
        assert_eq!(loaded, b"original pre-existing content");
    }

    #[test]
    fn migrate_renames_originals_to_deployed() {
        let dir = TempDir::new().unwrap();
        let originals = dir.path().join("originals");
        std::fs::create_dir_all(&originals).unwrap();
        std::fs::write(originals.join("hash1"), "content1").unwrap();

        DeployState::migrate_storage(dir.path()).unwrap();

        assert!(!originals.exists());
        let deployed = dir.path().join("deployed");
        assert!(deployed.exists());
        assert_eq!(
            std::fs::read_to_string(deployed.join("hash1")).unwrap(),
            "content1"
        );
    }

    #[test]
    fn migrate_noop_if_deployed_exists() {
        let dir = TempDir::new().unwrap();
        let deployed = dir.path().join("deployed");
        std::fs::create_dir_all(&deployed).unwrap();
        std::fs::write(deployed.join("hash1"), "existing").unwrap();

        let originals = dir.path().join("originals");
        std::fs::create_dir_all(&originals).unwrap();
        std::fs::write(originals.join("hash1"), "should not replace").unwrap();

        DeployState::migrate_storage(dir.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(deployed.join("hash1")).unwrap(),
            "existing"
        );
    }

    #[test]
    fn concurrent_lock_fails() {
        use fs2::FileExt;
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        let lock_path = dir.path().join("dotm.lock");
        std::fs::write(&lock_path, "").unwrap();

        // Hold the lock
        let f = std::fs::File::open(&lock_path).unwrap();
        f.lock_exclusive().unwrap();

        // Try to acquire from DeployState — should fail
        let result = DeployState::load_locked(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("another dotm process")
        );
    }
}
