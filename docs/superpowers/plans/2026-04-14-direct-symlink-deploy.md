# Direct Symlink Deployment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace dotm's staging-based deployment with direct symlinks to source files, eliminating the `.staged/` directory and `adopt` workflow.

**Architecture:** User-mode base/override files become symlinks directly to source in `packages/`. Templates and system-mode files are copied to target. State moves from `$XDG_STATE_HOME/dotm/` to `~/.dotm/`. The `adopt` command and `DeployStrategy` config are removed entirely.

**Tech Stack:** Rust (edition 2024), anyhow, serde, similar, crossterm, clap, fs2

**Spec:** `docs/superpowers/specs/2026-04-14-direct-symlink-deploy-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/config.rs` | Modify | Remove `DeployStrategy` enum, deprecation warning for `strategy` field, update `validate_system_packages()` |
| `src/state.rs` | Modify | `DeployEntry.staged` -> `Option<PathBuf>`, state dir to `~/.dotm/`, legacy fallback, version bump to 3, rewrite `check_entry_status`, remove `deployed/` storage, remove staged cleanup from restore/undeploy |
| `src/deployer.rs` | Rewrite | Replace `deploy_staged()` with `deploy_symlink()`, update `deploy_copy()` with `is_dir()` guard |
| `src/orchestrator.rs` | Modify | Remove staging, target-path collision detection, orchestrator decision tree, drift detection only for copies, remove `.gitignore` warning |
| `src/main.rs` | Modify | Remove `Adopt` subcommand, rewrite `Diff` handler, update state dir resolution with legacy fallback, update status message, update `Prune` handler |
| `src/scanner.rs` | Modify | Update doc comments on `EntryKind::Base` and `EntryKind::Override` |
| `src/list.rs` | Modify | Remove strategy display from verbose output |
| `src/lib.rs` | Modify | Remove `pub mod adopt;` |
| `src/adopt.rs` | Delete | Entire module removed |
| `tests/deployer.rs` | Rewrite | Test `deploy_symlink()` and updated `deploy_copy()` |
| `tests/state.rs` | Modify | Update `DeployEntry` construction, test new `check_entry_status` logic |
| `tests/orchestrator.rs` | Modify | Assert symlink-to-source, not symlink-to-staged |
| `tests/e2e.rs` | Modify | Remove `.staged` assertions, update drift/permission tests |
| `tests/system_packages.rs` | Modify | Remove staging assertions, test system-mode copy behavior |
| `tests/cli.rs` | Modify | Update state dir overrides |
| `tests/copy_drift.rs` | Modify | Update `DeployEntry` construction |
| `tests/orphan.rs` | Modify | Minor fixture updates |

---

### Task 1: Remove adopt module and CLI subcommand

This is fully independent — nothing else depends on `adopt.rs`.

**Files:**
- Delete: `src/adopt.rs`
- Modify: `src/lib.rs:1`
- Modify: `src/main.rs:69-73` (Adopt variant), `src/main.rs:437-504` (Adopt handler), `src/main.rs:387` (status message)

- [ ] **Step 1: Delete adopt.rs**

```bash
rm src/adopt.rs
```

- [ ] **Step 2: Remove adopt from lib.rs**

In `src/lib.rs`, remove the line:
```rust
pub mod adopt;
```

- [ ] **Step 3: Remove Adopt CLI subcommand and handler from main.rs**

In `src/main.rs`, remove the `Adopt` variant from the `Commands` enum (lines 69-73):
```rust
    /// Interactively adopt changes made to deployed files back into source
    Adopt {
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
```

Remove the entire `Commands::Adopt { system }` match arm (lines 437-504).

- [ ] **Step 4: Update status message to reference deploy instead of adopt**

In `src/main.rs`, change the status message (line 387) from:
```rust
                    println!("Run 'dotm diff' to see changes, 'dotm adopt' to review and accept.");
```
to:
```rust
                    println!("Run 'dotm diff' to see changes, 'dotm deploy' to re-sync.");
```

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo build 2>&1 | head -20`
Expected: compiles successfully

Run: `cargo test 2>&1 | tail -5`
Expected: all existing tests pass (adopt had no integration tests)

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "remove adopt module and CLI subcommand

Direct symlinks make adopt unnecessary — edits to deployed files
are already edits to the source. Templates can't reverse a render,
so the user edits the .tera source and re-deploys."
```

---

### Task 2: Config changes — deprecate strategy, update validation

**Files:**
- Modify: `src/config.rs:26-31` (DeployStrategy), `src/config.rs:58-71` (validate_system_packages)
- Modify: `src/list.rs:27-29` (strategy display)

- [ ] **Step 1: Write test for deprecation warning**

In `src/config.rs`, add to the existing `#[cfg(test)] mod tests` (or create one at the bottom):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_system_packages_does_not_require_strategy() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.sys]
system = true
target = "/etc/sys"
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        let errors = validate_system_packages(&root);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    #[test]
    fn strategy_field_still_parses_without_error() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.sys]
