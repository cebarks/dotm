# Changelog

## 1.0.0

Initial stable release.

### Deployment

- Symlink-based deployment for plain files, copy for overrides and templates
- Staging directory (`.staged/`) so symlinks point to a stable intermediate location
- Per-package target directories with environment variable expansion (`~`, `$XDG_CONFIG_HOME`, etc.)
- Dry-run mode (`--deploy --dry-run`) for all operations
- Force deploy (`--force`) to overwrite modified files
- `--package` filter to deploy/undeploy specific packages
- Idempotent deploys — re-running is a no-op when nothing changed

### Configuration

- Composable roles grouping packages (e.g. "desktop", "dev", "gaming")
- Host-specific configuration selecting roles and overriding variables
- Variable precedence: host vars > role vars (last listed role wins)
- Package dependencies with circular dependency detection
- Suggested packages (informational, not auto-deployed)

### File Overrides & Templates

- Host overrides (`file##host.<hostname>`) — highest priority, deployed as copy
- Role overrides (`file##role.<rolename>`) — deployed as copy
- Tera templates (`file.tera`) — rendered with merged variables
- Plain base files — deployed as symlink (lowest priority)

### System Packages

- System-level package deployment (`--system` flag, runs as root)
- Separate state tracking for system packages (`/var/lib/dotm/`)
- Per-package and per-file ownership/permissions control
- `preserve = true` option to keep existing file permissions

### State & Drift Detection

- Persistent state tracking (`$XDG_STATE_HOME/dotm/dotm-state.json`)
- SHA256-based drift detection for deployed files
- State versioning with automatic migration
- File locking to prevent concurrent state corruption

### Commands

- `deploy` / `undeploy` / `restore` — core deployment lifecycle
- `status` — deployment health with default/verbose/short modes and colored output
- `diff` — unified diffs for drifted files
- `adopt` — interactive hunk-by-hunk acceptance of external changes
- `check` — validate configuration (hosts, roles, packages, templates)
- `init` — scaffold a new package directory
- `add` — bring existing files under dotm management
- `list` — list packages/roles/hosts with optional tree view
- `prune` — remove orphaned files from previous deployments
- `completions` — generate shell completions for bash, zsh, and fish

### Git Integration

- `commit` — git-aware commit with auto-generated messages
- `push` / `pull` — remote sync with merge conflict detection
- `sync` — pull + deploy + push in one command
- Dirty file detection shown in `status` output

### Hooks

- Per-package pre/post deploy hooks
- Per-package pre/post undeploy hooks

### Orphan Detection

- Automatic detection of files from removed packages
- Optional auto-pruning during deploy
