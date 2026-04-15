# Direct Symlink Deployment Model

**Date**: 2026-04-14
**Status**: Approved

## Motivation

The current staging-based deployment model introduces unnecessary complexity in both the deploy and change adoption workflows. Files are copied to a `.staged/` directory, symlinked from the target to the staged copy, and backed up in content-addressed storage under the state directory. Detecting and adopting drift requires hashing staged copies, comparing against state records, and running an interactive hunk-by-hunk adoption flow.

This design replaces the staging model with direct symlinks to source files, eliminating the intermediate layer and leveraging git for diffing and change tracking.

## Design

### Deployment Model

Deployment mode is determined by file kind and system mode:

**User-mode packages:**

| Kind | Deployed as | Editable at target? | Diff via |
|------|------------|---------------------|----------|
| Base | Symlink -> source in `packages/` | Yes (edits go to repo) | `git diff` |
| Override | Symlink -> source in `packages/` | Yes (edits go to repo) | `git diff` |
| Template | Copy (rendered) to target | No (re-deploy to update) | `dotm diff` (re-render vs target) |

**Note on Override symlinks:** Override files have `##host.hostname` or `##role.rolename` suffixes in the source filename. The symlink target points to this suffixed source file (e.g., `~/.bashrc` -> `packages/shell/.bashrc##host.myhost`). This is a behavioral change from the current model where Overrides were copied to staging. With direct symlinks, editing `~/.bashrc` directly modifies the `##`-suffixed source file in the repo. This is intentional -- the override IS the source of truth for that host/role.

**System-mode packages** (`system = true`):

All files are copied to target, never symlinked. Symlinks from `/etc/` into a user-owned repo would be a security concern, and system files typically need specific ownership/permissions applied to the target.

| Kind | Deployed as | Diff via |
|------|------------|----------|
| Base | Copy to target | `dotm diff` (source vs target) |
| Override | Copy to target | `dotm diff` (source vs target) |
| Template | Copy (rendered) to target | `dotm diff` (re-render vs target) |

The `strategy` field in package config is removed. Deployment mode is determined by kind + system flag.

**User-mode metadata note:** The current model applies permission overrides (e.g., `permissions = { "bin/myscript" = "755" }`) to the staged copy, which takes effect via the symlink. With direct symlinks, metadata is NOT applied for user-mode packages -- the source file in the repo retains its natural permissions. Users relying on user-mode permission overrides must set permissions directly on the source file (e.g., `chmod 755 packages/bin/bin/myscript`). This is a behavioral change from the current model.

**Deploy pipeline:**
1. Scan packages, resolve overrides (unchanged from current)
2. Target-path collision detection: verify no two packages deploy to the same target path (replaces staging-path collision detection). This checks the fully-resolved target path (package target dir + relative path), so two packages deploying the same relative path to different target directories do not collide.
3. For each file:
   - If target already exists as a symlink (broken or pointing to a different source): remove it and recreate. This includes dotm-managed symlinks from a previous deploy that now point to a different source (e.g., after changing overrides). Pre-existing symlinks are replaced without backup (symlinks have no content of their own).
   - If target already exists as a directory: error (do not remove recursively; user must resolve manually)
   - If target already exists as a regular file and isn't managed by dotm: backup to `~/.dotm/originals/`, record original hash. Requires `--force` to proceed, otherwise return `Conflict`.
   - If target already exists as a regular file and IS managed by dotm (present in state): overwrite it (this is a re-deploy, not a conflict). This is important for `deploy_copy()` -- system-mode re-deploys and template re-deploys must not require `--force`. The orchestrator determines whether a file is managed by checking existing state, and passes `force=true` to the deployer for managed re-deploys. This keeps the deployer stateless.
   - User-mode Base/Override: create symlink from target -> canonicalized absolute path of source in packages/
   - System-mode Base/Override: copy source to target
   - Template (both modes): render via Tera + write to target
4. Save state

**Separation of concerns:** The deployer (`deploy_symlink()`, `deploy_copy()`) handles conflict detection (including explicit `is_dir()` guard with a descriptive error) and file operations, returning `DeployResult`. The orchestrator handles backup logic, state recording, and hook execution -- same separation as the current model.

**Orchestrator target-resolution decision tree** (evaluated before calling the deployer):
1. Is target in existing state? -> managed re-deploy: skip backup, pass `force=true` to deployer
2. Is target an unmanaged regular file? -> backup content to originals, pass user's `--force` value to deployer
3. Is target a symlink (any kind)? -> no backup needed, pass through to deployer (which removes and recreates)
4. Is target a directory? -> deployer returns error with clear message
5. Target does not exist? -> pass through to deployer normally

