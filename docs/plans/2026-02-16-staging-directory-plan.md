# Staging Directory Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace direct copy/symlink deployment with a staging directory that all deployed files flow through, enabling drift detection and interactive adoption of app-made changes.

**Architecture:** All files (base, override, template) are copied/rendered into `<dotfiles_dir>/.staged/`, then symlinked from the target directory. State tracks content hashes for drift detection. New `diff` and `adopt` commands enable reviewing and accepting external changes. Per-package `strategy` config allows falling back to direct copy for packages where symlinks are problematic.

**Tech Stack:** Rust, `sha2` crate (SHA-256 hashing), `similar` crate (diff/patch), `crossterm` (interactive terminal UI for adopt)

**Design doc:** `docs/plans/2026-02-16-staging-directory-design.md`

---

### Task 1: Add new dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add sha2, similar, and crossterm to Cargo.toml**

Add to the `[dependencies]` section:

```toml
sha2 = "0.10"
similar = "2"
crossterm = "0.28"
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

**Step 3: Commit**

```
feat: add sha2, similar, and crossterm dependencies
```

---

### Task 2: Add `strategy` and `permissions` to PackageConfig

**Files:**
- Modify: `src/config.rs`
- Test: `tests/config_parsing.rs`

**Step 1: Write the failing test**

Add to `tests/config_parsing.rs`:

```rust
#[test]
fn parse_package_with_strategy_and_permissions() {
    let toml_str = r#"
[dotm]
target = "~"

[packages.system]
description = "System configs"
target = "/"
strategy = "copy"

[packages.bin]
description = "Scripts"

[packages.bin.permissions]
"bin/myscript" = "755"
"bin/helper" = "700"
"#;

    let config: dotm::config::RootConfig = toml::from_str(toml_str).unwrap();

    let system = &config.packages["system"];
    assert_eq!(system.strategy, dotm::config::DeployStrategy::Copy);

    let bin = &config.packages["bin"];
    assert_eq!(bin.strategy, dotm::config::DeployStrategy::Stage);
    let perms = &bin.permissions;
    assert_eq!(perms.get("bin/myscript").unwrap(), "755");
    assert_eq!(perms.get("bin/helper").unwrap(), "700");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test config_parsing parse_package_with_strategy_and_permissions`
Expected: FAIL — `DeployStrategy` doesn't exist yet

**Step 3: Implement the config changes**

In `src/config.rs`, add a `DeployStrategy` enum and update `PackageConfig`:

```rust
use std::collections::HashMap;

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeployStrategy {
    #[default]
    Stage,
    Copy,
}

#[derive(Debug, Deserialize)]
pub struct PackageConfig {
    pub description: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub suggests: Vec<String>,
    pub target: Option<String>,
    #[serde(default)]
    pub strategy: DeployStrategy,
    #[serde(default)]
    pub permissions: HashMap<String, String>,
}
```

Note: `HashMap` is already imported at the top of the file for `packages: HashMap<String, PackageConfig>`.

**Step 4: Run test to verify it passes**

Run: `cargo test --test config_parsing parse_package_with_strategy_and_permissions`
Expected: PASS

**Step 5: Run all existing tests to verify nothing broke**

Run: `cargo test`
Expected: all tests pass (existing configs don't set `strategy` or `permissions`, so defaults apply)

**Step 6: Commit**

```
feat: add strategy and permissions fields to PackageConfig
```

---

### Task 3: Add `EntryKind` to scanner's `FileAction`

The scanner currently uses `is_copy: bool` and `is_template: bool` to describe how a file was resolved. Replace these with a single `EntryKind` enum that the new state tracking needs.

**Files:**
- Modify: `src/scanner.rs`
- Modify: `src/deployer.rs` (update match on new enum)
- Modify: `src/orchestrator.rs` (update match on new enum)
- Test: `tests/scanner.rs`, `tests/deployer.rs`

**Step 1: Write a failing test for the new enum**

Add to `tests/scanner.rs`:

```rust
#[test]
fn file_action_kind_reflects_resolution() {
    use dotm::scanner::EntryKind;

    let pkg_dir = Path::new("tests/fixtures/overrides/packages/configs");
    let actions = scan_package(pkg_dir, "myhost", &["desktop"]).unwrap();

    let app_conf = actions.iter().find(|a| a.target_rel_path == Path::new(".config/app.conf")).unwrap();
    assert_eq!(app_conf.kind, EntryKind::Override);

    let theme = actions.iter().find(|a| a.target_rel_path == Path::new(".config/theme.conf")).unwrap();
    assert_eq!(theme.kind, EntryKind::Base);

    let templated = actions.iter().find(|a| a.target_rel_path == Path::new(".config/templated.conf")).unwrap();
    assert_eq!(templated.kind, EntryKind::Template);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test scanner file_action_kind`
Expected: FAIL — `EntryKind` doesn't exist

**Step 3: Implement EntryKind**

In `src/scanner.rs`, add the enum and replace the two bools:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EntryKind {
    Base,
    Override,
    Template,
}

#[derive(Debug)]
pub struct FileAction {
    pub source: PathBuf,
    pub target_rel_path: PathBuf,
    pub kind: EntryKind,
}
```

Update `resolve_variant` to return the appropriate `EntryKind`:
- Host/role overrides: `EntryKind::Override`
- Templates: `EntryKind::Template`
- Base files: `EntryKind::Base`

**Step 4: Update deployer.rs to use EntryKind**

In `src/deployer.rs`, replace:

```rust
if action.is_template {
```

with pattern matching on `action.kind`:

```rust
match action.kind {
    EntryKind::Template => {
        let content = rendered_content.unwrap_or("");
        std::fs::write(&target_path, content)
            .with_context(|| format!("failed to write template output: {}", target_path.display()))?;
    }
    _ => {
        // Both Base and Override are copied/symlinked the same way at this layer.
        // The staging vs direct-copy decision happens in the orchestrator.
        std::fs::copy(&action.source, &target_path)
            .with_context(|| format!("failed to copy {} to {}", action.source.display(), target_path.display()))?;
    }
}
```

Note: The deployer's role changes significantly in Task 5. For now, make it compile and pass existing tests. The old `is_copy` logic that decided symlink-vs-copy moves to the orchestrator in Task 5.

**Step 5: Update orchestrator.rs**

Replace references to `action.is_copy` and `action.is_template` with `action.kind` matches. The orchestrator currently checks `action.is_template` to decide whether to render, and `action.is_copy || action.is_template` to decide state recording. Update both:

```rust
let rendered = if action.kind == scanner::EntryKind::Template {
    // ... render template
} else {
    None
};
```

And for state recording:

```rust
match action.kind {
    scanner::EntryKind::Base => {
        let abs_source = std::fs::canonicalize(&action.source)
            .unwrap_or_else(|_| action.source.clone());
        state.record_symlink(target_path.clone(), abs_source);
    }
    _ => {
        state.record_copy(target_path.clone());
    }
}
```

**Step 6: Update existing tests in `tests/deployer.rs`**

Replace `is_copy: false, is_template: false` with `kind: dotm::scanner::EntryKind::Base`, etc. in all existing deployer tests. The tests that check `is_copy: true, is_template: false` become `kind: EntryKind::Override`. The test with `is_copy: true, is_template: true` becomes `kind: EntryKind::Template`.

**Step 7: Run all tests**

Run: `cargo test`
Expected: all tests pass

**Step 8: Commit**

```
refactor: replace is_copy/is_template bools with EntryKind enum
```

---

### Task 4: Rewrite state tracking with DeployEntry

**Files:**
- Modify: `src/state.rs`
- Test: `tests/state.rs`

**Step 1: Write failing tests for new state structure**

Replace the contents of `tests/state.rs` with:

```rust
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
        kind: EntryKind::Base,
        package: "shell".to_string(),
    });
    state.record(DeployEntry {
        target: PathBuf::from("/home/user/.config/app.conf"),
        staged: PathBuf::from("/home/user/dotfiles/.staged/.config/app.conf"),
        source: PathBuf::from("/home/user/dotfiles/packages/configs/.config/app.conf##host.myhost"),
        content_hash: "def456".to_string(),
        kind: EntryKind::Override,
        package: "configs".to_string(),
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

    // Create staged file
    let staged_path = staged_dir.path().join(".bashrc");
    std::fs::write(&staged_path, "content").unwrap();

    // Create target symlink pointing to staged file
    let target_path = target_dir.path().join(".bashrc");
    std::os::unix::fs::symlink(&staged_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: staged_path.clone(),
        source: PathBuf::from("irrelevant"),
        content_hash: "hash".to_string(),
        kind: EntryKind::Base,
        package: "shell".to_string(),
    });
    state.save().unwrap();

    let removed = state.undeploy().unwrap();
    assert_eq!(removed, 1);
    assert!(!target_path.exists());
    assert!(!staged_path.exists());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test state`
Expected: FAIL — `DeployEntry`, `record()`, `entries()` don't exist

**Step 3: Rewrite src/state.rs**

```rust
use crate::scanner::EntryKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

    pub fn undeploy(&self) -> Result<usize> {
        let mut removed = 0;

        for entry in &self.entries {
            // Remove target symlink
            if entry.target.is_symlink() {
                std::fs::remove_file(&entry.target)
                    .with_context(|| format!("failed to remove symlink: {}", entry.target.display()))?;
            }
            // Remove staged file
            if entry.staged.exists() {
                std::fs::remove_file(&entry.staged)
                    .with_context(|| format!("failed to remove staged file: {}", entry.staged.display()))?;
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
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test state`
Expected: PASS

**Step 5: Commit**

```
refactor: rewrite DeployState with DeployEntry and content hashing
```

---

### Task 5: Add content hashing utility

**Files:**
- Create: `src/hash.rs`
- Modify: `src/lib.rs` (add `pub mod hash;`)

**Step 1: Write failing test**

Create a unit test in `src/hash.rs` (inline `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn hash_file_returns_consistent_sha256() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello world").unwrap();

        let hash1 = hash_file(&path).unwrap();
        let hash2 = hash_file(&path).unwrap();
        assert_eq!(hash1, hash2);
        // SHA-256 of "hello world"
        assert_eq!(hash1, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn hash_content_matches_hash_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        let content = "some content";
        std::fs::write(&path, content).unwrap();

        assert_eq!(hash_file(&path).unwrap(), hash_content(content.as_bytes()));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib hash`
Expected: FAIL — module doesn't exist

**Step 3: Implement hash.rs**

```rust
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn hash_file(path: &Path) -> Result<String> {
    let content = std::fs::read(path)
        .with_context(|| format!("failed to read file for hashing: {}", path.display()))?;
    Ok(hash_content(&content))
}

pub fn hash_content(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}
```

Add `pub mod hash;` to `src/lib.rs`.

**Step 4: Run tests**

Run: `cargo test --lib hash`
Expected: PASS

**Step 5: Commit**

```
feat: add SHA-256 content hashing utility
```

---

### Task 6: Rewrite deployer for staging

The deployer currently handles symlink-vs-copy logic. Replace it with two deployment modes: `stage_file` (copy/render to staging dir, symlink from target) and `copy_file` (direct copy to target, for strategy=copy packages).

**Files:**
- Modify: `src/deployer.rs`
- Test: `tests/deployer.rs`

**Step 1: Write failing tests for staged deployment**

Replace `tests/deployer.rs` with:

```rust
use dotm::deployer::{deploy_staged, deploy_copy, DeployResult};
use dotm::scanner::{EntryKind, FileAction};
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn stage_base_file_copies_to_staging_and_symlinks_target() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.profile");

    let action = FileAction {
        source: source.clone(),
        target_rel_path: PathBuf::from(".profile"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    // Staged file should be a real file
    let staged = staging_dir.path().join(".profile");
    assert!(staged.exists());
    assert!(!staged.is_symlink());

    // Target should be a symlink to the staged file
    let target = target_dir.path().join(".profile");
    assert!(target.is_symlink());
    assert_eq!(std::fs::read_link(&target).unwrap(), std::fs::canonicalize(&staged).unwrap());
}

#[test]
fn stage_template_renders_to_staging_and_symlinks_target() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.config/templated.conf.tera");

    let action = FileAction {
        source,
        target_rel_path: PathBuf::from(".config/templated.conf"),
        kind: EntryKind::Template,
    };

    let rendered = "color=blue";
    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, Some(rendered)).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let staged = staging_dir.path().join(".config/templated.conf");
    assert_eq!(std::fs::read_to_string(&staged).unwrap(), "color=blue");

    let target = target_dir.path().join(".config/templated.conf");
    assert!(target.is_symlink());
}

#[test]
fn stage_preserves_source_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    // Create a source file with executable permission
    let src_dir = TempDir::new().unwrap();
    let source = src_dir.path().join("script.sh");
    std::fs::write(&source, "#!/bin/bash\necho hi").unwrap();
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();

    let action = FileAction {
        source,
        target_rel_path: PathBuf::from("script.sh"),
        kind: EntryKind::Base,
    };

    deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, None).unwrap();

    let staged = staging_dir.path().join("script.sh");
    let mode = staged.metadata().unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
}

#[test]
fn copy_strategy_copies_directly_to_target() {
    let target_dir = TempDir::new().unwrap();
    let source = PathBuf::from("tests/fixtures/overrides/packages/configs/.profile");

    let action = FileAction {
        source: source.clone(),
        target_rel_path: PathBuf::from(".profile"),
        kind: EntryKind::Base,
    };

    let result = deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let target = target_dir.path().join(".profile");
    assert!(target.exists());
    assert!(!target.is_symlink());
}

#[test]
fn stage_detects_conflict_with_unmanaged_file() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    // Create an unmanaged file at the target
    std::fs::write(target_dir.path().join(".profile"), "unmanaged").unwrap();

    let action = FileAction {
        source: PathBuf::from("tests/fixtures/overrides/packages/configs/.profile"),
        target_rel_path: PathBuf::from(".profile"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));
}

#[test]
fn stage_force_overwrites_unmanaged_file() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    std::fs::write(target_dir.path().join(".profile"), "unmanaged").unwrap();

    let action = FileAction {
        source: PathBuf::from("tests/fixtures/overrides/packages/configs/.profile"),
        target_rel_path: PathBuf::from(".profile"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), false, true, None).unwrap();
    assert!(matches!(result, DeployResult::Created));
    assert!(target_dir.path().join(".profile").is_symlink());
}

#[test]
fn stage_dry_run_creates_nothing() {
    let staging_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let action = FileAction {
        source: PathBuf::from("tests/fixtures/overrides/packages/configs/.profile"),
        target_rel_path: PathBuf::from(".profile"),
        kind: EntryKind::Base,
    };

    let result = deploy_staged(&action, staging_dir.path(), target_dir.path(), true, false, None).unwrap();
    assert!(matches!(result, DeployResult::DryRun));
    assert!(!staging_dir.path().join(".profile").exists());
    assert!(!target_dir.path().join(".profile").exists());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test deployer`
Expected: FAIL — `deploy_staged` and `deploy_copy` don't exist

**Step 3: Rewrite src/deployer.rs**

```rust
use crate::scanner::{EntryKind, FileAction};
use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Debug)]
pub enum DeployResult {
    Created,
    Updated,
    Unchanged,
    Conflict(String),
    DryRun,
}

/// Deploy a file via the staging directory.
/// Copies/renders to staging_dir, then creates a symlink from target_dir.
pub fn deploy_staged(
    action: &FileAction,
    staging_dir: &Path,
    target_dir: &Path,
    dry_run: bool,
    force: bool,
    rendered_content: Option<&str>,
) -> Result<DeployResult> {
    let staged_path = staging_dir.join(&action.target_rel_path);
    let target_path = target_dir.join(&action.target_rel_path);

    if dry_run {
        return Ok(DeployResult::DryRun);
    }

    // Check for target conflicts (non-symlink file already exists)
    if target_path.exists() || target_path.is_symlink() {
        if target_path.is_symlink() {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing symlink: {}", target_path.display()))?;
        } else if force {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing file: {}", target_path.display()))?;
        } else {
            return Ok(DeployResult::Conflict(format!(
                "file already exists and is not managed by dotm: {}",
                target_path.display()
            )));
        }
    }

    // Create parent directories for both staged and target
    if let Some(parent) = staged_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create staging directory: {}", parent.display()))?;
    }
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create target directory: {}", parent.display()))?;
    }

    // Stage the file
    if action.kind == EntryKind::Template {
        let content = rendered_content.unwrap_or("");
        std::fs::write(&staged_path, content)
            .with_context(|| format!("failed to write staged template: {}", staged_path.display()))?;
        // For templates, copy permissions from the .tera source
        copy_permissions(&action.source, &staged_path)?;
    } else {
        std::fs::copy(&action.source, &staged_path)
            .with_context(|| format!("failed to stage {} to {}", action.source.display(), staged_path.display()))?;
        // copy() preserves permissions on Linux, but be explicit
    }

    // Create symlink from target to staged file
    let abs_staged = std::fs::canonicalize(&staged_path)
        .with_context(|| format!("failed to resolve staged path: {}", staged_path.display()))?;
    std::os::unix::fs::symlink(&abs_staged, &target_path)
        .with_context(|| format!("failed to create symlink: {} -> {}", target_path.display(), abs_staged.display()))?;

    Ok(DeployResult::Created)
}

/// Deploy a file directly to the target (strategy = "copy").
pub fn deploy_copy(
    action: &FileAction,
    target_dir: &Path,
    dry_run: bool,
    force: bool,
    rendered_content: Option<&str>,
) -> Result<DeployResult> {
    let target_path = target_dir.join(&action.target_rel_path);

    if dry_run {
        return Ok(DeployResult::DryRun);
    }

    if target_path.exists() || target_path.is_symlink() {
        if target_path.is_symlink() {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing symlink: {}", target_path.display()))?;
        } else if force {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing file: {}", target_path.display()))?;
        } else {
            return Ok(DeployResult::Conflict(format!(
                "file already exists and is not managed by dotm: {}",
                target_path.display()
            )));
        }
    }

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    if action.kind == EntryKind::Template {
        let content = rendered_content.unwrap_or("");
        std::fs::write(&target_path, content)
            .with_context(|| format!("failed to write template: {}", target_path.display()))?;
        copy_permissions(&action.source, &target_path)?;
    } else {
        std::fs::copy(&action.source, &target_path)
            .with_context(|| format!("failed to copy {} to {}", action.source.display(), target_path.display()))?;
    }

    Ok(DeployResult::Created)
}

/// Apply a specific permission mode (octal string like "755") to a file.
pub fn apply_permission_override(path: &Path, mode_str: &str) -> Result<()> {
    let mode = u32::from_str_radix(mode_str, 8)
        .with_context(|| format!("invalid permission mode '{}' for {}", mode_str, path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}

fn copy_permissions(source: &Path, dest: &Path) -> Result<()> {
    if let Ok(metadata) = std::fs::metadata(source) {
        let perms = metadata.permissions();
        std::fs::set_permissions(dest, perms)
            .with_context(|| format!("failed to copy permissions to {}", dest.display()))?;
    }
    Ok(())
}
```

**Step 4: Run deployer tests**

Run: `cargo test --test deployer`
Expected: PASS

**Step 5: Commit**

```
feat: rewrite deployer with staging and direct-copy modes
```

---

### Task 7: Rewrite orchestrator for staging pipeline

The orchestrator needs to: compute the staging dir path, detect collisions, route files through staging vs direct copy based on package strategy, hash content, and track the new state entries.

**Files:**
- Modify: `src/orchestrator.rs`
- Test: `tests/orchestrator.rs`, `tests/e2e.rs`

**Step 1: Write failing tests**

Add to `tests/e2e.rs`:

```rust
#[test]
fn e2e_deploy_stages_all_files() {
    let target = TempDir::new().unwrap();
    let dotfiles = Path::new("tests/fixtures/basic");

    let state_dir = TempDir::new().unwrap();
    let mut orch = Orchestrator::new(dotfiles, target.path()).unwrap()
        .with_state_dir(state_dir.path());
    let report = orch.deploy("testhost", false, false).unwrap();

    assert!(report.conflicts.is_empty());

    // All target files should be symlinks
    let bashrc = target.path().join(".bashrc");
    assert!(bashrc.is_symlink(), ".bashrc should be a symlink");

    let init_lua = target.path().join(".config/nvim/init.lua");
    assert!(init_lua.is_symlink(), "init.lua should be a symlink");

    // Symlinks should point into .staged/
    let bashrc_target = std::fs::read_link(&bashrc).unwrap();
    assert!(
        bashrc_target.to_str().unwrap().contains(".staged"),
        "symlink should point into .staged/, got: {}",
        bashrc_target.display()
    );

    // State should have entries with content hashes
    let state = dotm::state::DeployState::load(state_dir.path()).unwrap();
    assert!(!state.entries().is_empty());
    assert!(!state.entries()[0].content_hash.is_empty());
}

#[test]
fn e2e_collision_detection() {
    // This test requires a fixture with two packages that deploy the same relative path.
    // Create it dynamically.
    let dotfiles_tmp = TempDir::new().unwrap();

    // dotm.toml with two packages
    std::fs::write(
        dotfiles_tmp.path().join("dotm.toml"),
        "[dotm]\ntarget = \"~\"\n\n[packages.pkg_a]\ndescription = \"A\"\n\n[packages.pkg_b]\ndescription = \"B\"\n",
    ).unwrap();

    // Both packages deploy .config/collision.conf
    let pkg_a = dotfiles_tmp.path().join("packages/pkg_a/.config");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::write(pkg_a.join("collision.conf"), "from a").unwrap();

    let pkg_b = dotfiles_tmp.path().join("packages/pkg_b/.config");
    std::fs::create_dir_all(&pkg_b).unwrap();
    std::fs::write(pkg_b.join("collision.conf"), "from b").unwrap();

    // Host and role that include both
    std::fs::create_dir_all(dotfiles_tmp.path().join("hosts")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("hosts/testhost.toml"),
        "hostname = \"testhost\"\nroles = [\"all\"]\n",
    ).unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("roles")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("roles/all.toml"),
        "packages = [\"pkg_a\", \"pkg_b\"]\n",
    ).unwrap();

    let target = TempDir::new().unwrap();
    let mut orch = Orchestrator::new(dotfiles_tmp.path(), target.path()).unwrap();
    let result = orch.deploy("testhost", false, false);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("collision"),
        "expected collision error, got: {}",
        err_msg
    );
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test e2e e2e_deploy_stages_all_files e2e_collision_detection`
Expected: FAIL

**Step 3: Rewrite the orchestrator's deploy method**

Key changes to `src/orchestrator.rs`:

1. Compute `staging_dir` as `self.loader.base_dir().join(".staged")`
2. After scanning all packages, collect `(target_rel_path, package_name)` tuples and check for duplicates
3. For each file action, check the package's `strategy` config:
   - `Stage`: call `deployer::deploy_staged()`, hash the staged content, record a `DeployEntry`
   - `Copy`: call `deployer::deploy_copy()`, record a `DeployEntry` (staged path = target path for copy strategy)
4. Apply permission overrides from config after staging
5. Check for drift before overwriting existing staged files

The orchestrator struct needs the staging dir:

```rust
pub struct Orchestrator {
    loader: ConfigLoader,
    target_dir: PathBuf,
    state_dir: Option<PathBuf>,
    staging_dir: PathBuf,
}
```

Set `staging_dir` in `new()`:

```rust
pub fn new(dotfiles_dir: &Path, target_dir: &Path) -> Result<Self> {
    let loader = ConfigLoader::new(dotfiles_dir)?;
    let staging_dir = dotfiles_dir.join(".staged");
    Ok(Self {
        loader,
        target_dir: target_dir.to_path_buf(),
        state_dir: None,
        staging_dir,
    })
}
```

In `deploy()`, after scanning all packages but before deploying any files, collect all staging paths and check for collisions:

```rust
// Collect all actions across packages first for collision detection
let mut all_actions: Vec<(String, FileAction, PathBuf, Option<String>)> = Vec::new(); // (pkg_name, action, pkg_target, rendered)
let mut staging_paths: HashMap<PathBuf, String> = HashMap::new(); // staging_path -> pkg_name

// ... scan loop populates all_actions and checks staging_paths for duplicates ...

for (pkg_name, action, _, _) in &all_actions {
    let staging_path = self.staging_dir.join(&action.target_rel_path);
    if let Some(existing_pkg) = staging_paths.get(&staging_path) {
        bail!(
            "staging collision -- packages '{}' and '{}' both deploy {}",
            existing_pkg,
            pkg_name,
            action.target_rel_path.display()
        );
    }
    staging_paths.insert(staging_path, pkg_name.clone());
}
```

For drift detection, load existing state and check content hashes before overwriting:

```rust
let existing_state = self.state_dir.as_ref()
    .map(|d| DeployState::load(d))
    .transpose()?
    .unwrap_or_default();

// Build lookup from staged path to existing content_hash
let existing_hashes: HashMap<&Path, &str> = existing_state.entries()
    .iter()
    .map(|e| (e.staged.as_path(), e.content_hash.as_str()))
    .collect();

// Before staging a file:
if staged_path.exists() {
    if let Some(&expected_hash) = existing_hashes.get(staged_path.as_path()) {
        let current_hash = hash::hash_file(&staged_path)?;
        if current_hash != expected_hash && !force {
            eprintln!("warning: {} has been modified since last deploy, skipping (use --force to overwrite)", action.target_rel_path.display());
            // record in report.conflicts or a new report.skipped
            continue;
        }
    }
}
```

After staging, apply permission overrides:

```rust
if let Some(pkg_config) = self.loader.root().packages.get(pkg_name) {
    let rel_path_str = action.target_rel_path.to_str().unwrap_or("");
    if let Some(mode) = pkg_config.permissions.get(rel_path_str) {
        deployer::apply_permission_override(&staged_path, mode)?;
    }
}
```

**Step 4: Update existing e2e tests**

The existing `e2e_deploy_and_undeploy`, `e2e_deploy_with_overrides`, `e2e_deploy_with_template_rendering`, `e2e_idempotent_deploy`, and `e2e_role_override_when_no_host_match` tests need updates:

- Overrides and templates are now also symlinks (not direct copies). Update assertions that check `!app_conf.is_symlink()` to instead check `app_conf.is_symlink()`.
- Content assertions still work — reading a symlink reads the target's content.
- Tests that use `with_state_dir` should continue to work with the new state format.

**Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass

**Step 6: Commit**

```
feat: rewrite orchestrator with staging pipeline and collision detection
```

---

### Task 8: Update the Status command with drift detection

**Files:**
- Modify: `src/main.rs` (Status command handler)

**Step 1: Write a manual integration test approach**

Since the CLI is tested via e2e tests, add a library function first. Add to `src/state.rs`:

```rust
use crate::hash;

#[derive(Debug, PartialEq, Eq)]
pub enum FileStatus {
    Ok,
    Modified,
    Missing,
}

impl DeployState {
    pub fn check_entry_status(&self, entry: &DeployEntry) -> FileStatus {
        if !entry.target.exists() && !entry.target.is_symlink() {
            return FileStatus::Missing;
        }
        if entry.staged.exists() {
            if let Ok(current_hash) = hash::hash_file(&entry.staged) {
                if current_hash != entry.content_hash {
                    return FileStatus::Modified;
                }
            }
        } else {
            return FileStatus::Missing;
        }
        FileStatus::Ok
    }
}
```

Test in `tests/state.rs`:

```rust
#[test]
fn check_entry_status_detects_modified() {
    use dotm::state::FileStatus;

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
        kind: EntryKind::Base,
        package: "test".to_string(),
    };

    // Initially ok
    assert_eq!(state.check_entry_status(&entry), FileStatus::Ok);

    // Modify the staged file (simulating an app changing it)
    std::fs::write(&staged_path, "modified content").unwrap();
    assert_eq!(state.check_entry_status(&entry), FileStatus::Modified);
}
```

**Step 2: Run test to verify it fails, implement, verify it passes**

Run: `cargo test --test state check_entry_status`

**Step 3: Update the Status handler in main.rs**

Replace the `Commands::Status` match arm to use the new `check_entry_status`. Show `[ok]`, `[MODIFIED]`, `[MISSING]` per entry. Print summary counts at the bottom. Exit with code 1 if any are modified or missing.

```rust
Commands::Status => {
    let state_dir = dotm_state_dir();
    let state = dotm::state::DeployState::load(&state_dir)?;
    let entries = state.entries();

    if entries.is_empty() {
        println!("No files currently managed by dotm.");
        return Ok(());
    }

    let mut ok_count = 0;
    let mut modified_count = 0;
    let mut missing_count = 0;

    println!("Managed files:");
    for entry in entries {
        let status = state.check_entry_status(entry);
        let label = match status {
            dotm::state::FileStatus::Ok => { ok_count += 1; "ok" }
            dotm::state::FileStatus::Modified => { modified_count += 1; "MODIFIED" }
            dotm::state::FileStatus::Missing => { missing_count += 1; "MISSING" }
        };
        println!("  [{:8}] {}", label, entry.target.display());
    }

    println!();
    println!(
        "{} managed, {} modified, {} missing.",
        entries.len(), modified_count, missing_count
    );

    if modified_count > 0 || missing_count > 0 {
        if modified_count > 0 {
            println!("Run 'dotm diff' to see changes, 'dotm adopt' to review and accept.");
        }
        std::process::exit(1);
    }
}
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: PASS

**Step 5: Commit**

```
feat: add drift detection to status command with exit codes
```

---

### Task 9: Add the `diff` command

**Files:**
- Create: `src/diff.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

**Step 1: Write failing test for diff output**

In `src/diff.rs` (inline tests):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_diff_shows_unified_output() {
        let original = "line1\nline2\nline3\n";
        let modified = "line1\nmodified_line2\nline3\nnew_line4\n";
        let output = format_unified_diff(original, modified, "deployed: .config/app.conf", "current: .config/app.conf");
        assert!(output.contains("--- deployed:"));
        assert!(output.contains("+++ current:"));
        assert!(output.contains("-line2"));
        assert!(output.contains("+modified_line2"));
        assert!(output.contains("+new_line4"));
    }
}
```

**Step 2: Implement src/diff.rs**

```rust
use similar::{ChangeTag, TextDiff};

pub fn format_unified_diff(original: &str, modified: &str, label_a: &str, label_b: &str) -> String {
    let diff = TextDiff::from_lines(original, modified);
    let mut output = String::new();

    output.push_str(&format!("--- {}\n", label_a));
    output.push_str(&format!("+++ {}\n", label_b));

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&format!("{}", hunk));
    }

    output
}
```

Add `pub mod diff;` to `src/lib.rs`.

**Step 3: Add Diff command to main.rs**

Add to the `Commands` enum:

```rust
/// Show diffs for files modified since last deploy
Diff {
    /// Only show diff for a specific file path
    path: Option<String>,
},
```

Implement the handler: load state, for each entry with `FileStatus::Modified`, read the staged file content, reconstruct what was deployed (from `content_hash` — but we need the original content; store it or re-derive it).

**Important design note:** The diff needs to compare what was originally deployed vs what's currently in `.staged/`. We need the original content, not just the hash. Two options:
- Store the original content in a parallel directory (e.g., `.staged/.originals/`)
- Re-derive from source (re-scan, re-render). This is more correct since it shows drift from current source too.

Re-derivation is better: run the same scan+render pipeline to generate what *would* be deployed now, then diff against what's actually in `.staged/`. This also catches cases where both the source and the staged file changed.

The handler:

```rust
Commands::Diff { path } => {
    let state_dir = dotm_state_dir();
    let state = dotm::state::DeployState::load(&state_dir)?;

    // Re-derive expected content for each entry...
    // For entries where the staged file content differs from expected, show diff
    for entry in state.entries() {
        if let Some(ref filter) = path {
            if !entry.target.to_str().unwrap_or("").contains(filter) {
                continue;
            }
        }

        let status = state.check_entry_status(entry);
        if status != dotm::state::FileStatus::Modified {
            continue;
        }

        let current = std::fs::read_to_string(&entry.staged).unwrap_or_default();
        // For re-derivation, we need the orchestrator. For v1, compare against content_hash
        // by storing original content alongside. See note below.
        // Simplified approach: show current content and note the hash changed.
        // Full approach requires re-running scan+render.
    }
}
```

**Simpler v1 approach:** Store a copy of the deployed content in the state dir (e.g., `~/.local/state/dotm/originals/<hash>`). On diff, read the original from there and compare against current staged content.

Add to `DeployState`:

```rust
pub fn originals_dir(&self) -> PathBuf {
    self.state_dir.join("originals")
}