system = true
target = "/etc/sys"
strategy = "copy"
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        assert!(root.packages["sys"].strategy.is_some());
    }

    #[test]
    fn warn_deprecated_strategy_field() {
        let toml_str = r#"
[dotm]
target = "~"

[packages.shell]
strategy = "stage"
"#;
        let root: RootConfig = toml::from_str(toml_str).unwrap();
        let warnings = deprecated_strategy_warnings(&root);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("shell"));
        assert!(warnings[0].contains("deprecated"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib config::tests 2>&1 | tail -10`
Expected: FAIL — `deprecated_strategy_warnings` not found, and strategy validation test fails

- [ ] **Step 3: Remove strategy requirement from validate_system_packages**

In `src/config.rs`, remove lines 67-70 from `validate_system_packages()`:
```rust
            if pkg.strategy.is_none() {
                errors.push(format!(
                    "system package '{name}' must specify a deployment strategy"
                ));
            }
```

- [ ] **Step 4: Add deprecated_strategy_warnings function**

In `src/config.rs`, add after `validate_system_packages`:

```rust
pub fn deprecated_strategy_warnings(root: &RootConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    for (name, pkg) in &root.packages {
        if pkg.strategy.is_some() {
            warnings.push(format!(
                "warning: 'strategy' field on package '{name}' is deprecated and ignored; deployment mode is now determined automatically"
            ));
        }
    }
    warnings
}
```

- [ ] **Step 5: Remove strategy display from list.rs**

In `src/list.rs`, remove lines 27-29 from `render_packages`:
```rust
            if let Some(strategy) = pkg.strategy {
                out.push_str(&format!("  strategy: {strategy:?}\n"));
            }
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib config::tests 2>&1 | tail -10`
Expected: all 3 new tests PASS

Run: `cargo test --test list 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/list.rs && git commit -m "deprecate strategy field, remove from validation and list output

The strategy field is kept in PackageConfig for TOML parsing compat
but is now ignored. A new deprecated_strategy_warnings() function
produces warnings when strategy is set. validate_system_packages()
no longer requires strategy on system packages."
```

---

### Task 3: State layer — DeployEntry changes, state dir, version bump

This is the biggest foundation change. Everything downstream depends on it.

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Change DeployEntry.staged from PathBuf to Option\<PathBuf\>**

In `src/state.rs`, change the `DeployEntry` struct (line 74-95):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployEntry {
    pub target: PathBuf,
    #[serde(default)]
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
```

- [ ] **Step 2: Bump CURRENT_VERSION to 3**

In `src/state.rs`, change line 60:
```rust
const CURRENT_VERSION: u32 = 3;
```

- [ ] **Step 3: Update state dir default and add legacy fallback**

Replace `dotm_state_dir()` in `src/main.rs` (lines 961-966):

```rust
fn dotm_state_dir() -> PathBuf {
    let dotm_dir = dirs::home_dir()
        .expect("could not determine home directory")
        .join(".dotm");

    if dotm_dir.join("dotm-state.json").exists() {
        return dotm_dir;
    }

    // Legacy fallback: check XDG_STATE_HOME
    let legacy = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
        .expect("could not determine state directory")
        .join("dotm");

    if legacy.join("dotm-state.json").exists() {
        eprintln!("note: reading state from legacy location; run 'dotm deploy' to migrate to ~/.dotm/");
        return legacy;
    }

    // Default to new location
    dotm_dir
}
```

- [ ] **Step 4: Remove deployed/ storage methods from DeployState**

In `src/state.rs`, remove these methods from `impl DeployState`:

- `deployed_dir()` (lines 245-247)
- `store_deployed()` (lines 249-259)
- `load_deployed()` (lines 261-265)
- `migrate_storage()` (lines 267-275) — keep for now, used during v2 loading, but update to not rename

- [ ] **Step 5: Rewrite check_entry_status**

Replace `check_entry_status` in `src/state.rs` (lines 182-221):

```rust
    pub fn check_entry_status(&self, entry: &DeployEntry) -> FileStatus {
        // Check if target exists at all
        if !entry.target.exists() && !entry.target.is_symlink() {
            return FileStatus::missing();
        }

        let mut status = FileStatus::ok();

        if entry.target.is_symlink() {
            // Symlink entry: check it points to the right source
            // Transitional v2 check: if staged is Some and symlink points to staged path,
            // fall back to v2 behavior (hash the staged file)
            if let Some(ref staged) = entry.staged {
                if let Ok(link_target) = std::fs::read_link(&entry.target) {
                    let staged_canonical = std::fs::canonicalize(staged).ok();
                    let link_canonical = std::fs::canonicalize(&link_target).ok();
                    if staged_canonical.is_some() && staged_canonical == link_canonical {
                        // v2 transitional: symlink points to staged, use old hash check
                        if let Ok(current_hash) = hash::hash_file(staged) {
                            if current_hash != entry.content_hash {
                                status.content_modified = true;
                            }
                        }
                        return status;
                    }
                }
            }

            // v3 behavior: symlink should point to entry.source
            if let Ok(link_target) = std::fs::read_link(&entry.target) {
                let source_canonical = std::fs::canonicalize(&entry.source).ok();
                let link_canonical = std::fs::canonicalize(&link_target).ok();
                if source_canonical != link_canonical {
                    return FileStatus::missing(); // symlink points elsewhere
                }
            } else {
                return FileStatus::missing();
            }
        } else {
            // Copy entry: hash target content and compare
            if let Ok(current_hash) = hash::hash_file(&entry.target) {
                if current_hash != entry.content_hash {
                    status.content_modified = true;
                }
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
```

- [ ] **Step 6: Remove staged cleanup from restore(), undeploy(), undeploy_package()**

In `restore()`, remove lines 320-325:
```rust
            // Clean up staged file if separate from target
            if entry.staged != entry.target && entry.staged.exists() {
                std::fs::remove_file(&entry.staged)
                    .with_context(|| format!("failed to remove staged: {}", entry.staged.display()))?;
                cleanup_empty_parents(&entry.staged);
            }
```

In `undeploy_package()`, remove lines 361-365:
```rust
                if entry.staged != entry.target && entry.staged.exists() {
                    std::fs::remove_file(&entry.staged)
                        .with_context(|| format!("failed to remove staged file: {}", entry.staged.display()))?;
                    cleanup_empty_parents(&entry.staged);
                }
```

In `undeploy()`, remove lines 389-392:
```rust
            if entry.staged.exists() {
                std::fs::remove_file(&entry.staged)
                    .with_context(|| format!("failed to remove staged file: {}", entry.staged.display()))?;
                cleanup_empty_parents(&entry.staged);
            }
```

- [ ] **Step 7: Fix all compilation errors from staged type change**

The `staged` field changed from `PathBuf` to `Option<PathBuf>`. Fix all callers:

In `src/orchestrator.rs`:
- Every place that constructs a `DeployEntry` must use `staged: None` instead of `staged: staged_path.clone()`
- Every place that reads `entry.staged` must handle `Option` (use `.as_ref()` or match)
- Remove all `entry.staged` comparisons in orphan cleanup

In `src/main.rs`:
- `Diff` handler references `entry.staged` — will be rewritten in Task 5, for now change to `entry.target` temporarily
- `Prune` handler `entry.staged` cleanup — remove the staged cleanup block (lines 824-827)

This step requires checking every compilation error and fixing. Run `cargo build` iteratively until it compiles.

- [ ] **Step 8: Run tests and fix state test failures**

Run: `cargo test --test state 2>&1`

Fix tests in `tests/state.rs` — update all `DeployEntry` construction to use `staged: None` instead of `staged: PathBuf::from(...)`.

Update `check_entry_status_detects_modified` test to create a regular file (not a staged symlink scenario):

```rust
#[test]
fn check_entry_status_detects_modified() {
    let dir = TempDir::new().unwrap();
    let target_path = dir.path().join("target_file");
    std::fs::write(&target_path, "original content").unwrap();

    let state = DeployState::new(dir.path());
    let original_hash = dotm::hash::hash_content(b"original content");

    let entry = DeployEntry {
        target: target_path.clone(),
        staged: None,
        source: PathBuf::from("/source/file"),
        content_hash: original_hash,
        original_hash: None,
        kind: dotm::scanner::EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    };

    // Initially should be OK
    let status = state.check_entry_status(&entry);
    assert!(status.is_ok());

    // Modify the target
    std::fs::write(&target_path, "modified content").unwrap();
    let status = state.check_entry_status(&entry);
    assert!(status.is_modified());
}
```

Update `undeploy_removes_target_and_staged` to not test staged cleanup:

```rust
#[test]
fn undeploy_removes_target_file() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target_file");
    std::fs::write(&target, "content").unwrap();

    let mut state = DeployState::new(dir.path());
    state.record(DeployEntry {
        target: target.clone(),
        staged: None,
        source: PathBuf::from("/source"),
        content_hash: "hash".to_string(),
        original_hash: None,
        kind: dotm::scanner::EntryKind::Base,
        package: "test".to_string(),
        owner: None,
        group: None,
        mode: None,
        original_owner: None,
        original_group: None,
        original_mode: None,
    });

    let removed = state.undeploy().unwrap();
    assert_eq!(removed, 1);
    assert!(!target.exists());
}
```

- [ ] **Step 9: Run full test suite**

Run: `cargo test 2>&1 | tail -20`

Some tests in other files may fail due to `staged` type change — note which ones fail. They will be fixed in later tasks. The `state` tests should pass.

- [ ] **Step 10: Commit**

```bash
git add -A && git commit -m "rewrite state layer for direct symlink model

- DeployEntry.staged becomes Option<PathBuf> for v2/v3 compat
- CURRENT_VERSION bumped to 3
- State dir defaults to ~/.dotm/ with legacy XDG fallback
- check_entry_status branches on symlink vs copy entries
- Remove deployed/ storage methods (store_deployed, load_deployed)
- Remove staged cleanup from restore/undeploy/undeploy_package"
```

---

### Task 4: Deployer rewrite

**Files:**
- Modify: `src/deployer.rs`
- Rewrite: `tests/deployer.rs`

- [ ] **Step 1: Write tests for deploy_symlink**

Rewrite `tests/deployer.rs`:

```rust
use dotm::deployer::{self, DeployResult};
use dotm::scanner::{EntryKind, FileAction};
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

#[test]
fn symlink_base_file_to_source() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join(".bashrc");
    std::fs::write(&source, "export PATH=$PATH").unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from(".bashrc"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Created));

    let target = target_dir.path().join(".bashrc");
    assert!(target.is_symlink());
    let link_dest = std::fs::read_link(&target).unwrap();
    assert_eq!(std::fs::canonicalize(&link_dest).unwrap(), std::fs::canonicalize(&source).unwrap());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "export PATH=$PATH");
}

#[test]
fn symlink_replaces_existing_symlink() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let old_source = source_dir.path().join("old");
    std::fs::write(&old_source, "old").unwrap();
    let new_source = source_dir.path().join(".bashrc");
    std::fs::write(&new_source, "new").unwrap();

    let target = target_dir.path().join(".bashrc");
    std::os::unix::fs::symlink(&old_source, &target).unwrap();

    let action = FileAction {
        source: new_source.clone(),
        target_rel_path: std::path::PathBuf::from(".bashrc"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Updated));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
}

#[test]
fn symlink_conflicts_with_unmanaged_regular_file() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join(".bashrc");
    std::fs::write(&source, "new").unwrap();

    let target = target_dir.path().join(".bashrc");
    std::fs::write(&target, "existing").unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from(".bashrc"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));
}

#[test]
fn symlink_force_overwrites_regular_file() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join(".bashrc");
    std::fs::write(&source, "new").unwrap();

    let target = target_dir.path().join(".bashrc");
    std::fs::write(&target, "existing").unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from(".bashrc"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, true).unwrap();
    assert!(matches!(result, DeployResult::Created | DeployResult::Updated));
    assert!(target.is_symlink());
}

#[test]
fn symlink_errors_on_directory_target() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join("config");
    std::fs::write(&source, "content").unwrap();

    // Create a directory at the target path
    std::fs::create_dir_all(target_dir.path().join("config")).unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from("config"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), false, false).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));
    if let DeployResult::Conflict(msg) = result {
        assert!(msg.contains("directory"), "expected directory error, got: {msg}");
    }
}

#[test]
fn symlink_dry_run_creates_nothing() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join(".bashrc");
    std::fs::write(&source, "content").unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from(".bashrc"),
        kind: EntryKind::Base,
    };

    let result = deploy_symlink(&action, target_dir.path(), true, false).unwrap();
    assert!(matches!(result, DeployResult::DryRun));
    assert!(!target_dir.path().join(".bashrc").exists());
}

#[test]
fn copy_writes_rendered_template() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join("config.tera");
    std::fs::write(&source, "template source").unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from("config"),
        kind: EntryKind::Template,
    };

    let result = deployer::deploy_copy(&action, target_dir.path(), false, false, Some("rendered content")).unwrap();
    assert!(matches!(result, DeployResult::Created));
    assert_eq!(std::fs::read_to_string(target_dir.path().join("config")).unwrap(), "rendered content");
    assert!(!target_dir.path().join("config").is_symlink());
}

#[test]
fn copy_errors_on_directory_target() {
    let source_dir = TempDir::new().unwrap();
    let target_dir = TempDir::new().unwrap();

    let source = source_dir.path().join("config");
    std::fs::write(&source, "content").unwrap();

    std::fs::create_dir_all(target_dir.path().join("config")).unwrap();

    let action = FileAction {
        source: source.clone(),
        target_rel_path: std::path::PathBuf::from("config"),
        kind: EntryKind::Base,
    };

    let result = deployer::deploy_copy(&action, target_dir.path(), false, false, None).unwrap();
    assert!(matches!(result, DeployResult::Conflict(_)));
}

#[test]
fn apply_permission_override_sets_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("script.sh");
    std::fs::write(&file, "#!/bin/sh").unwrap();

    deployer::apply_permission_override(&file, "755").unwrap();

    let meta = std::fs::metadata(&file).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o755);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test deployer 2>&1 | tail -10`
Expected: FAIL — `deploy_symlink` function doesn't exist yet

- [ ] **Step 3: Rewrite deployer.rs**

Replace `src/deployer.rs` entirely:

```rust
use crate::scanner::FileAction;
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

/// Deploy a file by creating a symlink from target to the source file.
///
/// Used for user-mode Base and Override files. The symlink points to the
/// canonicalized absolute path of the source file in packages/.
pub fn deploy_symlink(
    action: &FileAction,
    target_dir: &Path,
    dry_run: bool,
    force: bool,
) -> Result<DeployResult> {
    let target_path = target_dir.join(&action.target_rel_path);

    if dry_run {
        return Ok(DeployResult::DryRun);
    }

    let was_existing = target_path.is_symlink() || target_path.exists();

    // Handle existing target
    if target_path.is_dir() && !target_path.is_symlink() {
        return Ok(DeployResult::Conflict(format!(
            "target is a directory (remove it manually): {}",
            target_path.display()
        )));
    }

    if target_path.is_symlink() {
        std::fs::remove_file(&target_path)
            .with_context(|| format!("failed to remove existing symlink: {}", target_path.display()))?;
    } else if target_path.exists() {
        if force {
            std::fs::remove_file(&target_path)
                .with_context(|| format!("failed to remove existing file: {}", target_path.display()))?;
        } else {
            return Ok(DeployResult::Conflict(format!(
                "file already exists and is not managed by dotm: {}",
                target_path.display()
            )));
        }
    }

    // Create parent directories
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create target directory: {}", parent.display()))?;
    }

    // Create symlink to canonicalized source path
    let abs_source = std::fs::canonicalize(&action.source)
        .with_context(|| format!("failed to canonicalize source path: {}", action.source.display()))?;
    std::os::unix::fs::symlink(&abs_source, &target_path)
        .with_context(|| format!("failed to create symlink: {} -> {}", target_path.display(), abs_source.display()))?;

    if was_existing {
        Ok(DeployResult::Updated)
    } else {
        Ok(DeployResult::Created)
    }
}

/// Deploy a file by copying content directly to the target.
///
/// Used for templates (rendered content) and system-mode files.
/// Templates get rendered content written; base/override files are copied from source.
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

    let was_existing = target_path.is_symlink() || target_path.exists();

    // Handle existing target
    if target_path.is_dir() && !target_path.is_symlink() {
        return Ok(DeployResult::Conflict(format!(
            "target is a directory (remove it manually): {}",
            target_path.display()
        )));
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

    // Create parent directories
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    match action.kind {
        crate::scanner::EntryKind::Template => {
            let content = rendered_content.unwrap_or("");
            std::fs::write(&target_path, content)
                .with_context(|| format!("failed to write template output: {}", target_path.display()))?;
        }
        crate::scanner::EntryKind::Base | crate::scanner::EntryKind::Override => {
            std::fs::copy(&action.source, &target_path)
                .with_context(|| format!("failed to copy {} to {}", action.source.display(), target_path.display()))?;
            copy_permissions(&action.source, &target_path)?;
        }
    }

    if was_existing {
        Ok(DeployResult::Updated)
    } else {
        Ok(DeployResult::Created)
    }
}

/// Parse an octal mode string (e.g. "755") and apply it to the file at `path`.
pub fn apply_permission_override(path: &Path, mode_str: &str) -> Result<()> {
    let mode = u32::from_str_radix(mode_str, 8)
        .with_context(|| format!("invalid octal permission string: '{mode_str}'"))?;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions {mode_str} on {}", path.display()))?;
    Ok(())
}

/// Copy the Unix file permissions from `source` to `dest`.
fn copy_permissions(source: &Path, dest: &Path) -> Result<()> {
    let metadata = std::fs::metadata(source)
        .with_context(|| format!("failed to read metadata from {}", source.display()))?;
    std::fs::set_permissions(dest, metadata.permissions())
        .with_context(|| format!("failed to set permissions on {}", dest.display()))?;
    Ok(())
}
```

- [ ] **Step 4: Run deployer tests**

Run: `cargo test --test deployer 2>&1 | tail -20`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/deployer.rs tests/deployer.rs && git commit -m "rewrite deployer: deploy_symlink replaces deploy_staged

New deploy_symlink() creates symlinks directly to source files.
deploy_copy() retained for templates and system-mode files.
Both functions now include is_dir() guard with descriptive error.
deploy_staged() removed entirely."
```

---

### Task 5: Orchestrator rewrite

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Remove staging_dir and PendingAction.strategy**

In `src/orchestrator.rs`:

Remove `staging_dir` from the `Orchestrator` struct and constructor:
```rust
pub struct Orchestrator {
    loader: ConfigLoader,
    target_dir: PathBuf,
    state_dir: Option<PathBuf>,
    system_mode: bool,
    package_filter: Option<String>,
}
```

Update `Orchestrator::new()` to remove `staging_dir`:
```rust
    pub fn new(dotfiles_dir: &Path, target_dir: &Path) -> Result<Self> {
        let loader = ConfigLoader::new(dotfiles_dir)?;
        Ok(Self {
            loader,
            target_dir: target_dir.to_path_buf(),
            state_dir: None,
            system_mode: false,
            package_filter: None,
        })
    }
```

Remove `get_pkg_strategy()` method.

Remove `strategy` from `PendingAction`:
```rust
struct PendingAction {
    pkg_name: String,
    action: scanner::FileAction,
    pkg_target: PathBuf,
    rendered: Option<String>,
    is_system: bool,
}
```

- [ ] **Step 2: Replace collision detection**

Replace the staging-path collision detection (Phase 2 in deploy, lines 203-217) with target-path collision detection:

```rust
        // Phase 2: Target-path collision detection
        let mut target_owners: HashMap<PathBuf, String> = HashMap::new();
        for p in &pending {
            let target_path = p.pkg_target.join(&p.action.target_rel_path);
            if let Some(existing) = target_owners.get(&target_path) {
                bail!(
                    "target collision -- packages '{}' and '{}' both deploy {}",
                    existing,
                    p.pkg_name,
                    target_path.display()
                );
            }
            target_owners.insert(target_path, p.pkg_name.clone());
        }
```

- [ ] **Step 3: Rewrite the deploy loop**

Replace Phase 4 of the `deploy()` method. The key changes:
- Use the orchestrator decision tree for target resolution
- Call `deploy_symlink()` for user-mode Base/Override
- Call `deploy_copy()` for templates and system-mode files
- Remove all staging references
- Drift detection only for copies (templates and system-mode)
- Pass `force=true` for managed re-deploys
- Record `staged: None` in new DeployEntry

The deploy loop should look like:

```rust
        for p in &pending {
            // Hook handling unchanged...

            let target_path = p.pkg_target.join(&p.action.target_rel_path);
            let is_user_mode_symlink = !p.is_system
                && (p.action.kind == scanner::EntryKind::Base
                    || p.action.kind == scanner::EntryKind::Override);

            // Orchestrator decision tree
            let is_managed = existing_targets.contains(&target_path);
            let effective_force = if is_managed { true } else { force };

            // Drift detection: only for copies (templates and system-mode)
            if !is_user_mode_symlink && target_path.exists() && !target_path.is_symlink() {
                if let Some(&expected_hash) = existing_hashes.get(&target_path) {
                    let current_hash = hash::hash_file(&target_path)?;
                    if current_hash != expected_hash && !force {
                        eprintln!(
                            "warning: {} has been modified since last deploy, skipping (use --force to overwrite)",
                            p.action.target_rel_path.display()
                        );
                        report.conflicts.push((target_path, "modified since last deploy".to_string()));
                        continue;
                    }
                }
            }

            // Backup pre-existing unmanaged files
            let (original_hash, original_owner, original_group, original_mode) =
                if !dry_run && !is_managed && target_path.exists() && !target_path.is_symlink() {
                    let content = std::fs::read(&target_path)?;
                    let hash = hash::hash_content(&content);
                    state.store_original(&hash, &content)?;
                    let (owner, group, mode) = metadata::read_file_metadata(&target_path)?;
                    (Some(hash), Some(owner), Some(group), Some(mode))
                } else {
                    (None, None, None, None)
                };

            // Deploy
            let result = if is_user_mode_symlink {
                deployer::deploy_symlink(&p.action, &p.pkg_target, dry_run, effective_force)?
            } else {
                deployer::deploy_copy(&p.action, &p.pkg_target, dry_run, effective_force, p.rendered.as_deref())?
            };

            match result {
                DeployResult::Created | DeployResult::Updated => {
                    let content_hash = if !dry_run {
                        hash::hash_file(if is_user_mode_symlink { &p.action.source } else { &target_path })?
                    } else {
                        String::new()
                    };

                    // Apply metadata for system-mode copies only
                    let resolved = if !dry_run && p.is_system {
                        if let Some(pkg_config) = self.loader.root().packages.get(&p.pkg_name) {
                            let rel_path_str = p.action.target_rel_path.to_str().unwrap_or("");
                            let resolved = metadata::resolve_metadata(pkg_config, rel_path_str);
                            if resolved.owner.is_some() || resolved.group.is_some() {
                                if let Err(e) = metadata::apply_ownership(
                                    &target_path,
                                    resolved.owner.as_deref(),
                                    resolved.group.as_deref(),
                                ) {
                                    eprintln!("warning: failed to set ownership on {}: {e}", target_path.display());
                                }
                            }
                            if let Some(ref mode) = resolved.mode {
                                deployer::apply_permission_override(&target_path, mode)?;
                            }
                            resolved
                        } else {
                            metadata::resolve_metadata(&crate::config::PackageConfig::default(), "")
                        }
                    } else {
                        metadata::resolve_metadata(&crate::config::PackageConfig::default(), "")
                    };

                    let abs_source = std::fs::canonicalize(&p.action.source)
                        .unwrap_or_else(|_| p.action.source.clone());

                    state.record(DeployEntry {
                        target: target_path.clone(),
                        staged: None,
                        source: abs_source,
                        content_hash,
                        original_hash,
                        kind: p.action.kind,
                        package: p.pkg_name.clone(),
                        owner: resolved.owner,
                        group: resolved.group,
                        mode: resolved.mode,
                        original_owner,
                        original_group,
                        original_mode,
                    });

                    if matches!(result, DeployResult::Updated) {
                        report.updated.push(target_path);
                    } else {
                        report.created.push(target_path);
                    }
                }
                DeployResult::Conflict(msg) => {
                    report.conflicts.push((target_path, msg));
                }
                DeployResult::DryRun => {
                    report.dry_run_actions.push(target_path);
                }
                _ => {}
            }
        }
```

- [ ] **Step 4: Update existing_hashes to key on target_path (not staged_path)**

In Phase 3 (state loading), change the hash map to key on target instead of staged:

```rust
        let existing_hashes: HashMap<PathBuf, &str> = existing_state
            .entries()
            .iter()
            .map(|e| (e.target.clone(), e.content_hash.as_str()))
            .collect();

        let existing_targets: std::collections::HashSet<PathBuf> = existing_state
            .entries()
            .iter()
            .map(|e| e.target.clone())
            .collect();
```

- [ ] **Step 5: Remove orphan staged cleanup and .gitignore warning**

In orphan detection (Phase 4.5), remove the staged cleanup:
```rust
                    if !dry_run && self.loader.root().dotm.auto_prune {
                        if old_entry.target.is_symlink() || old_entry.target.exists() {
                            let _ = std::fs::remove_file(&old_entry.target);
                            crate::state::cleanup_empty_parents(&old_entry.target);
                        }
                        report.pruned.push(old_entry.target.clone());
                    }
```

Remove the `.gitignore` warning block at the end of `deploy()` (lines 582-594).

- [ ] **Step 6: Verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: compiles (may have warnings)

- [ ] **Step 7: Update orchestrator test**

In `tests/orchestrator.rs`, update assertions to check symlink points to source:

```rust
#[test]
fn full_deploy_basic_fixture() {
    let fixture = std::path::PathBuf::from("tests/fixtures/basic");
    let target_dir = tempfile::TempDir::new().unwrap();
    let state_dir = tempfile::TempDir::new().unwrap();

    let mut orch = dotm::orchestrator::Orchestrator::new(&fixture, target_dir.path())
        .unwrap()
        .with_state_dir(state_dir.path());

    let report = orch.deploy("testhost", false, false).unwrap();
    assert!(!report.created.is_empty(), "should have created files");
    assert!(report.conflicts.is_empty(), "should have no conflicts");

    // Verify symlinks point to source in packages/, not to .staged/
    let bashrc = target_dir.path().join(".bashrc");
    assert!(bashrc.is_symlink(), ".bashrc should be a symlink");
    let link_dest = std::fs::read_link(&bashrc).unwrap();
    let dest_str = link_dest.to_string_lossy();
    assert!(
        dest_str.contains("packages/"),
        "symlink should point into packages/, got: {}",
        dest_str
    );
    assert!(
        !dest_str.contains(".staged"),
        "symlink must NOT point into .staged/, got: {}",
        dest_str
    );
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test --test orchestrator 2>&1`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/orchestrator.rs tests/orchestrator.rs && git commit -m "rewrite orchestrator for direct symlink deployment

- Remove staging_dir, effective_staging_dir, PendingAction.strategy
- Target-path collision detection replaces staging-path
- Orchestrator decision tree: managed re-deploy passes force=true
- Drift detection only for copies (templates, system-mode)
- Symlinks for user-mode Base/Override, copies for everything else
- Remove .gitignore warning for .staged/
- Remove orphan staged cleanup"
```

---

### Task 6: CLI changes — Diff rewrite, Prune update, scanner/list docs

**Files:**
- Modify: `src/main.rs` (Diff handler, Prune handler)
- Modify: `src/scanner.rs` (doc comments)

- [ ] **Step 1: Rewrite Diff handler**

Replace the `Commands::Diff` handler in `src/main.rs` (lines 395-436):

```rust
        Commands::Diff { path, system, host } => {
            let state_dir = if system {
                check_system_privileges();
                system_state_dir()
            } else {
                dotm_state_dir()
            };
            let state = dotm::state::DeployState::load(&state_dir)?;
            let mut found_diffs = false;

            // Try to load config for full diff support
            let config_context = (|| -> Option<(dotm::loader::ConfigLoader, toml::map::Map<String, toml::Value>)> {
                let loader = dotm::loader::ConfigLoader::new(&cli.dir).ok()?;
                let hostname = host.clone().or_else(|| {
                    hostname::get().ok().map(|h| h.to_string_lossy().to_string())
                })?;
                let host_config = loader.load_host(&hostname).ok()?;
                let mut merged_vars = toml::map::Map::new();
                for role_name in &host_config.roles {
                    if let Ok(role) = loader.load_role(role_name) {
                        merged_vars = dotm::vars::merge_vars(&merged_vars, &role.vars);
                    }
                }
                merged_vars = dotm::vars::merge_vars(&merged_vars, &host_config.vars);
                Some((loader, merged_vars))
            })();

            let has_config = config_context.is_some();
            if !has_config && !state.entries().is_empty() {
                eprintln!("warning: could not load dotfiles config; showing drift status only (no diff content)");
            }

            for entry in state.entries() {
                if let Some(ref filter) = path {
                    if !entry.target.to_str().unwrap_or("").contains(filter) {
                        continue;
                    }
                }

                // Skip user-mode symlinks (use git diff instead)
                if entry.target.is_symlink() {
                    continue;
                }

                let status = state.check_entry_status(entry);
                if !status.is_modified() {
                    continue;
                }

                found_diffs = true;

                if let Some((ref loader, ref vars)) = config_context {
                    // Full diff: re-render or read source, compare to target
                    let expected = if entry.kind == dotm::scanner::EntryKind::Template {
                        std::fs::read_to_string(&entry.source)
                            .ok()
                            .and_then(|tmpl| dotm::template::render_template(&tmpl, vars).ok())
                    } else {
                        std::fs::read_to_string(&entry.source).ok()
                    };

                    let current = std::fs::read_to_string(&entry.target).unwrap_or_default();

                    if let Some(expected) = expected {
                        let label_a = format!("expected: {}", entry.target.display());
                        let label_b = format!("current:  {}", entry.target.display());
                        print!("{}", dotm::diff::format_unified_diff(&expected, &current, &label_a, &label_b));
                    } else {
                        println!("  M {} (source unavailable)", entry.target.display());
                    }
                } else {
                    println!("  M {}", entry.target.display());
                }
            }

            if !found_diffs {
                println!("No modified files.");
            }
        }
```

Also add `host: Option<String>` to the Diff CLI subcommand:
```rust
    Diff {
        /// Only show diff for a specific file path
        path: Option<String>,
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },
```

- [ ] **Step 2: Update Prune handler**

In the `Commands::Prune` handler, remove the staged cleanup block (lines 824-827):

```rust
                        if entry.staged != entry.target && entry.staged.exists() {
                            let _ = std::fs::remove_file(&entry.staged);
                            dotm::state::cleanup_empty_parents(&entry.staged);
                        }
```

Replace with just target cleanup (which is already there at lines 820-823).

- [ ] **Step 3: Update scanner doc comments**

In `src/scanner.rs`, update `EntryKind` doc comments (lines 8-13):

```rust
pub enum EntryKind {
    /// Plain base file -- symlink (user-mode) or copy (system-mode)
    Base,
    /// Host or role override -- symlink (user-mode) or copy (system-mode)
    Override,
    /// Tera template -- rendered and written as a copy
    Template,
}
```

- [ ] **Step 4: Emit deprecation warnings in Check command**

In the `Commands::Check` handler in `src/main.rs`, after existing validation (around line 579), add:

```rust
            // Emit deprecation warnings for strategy field
            let dep_warnings = dotm::config::deprecated_strategy_warnings(loader.root());
            for w in &dep_warnings {
                eprintln!("{w}");
            }
```

- [ ] **Step 5: Verify compilation and run tests**

Run: `cargo build 2>&1 | head -20`
Expected: compiles

Run: `cargo test 2>&1 | tail -10`

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/scanner.rs && git commit -m "rewrite Diff handler, update Prune, scanner docs

- Diff handler loads config for template re-rendering, gains --host flag
- Diff skips symlink entries (use git diff)
- Diff falls back to drift-only mode when config unavailable
- Prune handler removes staged cleanup logic
- Scanner EntryKind doc comments updated for new deployment model
- Check command emits deprecation warnings for strategy field"
```

---

### Task 7: Test suite updates

**Files:**
- Modify: `tests/e2e.rs`
- Modify: `tests/system_packages.rs`
- Modify: `tests/cli.rs`
- Modify: `tests/copy_drift.rs`
- Modify: `tests/orphan.rs`

- [ ] **Step 1: Update e2e.rs**

Key changes:
- All symlink assertions should check that target points into `packages/`, not `.staged/`
- Remove `e2e_deploy_stages_all_files` test (staging no longer exists)
- Update `e2e_deploy_detects_modification` to modify the source file directly (for symlinks, editing the target IS editing the source)
- Update `e2e_permission_override_applied` — user-mode permissions are no longer applied to staged copies
- Update `e2e_collision_detection` — collision is on target paths now

For each test, update `DeployEntry` construction to use `staged: None`.

Update the `use_fixture` helper — no longer needs to skip `.staged/` directories.

- [ ] **Step 2: Update system_packages.rs**

- Remove `system_deploy_uses_separate_staging` test entirely (staging no longer exists)
- Remove `strategy = "copy"` / `strategy = "stage"` from fixture TOML configs
- Update remaining tests to verify system packages are deployed as copies, not symlinks
- Verify system-mode files are regular files (not symlinks) at the target path

- [ ] **Step 3: Update cli.rs**

- Update `copy_dir_recursive` helper — no longer needs to skip `.staged/` dirs
- Update `cli_status_no_state` test — override `HOME` instead of `XDG_STATE_HOME` for state isolation (since state now defaults to `~/.dotm/`)

- [ ] **Step 4: Update copy_drift.rs**

Update `DeployEntry` construction to use `staged: None`.

- [ ] **Step 5: Update orphan.rs**

Update `copy_dir_recursive` helper — no longer needs to skip `.staged/`.

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests PASS

Run: `cargo clippy -- -D warnings 2>&1`
Expected: no clippy warnings

- [ ] **Step 7: Commit**

```bash
git add tests/ && git commit -m "update test suite for direct symlink deployment model

- e2e tests assert symlinks point to packages/ source
- Remove staging-specific tests
- System package tests verify copy behavior without strategy field
- Update DeployEntry construction across all test files
- CLI tests use HOME override for state isolation"
```

---

### Task 8: Final cleanup and verification

- [ ] **Step 1: Clean up any remaining references to staging**

Run: `rg "staged\b|\.staged|deploy_staged|staging_dir|effective_staging" src/ tests/ --type rust`

Fix any remaining references. There should be none except the transitional `staged: Option<PathBuf>` field in `DeployEntry`.

- [ ] **Step 2: Clean up DeployStrategy references**

Run: `rg "DeployStrategy|get_pkg_strategy" src/ --type rust`

The only remaining reference should be the `DeployStrategy` enum definition in `config.rs` (kept for TOML parsing compat) and the `strategy` field on `PackageConfig`.

- [ ] **Step 3: Run full CI check**

Run: `just check`  (which runs `cargo test` + `cargo clippy -- -D warnings`)
Expected: PASS with no warnings

- [ ] **Step 4: Manual smoke test**

If you have a dotfiles repo available:

```bash
cargo run -- deploy --dry-run --host testhost
cargo run -- status
cargo run -- list packages --verbose
cargo run -- check
```

Verify:
- `list packages --verbose` does NOT show strategy
- `check` emits deprecation warning if strategy is set in config
- `deploy --dry-run` shows expected files
- No references to `.staged/` in any output

- [ ] **Step 5: Commit final cleanup**

```bash
git add -A && git commit -m "final cleanup: remove remaining staging references"
```