### State & Storage

All dotm runtime data lives under `~/.dotm/`:

- `~/.dotm/state.json` -- deployment records
- `~/.dotm/originals/{hash}` -- content-addressed backups of pre-dotm files

For system mode, state lives at `/var/lib/dotm/` (controlled by system_mode flag).

**Legacy state fallback:** On startup, if `~/.dotm/state.json` does not exist, check the legacy path at `$XDG_STATE_HOME/dotm/dotm-state.json`. If found, read state from the legacy location and print a one-time message: `"note: reading state from legacy location; run 'dotm deploy' to migrate to ~/.dotm/"`. This prevents silent data loss when users upgrade the binary without running a full migration.

**State version:** `CURRENT_VERSION` bumps from 2 to 3. The `staged` field is removed from `DeployEntry` and marked with `#[serde(default)]` during the transition so that v2 state files can still be deserialized by v3 code. On load:
- v3 state: loads normally, `staged` field absent
- v2 state: loads with `staged` populated from the existing field value (as `Some(PathBuf)`), auto-upgrades version to 3 on next save. Commands that only read state (status, diff) work without explicit migration. Commands that write state (deploy, undeploy) save in v3 format with `staged` omitted.
- v4+ state: rejected (existing behavior)

This means explicit migration is NOT required before using dotm -- the state format gracefully degrades. However, the full migration (moving state dir from XDG to `~/.dotm/`, converting staged symlinks to direct symlinks) requires running `dotm deploy` at least once.

**DeployEntry** (simplified):

```rust
struct DeployEntry {
    target: PathBuf,
    source: PathBuf,
    kind: EntryKind,       // Base, Override, Template
    package: String,
    content_hash: String,  // for templates and system-mode copies: hash of deployed content, used for drift detection. For user-mode symlinks: hash of source at deploy time, informational only (not used in drift detection since the symlink IS the source)
    original_hash: Option<String>,
    #[serde(default)]      // transitional: allows v2 state to deserialize
    staged: Option<PathBuf>,  // ignored in v3, present only for v2 compat deserialization
    // Metadata (kept for system dotfiles)
    owner: Option<String>,
    group: Option<String>,
    mode: Option<String>,
    original_owner: Option<String>,
    original_group: Option<String>,
    original_mode: Option<String>,
}
```

After the migration period (next major version), the `staged` field can be fully removed.

### Status Checking (`check_entry_status`)

The current `check_entry_status` hashes `entry.staged` to detect drift. The new implementation branches by deployment kind:

**User-mode symlinks (Base/Override):**
- Check that target exists and is a symlink
- Check that the symlink destination (via `std::fs::read_link`) matches `entry.source` after canonicalization of both paths (handles relative vs absolute, intermediate symlinks)
- If the symlink is missing or points elsewhere: report as missing/broken
- No content drift check needed -- the symlink and source are the same file

**Transitional v2 entries:** When `entry.staged` is `Some(path)` and the target is a symlink pointing to the staged path (not the source), this is a pre-migration entry. `check_entry_status` should fall back to v2 behavior: hash the staged file, compare against `content_hash`. This avoids false "broken" reports for users who upgraded but haven't re-deployed yet. Once the user runs `dotm deploy`, entries are rewritten in v3 format and this fallback path is no longer hit.

**Copies (Templates, and all system-mode files):**
- Check that target exists as a regular file
- Hash the target file content, compare against `entry.content_hash`
- If hash differs: report as content modified
- Metadata checks unchanged (owner/group/mode vs recorded values)

### Diff & Drift Detection

**`dotm diff` command rewrite:**

The current `dotm diff` loads deployed content from `state_dir/deployed/{hash}` and compares against the staged file. The new implementation must load the orchestrator and config to produce diffs:

1. Resolve hostname: use `--host` flag if provided, otherwise auto-detect via `gethostname()` (same as `deploy` command)
2. Load host config and merge vars (same as deploy pipeline steps)
3. For each state entry:
   - **User-mode symlinks**: skip (diffs are in `git diff`)
   - **System-mode copies (Base/Override)**: read source file, read target file, show unified diff
   - **Template copies**: re-render the template with current vars, read target file, show unified diff between re-rendered content and target content

