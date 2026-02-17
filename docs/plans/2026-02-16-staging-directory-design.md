# Staging Directory Design

## Problem

Currently, dotm handles files three ways:
- **Base files** are symlinked directly from `~/target` to `packages/<pkg>/file`
- **Overrides** (`##host`, `##role`) are copied directly to `~/target`
- **Templates** (`.tera`) are rendered and written directly to `~/target`

Copies and rendered files are disconnected from the dotfiles repo. When apps modify these files, changes are lost on re-deploy. There's no way to detect drift or adopt changes back into the repo.

## Solution

Introduce a **staging directory** (`~/dotfiles/.staged/`) as the single source for all deployed files. All files — base, override, and template — are staged into `.staged/` as real files, then the target directory contains only symlinks into `.staged/`.

Add commands for **drift detection** (`status`, `diff`) and **interactive adoption** (`adopt`) to manage changes apps make to deployed files.

## Architecture

### Deployment Flow

```
source (packages/)  -->  stage to .staged/  -->  symlink from ~/target
                           (real files)           (symlinks always)
```

- **Base files**: copied from `packages/<pkg>/file` into `.staged/<rel-path>`
- **Overrides** (`##host`, `##role`): copied from the override source into `.staged/<rel-path>`
- **Templates** (`.tera`): rendered and written to `.staged/<rel-path>`
- **Target**: `~/file` is always a symlink to `~/dotfiles/.staged/<rel-path>`

`.staged/` is gitignored and contains only real files — a complete, inspectable snapshot of the deployed state.

### Staging Directory Layout

Flat global layout mirroring target-relative paths:

```
~/dotfiles/.staged/
  .bashrc
  .config/
    app.conf
    nvim/
      init.lua
    wireplumber/
      main.lua.d/
        foo.lua
  bin/
    myscript
```

### Collision Detection

Before deploying, the orchestrator collects all `(staging_path, package_name)` tuples across all packages. If two packages produce the same staging path, it's a hard error:

```
error: staging collision -- packages 'gaming' and 'desktop' both deploy .config/app.conf
```

This is a configuration bug and should fail loudly.

### Per-Package Deploy Strategy

A new optional `strategy` field in `dotm.toml`:

```toml
[packages.system]
description = "System-level configs"
target = "/"
strategy = "copy"    # default is "stage"
```

- `"stage"` (default): files go through `.staged/` and get symlinked from the target
- `"copy"`: files are copied directly to the target (current behavior, useful for system packages where symlinks cause issues)

### File Permissions

**Default behavior**: When staging a file, preserve the source file's permission bits. After writing/copying the staged file, call `set_permissions()` to mirror the source mode.

**Config override**: An optional `permissions` table per package in `dotm.toml`:

```toml
[packages.bin]
description = "Personal scripts"

[packages.bin.permissions]
"bin/myscript" = "755"
"bin/helper" = "700"
```

Paths are relative to the package directory. Config overrides take precedence over source permissions.

## State Tracking

Revised `DeployState` structure:

```rust
struct DeployState {
    entries: Vec<DeployEntry>,
}

struct DeployEntry {
    /// Path in the target dir (e.g., ~/.config/app.conf)
    target: PathBuf,
    /// Path in .staged/ (e.g., ~/dotfiles/.staged/.config/app.conf)
    staged: PathBuf,
    /// Original source in packages/ (for adopt to know where to write back)
    source: PathBuf,
    /// SHA-256 hash of content at deploy time (for drift detection)
    content_hash: String,
    /// How the staged file was produced
    kind: EntryKind, // Base, Override, Template
    /// Package name this entry belongs to
    package: String,
}
```

The `content_hash` is computed at deploy time and stored. Drift detection compares the current `.staged/` file hash against the stored hash.

### Undeploy

Undeploy removes both the target symlink and the staged file for each entry, then cleans up empty directories.

## Commands

### `dotm deploy` (modified)

Deployment with drift-aware staging:

1. Scan packages, resolve overrides, collect staging paths
2. Check for staging collisions (hard error if found)
3. For each file action:
   a. Generate the expected content (copy source / render template)
   b. If staged file already exists, compare current content hash against stored hash
   c. If hashes match (or file is new): stage the file, create target symlink
   d. If hashes differ and `--force` not set: warn and skip
   e. If `--force`: overwrite regardless
4. Save state with content hashes

### `dotm status` (extended)

Shows all managed files with state indicators:

```
Managed files:
  [ok]       ~/.config/app.conf
  [ok]       ~/.bashrc
  [MODIFIED] ~/.config/wireplumber/main.lua.d/foo.lua
  [MISSING]  ~/.config/old-thing.conf

3 managed, 1 modified, 1 missing.
Run 'dotm diff' to see changes, 'dotm adopt' to review and accept.
```

Exit code: `0` if all files are clean, `1` if any are modified or missing.

### `dotm diff` (new)

Shows unified diffs for all drifted files — the diff between the content at deploy time and the current staged file content:

```
--- deployed: .config/wireplumber/main.lua.d/foo.lua
+++ current:  .config/wireplumber/main.lua.d/foo.lua
@@ -3,4 +3,5 @@
 some_setting = true
-old_value = 1
+old_value = 2
+new_line_added = true
```

Optionally filter: `dotm diff <path>`.

### `dotm adopt` (new)

Interactive hunk-level adoption of drifted changes back into source files:

1. Load state, identify drifted files (hash mismatch)
2. For each drifted file, compute the diff using a Rust diff library (e.g., `similar` or `diffy`)
3. Present hunks interactively, user accepts/rejects each hunk
4. For accepted hunks on Base or Override files: apply the patch to the source file in `packages/`
5. For Template-sourced files: warn that auto-adopt isn't possible (rendered output can't be reverse-mapped to Tera source), show diff for manual editing
6. Re-stage adopted files, update `content_hash` in state

## Config Changes

### `dotm.toml` additions

```toml
[packages.system]
description = "System-level configs"
target = "/"
strategy = "copy"              # "stage" (default) or "copy"

[packages.bin]
description = "Personal scripts"

[packages.bin.permissions]     # optional permission overrides
"bin/myscript" = "755"
```

### `.gitignore` addition

```
.staged/
```

## Migration

Existing deployments use the old state format (separate `symlinks` and `copies` lists). On first deploy after this change:

1. If old-format state is detected, undeploy using the old logic (remove symlinks and copies)
2. Re-deploy with the new staging system
3. Save new-format state

Alternatively, require `dotm undeploy && dotm deploy` as a manual migration step.