pub fn store_original(&self, content_hash: &str, content: &[u8]) -> Result<()> {
    let dir = self.originals_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(content_hash), content)?;
    Ok(())
}

pub fn load_original(&self, content_hash: &str) -> Result<Vec<u8>> {
    let path = self.originals_dir().join(content_hash);
    std::fs::read(&path).with_context(|| format!("failed to read original: {}", path.display()))
}
```

Call `state.store_original()` in the orchestrator after staging each file.

**Step 4: Run all tests**

Run: `cargo test`
Expected: PASS

**Step 5: Commit**

```
feat: add diff command showing unified diffs for drifted files
```

---

### Task 10: Add the `adopt` command with interactive hunk selection

This is the most complex task. It uses the `similar` crate for diff computation and `crossterm` for terminal interaction.

**Files:**
- Create: `src/adopt.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`

**Step 1: Write failing tests for hunk extraction**

In `src/adopt.rs` (inline tests):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_hunks_from_diff() {
        let original = "line1\nline2\nline3\nline4\nline5\n";
        let modified = "line1\nchanged2\nline3\nline4\nnew5\n";

        let hunks = extract_hunks(original, modified);
        assert_eq!(hunks.len(), 2);
    }

    #[test]
    fn apply_selected_hunks() {
        let original = "line1\nline2\nline3\nline4\nline5\n";
        let modified = "line1\nchanged2\nline3\nline4\nnew5\n";

        let hunks = extract_hunks(original, modified);
        // Accept only the first hunk
        let result = apply_hunks(original, &hunks, &[true, false]);
        assert!(result.contains("changed2"));
        assert!(result.contains("line5")); // second hunk not applied
    }
}
```