This means `dotm diff` needs access to the dotfiles directory and host config, not just the state file. The `Diff` CLI command handler must instantiate the config loader and resolve vars. The `Diff` CLI command gains a `--host` flag (matching `deploy`'s existing flag).

**Error handling:** If the dotfiles directory or host config cannot be loaded (e.g., user runs `dotm diff` from an unrelated directory), `dotm diff` falls back to state-only mode: it can still report which entries have drifted (by hashing target files against `content_hash`) but cannot show the actual diff content for any file type (templates need vars for re-rendering, system-mode copies need the source file from the repo). In this case, it prints a warning and shows only the drift status. In fallback mode, symlink entries are detected by checking whether the target is a symlink at runtime (`target.is_symlink()`) rather than relying on config to determine user-mode vs system-mode. This works because user-mode Base/Override are always symlinks and system-mode entries are always regular files.

**Base/Override symlinks (user-mode):**
- No drift detection needed -- the symlink points to the source, they are the same file
- `dotm status` checks that the symlink exists and points to the correct source
- Diffing is handled by `git diff` in the dotfiles repo

**Template files and system-mode copies:**
- Drift detection: re-render template / read source, hash the result, compare against `content_hash` in state
- `dotm diff`: show unified diff between expected content and current target content
- If drifted, the user re-deploys (no adopt flow)

### Adopt

**Removed entirely.**

- Base/Override (user-mode): edits at the target ARE edits to the source. Nothing to adopt.
- Base/Override (system-mode): re-deploy to push source changes to target.
- Templates: can't reverse a Tera render. User edits the `.tera` source and re-deploys.

The `Adopt` CLI subcommand is removed. The status message referencing `dotm adopt` (in `main.rs`) is updated to point users to `dotm deploy` for re-syncing.

### Undeploy & Restore

**`dotm undeploy`:**
- Base/Override (user-mode): remove the symlink. If `original_hash` exists, restore content from `~/.dotm/originals/{hash}`.
- Copies (templates, system-mode): remove the copied file. Same restore logic.
- Clean up empty parent directories (unchanged).
- Remove entries from state.
- All `entry.staged` cleanup logic (the `if entry.staged != entry.target` pattern) is removed from `undeploy()`, `undeploy_package()`, and `restore()`.

**Orphan detection:**
- Compare previous state entries against current deploy set
- Orphaned targets get removed or warned about, depending on `auto_prune`
- Remove staged cleanup from orphan handling in `orchestrator.rs` (the `old_entry.staged` cleanup pattern)

### Metadata

Metadata tracking (owner/group/mode) is retained for system dotfiles. The resolve/apply flow in `metadata.rs` remains. For system-mode packages (all copies), metadata is applied to the target file. For user-mode symlinks, metadata is not applied (the source file in the repo retains its natural permissions).

### Git Workflow Implications

With direct symlinks, user edits to deployed base/override files are immediately visible in the dotfiles repo as modified files. This changes two workflows:

**`dotm sync` (pull + deploy + push):** Previously, edits to deployed files went to the staged copy and were invisible to git until `dotm adopt` pushed them back to the source. Now, edits are already in the repo, so `dotm sync` will push any local edits. This is the desired behavior -- no adopt step needed.

**`dotm commit`:** The auto-generated commit message from `git_repo.dirty_files()` will now include user edits to deployed configs. Previously, these edits were invisible to git. This is correct behavior.

**`dotm add`:** Unchanged. Moves a file into packages/ and tells the user to run `dotm deploy`. The symlink created by deploy now points directly to the source.

## What Gets Removed

- `.staged/` directory and all staging logic
- `deployer::deploy_staged()` function
- `state_dir/deployed/{hash}` content-addressed deployed backups (`deployed_dir()`, `store_deployed()`, `load_deployed()` methods on `DeployState`)
- `DeployStrategy` enum and `strategy` config field
- `adopt.rs` -- the entire interactive hunk-by-hunk adoption module
- `Adopt` CLI subcommand and its handler in `main.rs`
- The `.gitignore` warning for `.staged/` (in `orchestrator.rs` deploy method)
- Collision detection for staging paths (replaced by target-path collision detection)
- The `staged` field on `DeployEntry` (kept as `Option` with `serde(default)` during transition)
- `staging_dir` field on `Orchestrator` struct, `effective_staging_dir` logic, and staging-related constructor code
- `PendingAction.strategy` field in `orchestrator.rs` (replaced by checking system_mode + kind)
- All `entry.staged` cleanup patterns in `restore()`, `undeploy()`, `undeploy_package()`, orphan handling, and `prune` command

## What Changes

- `deployer.rs` -- simplifies to `deploy_symlink()` and `deploy_copy()` (for templates and system-mode files)
- `orchestrator.rs` -- simpler deploy loop, no staging dir management, drift detection only for copies, target-path collision detection replaces staging-path collision detection, remove `.gitignore` warning, remove `staging_dir` from constructor and struct, replace `PendingAction.strategy` with system-mode check
- `state.rs` -- `~/.dotm/` as default state home, slimmer `DeployEntry`, remove `deployed/` storage, rewrite `check_entry_status` to branch on symlink vs copy, remove `entry.staged` cleanup from restore/undeploy/undeploy_package, bump `CURRENT_VERSION` to 3
- `main.rs` -- state dir resolution changes from XDG_STATE_HOME to `~/.dotm/` with legacy fallback, `Diff` handler rewritten to use orchestrator/config for re-rendering (gains `--host` flag), `Adopt` subcommand removed, status message updated to reference `dotm deploy` instead of `dotm adopt`, `Prune` handler updated to remove staged cleanup logic (prune-then-redeploy pattern preserved)
- `config.rs` -- remove `DeployStrategy` enum, keep `strategy` as `Option<DeployStrategy>` with `#[serde(default)]` temporarily so existing `dotm.toml` files with `strategy = "copy"` still parse without error (field is ignored), update `validate_system_packages()` to drop the strategy requirement. Emit a deprecation warning if `strategy` is set on any package: `"warning: 'strategy' field on package '{name}' is deprecated and ignored; deployment mode is now determined automatically"`. The `strategy` field can be fully removed in the next major version.
- `list.rs` -- remove strategy display from verbose package listing
- `scanner.rs` -- update `EntryKind::Override` doc comment (no longer "deployed as a copy"), update `EntryKind::Base` doc comment to note system-mode copies it as well
- Test suite -- significant updates:
  - `tests/orchestrator.rs` -- assert symlink-to-source (not symlink-to-staged)
  - `tests/e2e.rs` -- remove `.staged` assertions, update drift detection tests, update permission override tests (user-mode permissions no longer applied)
  - `tests/deployer.rs` -- rewrite to test `deploy_symlink()` and updated `deploy_copy()`
  - `tests/system_packages.rs` -- remove staging directory assertions, test system-mode copy behavior
  - `tests/cli.rs` -- override `HOME` instead of `XDG_STATE_HOME` for state isolation

## What Stays Unchanged

- `scanner.rs` -- file discovery, override resolution logic, `EntryKind` enum values
- `template.rs` -- Tera rendering
- `hooks.rs` -- pre/post deploy/undeploy hooks
- `metadata.rs` -- ownership/permissions resolution and application
- `git.rs` -- git operations
- `diff.rs` -- unified diff formatting (used for template and system-mode diffs)
- `resolver.rs` -- dependency resolution
- `loader.rs` -- config loading
- `vars.rs` -- variable merging
- `hash.rs` -- SHA-256 hashing
- `dotm add` command -- unchanged behavior
- State locking, restore from originals
- Binary file handling -- unaffected (symlinks are content-agnostic, copies work as before)

## State Migration

Existing deployments need migration from v2 state to v3. Migration requires access to the current `dotm.toml` to determine which packages are system packages.

**Graceful degradation:** The v3 binary can read v2 state files without explicit migration (via `#[serde(default)]` on the removed `staged` field). Read-only commands (`status`, `diff`, `list`) work immediately. Write commands (`deploy`, `undeploy`) save in v3 format, effectively auto-migrating the state file.

**Full migration** (converts staging symlinks to direct symlinks and moves state dir):

1. If `~/.dotm/` already exists, warn the user and abort unless `--force` is passed
2. Load v2 state from `$XDG_STATE_HOME/dotm/` (current location)
3. Load `dotm.toml` to determine system packages
4. Create `~/.dotm/` directory
5. For each entry, first check if the package is system-mode (takes precedence):
   - **System-mode** (any kind): remove old staged copy (if separate from target), remove old symlink, copy source to target. If the entry is a Template, re-render before copying.
   - **User-mode, Base or Override**: remove the staged copy, remove the old symlink, create new symlink directly to source
   - **User-mode, Template**: remove the staged copy, remove the old symlink, re-render template and write to target
6. Extract pre-dotm originals from old state dir:
   - Collect all `original_hash` values from state entries
   - Copy those specific blobs from `deployed/` (or `originals/` if it still exists from v1) into `~/.dotm/originals/`
   - Ignore all other blobs in `deployed/` (these are deploy-time content snapshots, no longer needed)
   - Note: the v1->v2 migration renamed `originals/` to `deployed/`, so in a v2 state dir, pre-dotm backups are mixed into `deployed/` alongside deploy snapshots. The `original_hash` field on each entry is the key to distinguishing them.
7. Discard `deployed/` directory from old state dir (no longer needed)
8. Write new state format (v3) to `~/.dotm/state.json`
9. Clean up old `.staged/` directory
10. Remove old state dir if empty

In practice, the simplest migration path is just running `dotm deploy --force` which will re-deploy everything with the new model and save v3 state to `~/.dotm/`.