**Step 2: Implement src/adopt.rs**

Core functions:
- `extract_hunks(original: &str, modified: &str) -> Vec<Hunk>` — uses `similar::TextDiff` to extract hunks
- `apply_hunks(original: &str, hunks: &[Hunk], accepted: &[bool]) -> String` — applies selected hunks to the original
- `interactive_adopt(entry: &DeployEntry, original: &str, current: &str) -> Result<Option<String>>` — shows each hunk with (y/n/q) prompts using crossterm, returns the patched content or None if all rejected

The Hunk struct:

```rust
pub struct Hunk {
    pub header: String,
    pub lines: Vec<HunkLine>,
}

pub enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
}
```

The interactive UI: for each hunk, display it with coloring (red for removals, green for additions), prompt `Accept this change? [y/n/q] `. Collect responses, then apply accepted hunks.

**Step 3: Add Adopt command to main.rs**

```rust
/// Interactively adopt changes made to deployed files
Adopt,
```

Handler:
1. Load state
2. For each modified entry:
   a. Load original content from `state.load_original()`
   b. Read current staged content
   c. If `entry.kind == EntryKind::Template`, warn and skip
   d. Call `adopt::interactive_adopt()` to get patched content
   e. If user accepted changes, write patched content back to `entry.source` in packages/
   f. Re-stage from the updated source, update content hash in state

**Step 4: Run all tests**

Run: `cargo test`
Expected: PASS

**Step 5: Manual testing**

The interactive UI can't be unit-tested easily. Manual test:
1. `cargo run -- deploy --host <hostname>`
2. Modify a staged file: `echo "change" >> ~/dotfiles/.staged/.bashrc`
3. `cargo run -- status` — should show MODIFIED
4. `cargo run -- diff` — should show the diff
5. `cargo run -- adopt` — should show hunks interactively

**Step 6: Commit**

```
feat: add adopt command with interactive hunk-level selection
```

---

### Task 11: Update the `undeploy` command and add `.staged/` cleanup

**Files:**
- Modify: `src/state.rs` (undeploy should clean .staged/ empty dirs)
- Modify: `src/main.rs` (update undeploy handler)

**Step 1: Write failing test**

Add to `tests/state.rs`:

```rust
#[test]
fn undeploy_cleans_empty_staged_directories() {
    let staged_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    // Create nested staged file
    let staged_parent = staged_dir.path().join(".config/nested");
    std::fs::create_dir_all(&staged_parent).unwrap();
    let staged_path = staged_parent.join("file.conf");
    std::fs::write(&staged_path, "content").unwrap();

    let target_path = target_dir.path().join(".config/nested/file.conf");
    std::fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&staged_path, &target_path).unwrap();

    let state_dir = TempDir::new().unwrap();
    let mut state = DeployState::new(state_dir.path());
    state.record(DeployEntry {
        target: target_path.clone(),
        staged: staged_path.clone(),
        source: PathBuf::from("irrelevant"),
        content_hash: "hash".to_string(),
        kind: EntryKind::Base,
        package: "test".to_string(),
    });
    state.save().unwrap();

    state.undeploy().unwrap();
    assert!(!staged_path.exists());
    // Empty parent dirs should be cleaned up
    assert!(!staged_parent.exists());
}
```

**Step 2: Implement empty directory cleanup in undeploy**

After removing each staged file, walk up the parent directories and remove any that are empty:

```rust
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
```

**Step 3: Also clean originals dir on undeploy**

After removing all files, remove the originals directory if it exists.

**Step 4: Run all tests**

Run: `cargo test`
Expected: PASS

**Step 5: Commit**

```
feat: clean up empty directories on undeploy
```

---

### Task 12: Add `.staged/` to dotfiles .gitignore

**Files:**
- Modify: `~/dotfiles/.gitignore` (the user's actual dotfiles repo, NOT the dotm code repo)

This is a documentation/setup step. The `dotm init` command or `dotm deploy` should warn if `.staged/` is not gitignored.

**Step 1: Add a warning in deploy when .staged/ is not gitignored**

In the orchestrator or main.rs, after deployment:

```rust
let gitignore_path = self.loader.base_dir().join(".gitignore");
let staged_ignored = if gitignore_path.exists() {
    std::fs::read_to_string(&gitignore_path)
        .map(|c| c.lines().any(|l| l.trim() == ".staged" || l.trim() == ".staged/"))
        .unwrap_or(false)
} else {
    false
};
if !staged_ignored {
    eprintln!("warning: '.staged/' is not in your .gitignore — add it to avoid committing staged files");
}
```

**Step 2: Commit**

```
feat: warn when .staged/ is not gitignored
```

---

### Task 13: Update existing e2e tests for new behavior

All existing e2e tests need to be verified and updated for the new staging behavior. Key changes:

- All deployed files are now symlinks (including overrides and templates)
- Symlinks point into `.staged/` not directly into `packages/`
- State format has changed

**Files:**
- Modify: `tests/e2e.rs`
- Modify: `tests/orchestrator.rs`

**Step 1: Update each test**

Go through each existing test and update assertions:

- `e2e_deploy_and_undeploy`: `.bashrc` is a symlink → still true, but target is `.staged/`. Update assertion to check symlink target contains `.staged`.
- `e2e_deploy_with_overrides`: `app.conf` was asserted `!is_symlink()` → now it IS a symlink. Update to check it's a symlink AND content still contains "myhost".
- `e2e_deploy_with_template_rendering`: `templated.conf` was asserted `!is_symlink()` → now it IS a symlink. Content check for "blue" still works via symlink.
- `e2e_idempotent_deploy`: Should still work — re-deploy replaces symlinks.
- `e2e_role_override_when_no_host_match`: Same symlink update.
- `full_deploy_basic_fixture` in orchestrator.rs: Update similarly.

**Step 2: Run all tests**

Run: `cargo test`
Expected: PASS

**Step 3: Commit**

```
test: update e2e tests for staging directory behavior
```

---

### Task 14: Add permission override tests

**Files:**
- Test: `tests/e2e.rs` or `tests/deployer.rs`

**Step 1: Write test for permission override via config**

Create a dynamic test fixture with a package that has a permissions config:

```rust
#[test]
fn e2e_permission_override_applied() {
    use std::os::unix::fs::PermissionsExt;

    let dotfiles_tmp = TempDir::new().unwrap();

    std::fs::write(
        dotfiles_tmp.path().join("dotm.toml"),
        r#"
[dotm]
target = "~"

[packages.scripts]
description = "Scripts"

[packages.scripts.permissions]
"bin/myscript" = "755"
"#,
    ).unwrap();

    let pkg_dir = dotfiles_tmp.path().join("packages/scripts/bin");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(pkg_dir.join("myscript"), "#!/bin/bash\necho hi").unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("hosts")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("hosts/testhost.toml"),
        "hostname = \"testhost\"\nroles = [\"all\"]\n",
    ).unwrap();

    std::fs::create_dir_all(dotfiles_tmp.path().join("roles")).unwrap();
    std::fs::write(
        dotfiles_tmp.path().join("roles/all.toml"),
        "packages = [\"scripts\"]\n",
    ).unwrap();

    let target = TempDir::new().unwrap();
    let state_dir = TempDir::new().unwrap();
    let mut orch = Orchestrator::new(dotfiles_tmp.path(), target.path()).unwrap()
        .with_state_dir(state_dir.path());
    orch.deploy("testhost", false, false).unwrap();

    // The staged file should have 755 permissions
    let staged = dotfiles_tmp.path().join(".staged/bin/myscript");
    let mode = staged.metadata().unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
}
```

**Step 2: Run test, verify it passes (should pass if Task 7 wired permissions correctly)**

Run: `cargo test --test e2e e2e_permission_override_applied`
Expected: PASS

**Step 3: Commit**

```
test: add permission override integration test
```

---

### Execution Notes

**Task ordering:** Tasks 1-6 are foundational and must be done sequentially. Tasks 7-8 depend on 1-6. Task 9 depends on 8. Task 10 depends on 9. Tasks 11-14 can be done in any order after Task 7.

**Key crates to reference:**
- `sha2` docs: `Sha256::new()`, `hasher.update()`, `hasher.finalize()`, `format!("{:x}", ...)`
- `similar` docs: `TextDiff::from_lines()`, `.unified_diff()`, `.iter_hunks()`
- `crossterm` docs: `crossterm::event::read()`, `crossterm::style::*` for colored output

**Testing approach:** Unit tests for pure logic (hashing, diffing, hunk extraction). Integration tests via e2e fixtures for the full pipeline. Interactive adopt requires manual testing.

**Migration:** After all tasks are done, users need to `dotm undeploy && dotm deploy` once to migrate from the old state format to the new one.
