# Specification: dotm setup Command

**Status**: Draft
**Created**: 2026-03-31
**Author**: Anten Skrabec

## Overview

The `setup` command provides a mechanism for running one-time or occasional package-specific initialization tasks. Unlike `deploy` (which manages config files via symlinks/copies), `setup` handles imperative operations like installing dependencies, running system configuration commands, or performing initial setup tasks.

## Motivation

### Current Limitations

dotm currently manages files (symlinks, templates, copies) but cannot handle:
- Package manager operations (`brew bundle`, `apt install`, `cargo install`)
- System preference commands (`defaults write` on macOS, `gsettings` on Linux)
- One-time initialization scripts (database setup, key generation)
- Dependency installation (npm packages, Python requirements)

### Why Not Hooks?

Post-deploy hooks were considered but are inappropriate because:
1. **Frequency**: Hooks run on every deploy; setup tasks should run once (or when changed)
2. **Semantics**: Hooks are for file deployment side effects; setup is for package initialization
3. **State tracking**: Setup needs to track execution state; hooks do not
4. **Idempotency**: Setup scripts can be expensive and should avoid unnecessary re-runs

## Goals

1. **Explicit separation**: `deploy` manages files, `setup` runs initialization tasks
2. **Idempotency**: Track execution state to avoid redundant runs
3. **Change detection**: Re-run setup when script content changes
4. **Dependency ordering**: Respect package dependencies for setup execution order
5. **Dry-run support**: Show what would be executed without running
6. **State visibility**: Users can see what has/hasn't been set up

## Non-Goals

- **Teardown/cleanup**: Not included in initial implementation (tracked separately in issue)
- **Continuous execution**: Setup is for one-time/occasional tasks, not recurring operations
- **Complex orchestration**: No conditionals, retries, or complex workflow logic
- **Cross-platform abstraction**: Scripts are responsible for platform detection

## Configuration Schema

### Package-Level Setup Configuration

```toml
[packages.homebrew]
description = "Homebrew package manager"
setup = "brew bundle --file=~/.Brewfile --no-lock"
setup_shell = "zsh"  # Optional, defaults to "sh"

[packages.macos-defaults]
description = "macOS system preferences"
setup = "scripts/apply-defaults.sh"

[packages.dev-tools]
description = "Development tools and dependencies"
setup = "pip install -r requirements.txt"
setup_after = ["homebrew"]  # Run after these packages' setup
```

### Field Definitions

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `setup` | string | No | - | Shell command or script path to execute |
| `setup_shell` | string | No | `"sh"` | Shell to use for execution (`sh`, `bash`, `zsh`, or full path) |
| `setup_after` | array[string] | No | `[]` | Package names whose setup must run before this one |

### Path Resolution

- Relative script paths (e.g., `"scripts/apply.sh"`) are resolved relative to the package directory
- Paths support shell expansion via `shellexpand::full()` (`~`, `$VAR`, `${VAR}`)
- Commands can be inline (`"brew bundle"`) or script references (`"scripts/setup.sh"`)

### Shell Selection

The `setup_shell` field determines execution environment:

```toml
setup_shell = "sh"           # Default: POSIX shell
setup_shell = "bash"         # Bash shell
setup_shell = "zsh"          # Zsh shell
setup_shell = "/bin/zsh -l"  # Full path with flags (login shell)
```

**Execution model**: The `setup_shell` string is split on whitespace. The first token is the binary, remaining tokens are prepended as flags before `-c`:

```
setup_shell = "zsh"          → zsh -c '<setup_command>'
setup_shell = "/bin/zsh -l"  → /bin/zsh -l -c '<setup_command>'
```

**Environment variables set**:
- `DOTM_PACKAGE` - Package name
- `DOTM_SETUP_ROOT` - Package target directory (expanded)
- `DOTM_PACKAGES_DIR` - Base packages directory
- `HOME` - User home directory (inherited)
- `PATH` - Inherited from parent process

**Working directory**: Package directory (e.g., `~/dotfiles/packages/homebrew/`)

## Command-Line Interface

### Basic Usage

```bash
# Run all setup tasks for current host's packages
dotm setup

# Run setup for specific package(s)
dotm setup --package homebrew
# --package accepts one package per invocation (matches deploy --package);
# run the command again for additional packages.

# Dry run (show what would execute)
dotm setup --dry-run

# Force re-run even if already executed successfully
dotm setup --force

# List available setup tasks and their status
dotm setup --list

# Target specific host configuration
dotm setup --host dev-server

# System package setup (requires root)
sudo dotm setup --system
```

### Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--package <NAME>` | `-p` | Run setup only for specified package(s). Can be repeated. If omitted, runs all packages. |
| `--dry-run` | - | Show what would be executed without running commands |
| `--force` | `-f` | Re-run setup even if already executed successfully |
| `--list` | `-l` | List all packages with setup tasks and their status |
| `--host <NAME>` | - | Target specific host (defaults to current hostname) |
| `--system` | - | Run system package setup (requires appropriate privileges) |
| `--verbose` | `-v` | Show detailed execution output |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All setup tasks succeeded |
| 1 | One or more setup tasks failed |
| 2 | Invalid configuration or arguments |

## State Management

### State Storage

Setup execution state is stored separately from deploy state:

| Context | State File |
|---------|-----------|
| User packages | `$XDG_STATE_HOME/dotm/setup-state.json` (typically `~/.local/state/dotm/setup-state.json`) |
| System packages | `/var/lib/dotm/setup-state.json` |

Follows the same pattern as `DeployState` in `state.rs`.

### State Schema

```json
{
  "version": 1,
  "entries": {
    "homebrew": {
      "last_run": "2026-03-31T12:34:56Z",
      "script_hash": "abc123def456...",
      "status": "success",
      "exit_code": 0,
      "duration_ms": 1523
    },
    "macos-defaults": {
      "last_run": "2026-03-31T12:35:10Z",
      "script_hash": "789ghi012jkl...",
      "status": "success",
      "exit_code": 0,
      "duration_ms": 234
    },
    "dev-tools": {
      "last_run": "2026-03-31T12:36:00Z",
      "script_hash": "mno345pqr678...",
      "status": "failed",
      "exit_code": 1,
      "duration_ms": 5021,
      "error": "Command 'pip install -r requirements.txt' failed with exit code 1"
    }
  }
}
```

### Rust Types

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const SETUP_STATE_FILE: &str = "setup-state.json";
const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct SetupState {
    #[serde(default)]
    version: u32,
    #[serde(skip)]
    state_dir: PathBuf,
    entries: HashMap<String, SetupEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupEntry {
    pub last_run: String,        // ISO 8601 timestamp
    pub script_hash: String,      // SHA256 hex
    pub status: SetupStatus,
    pub exit_code: i32,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupStatus {
    Success,
    Failed,
    Skipped,
}
```

**Note on `setup_shell`**: Use `Option<String>` in `PackageConfig` (not bare `String`
with serde default) to avoid conflicting with `#[derive(Default)]` which would produce
an empty string. Default to `"sh"` at the execution site instead.

### Script Hash Computation

Uses SHA256 via `hash` module (same as `DeployState`):

**For inline commands**: `hash::hash_content(setup_command.as_bytes())`

**For script files**: `hash::hash_file(script_path)`

**Hash mismatch behavior**: If stored hash differs from current, treat as if never run (even without `--force`).

## Execution Model

### Execution Flow

```
1. Load host config → resolve roles → resolve packages (via resolver::resolve_packages)
2. Filter packages to those with `setup` field
3. Apply --package filter if specified
4. Build dependency graph using setup_after + package depends
5. Topological sort to determine execution order
6. For each package in order:
   a. Compute current script hash
   b. Load state for package
   c. Decide: skip, run, or force-run
   d. Execute if needed (unless --dry-run)
   e. Update state
   f. Stop on first failure
```

### Execution Decision Logic

```rust
fn should_run_setup(
    package: &str,
    state: &SetupState,
    config: &PackageConfig,
    force: bool,
) -> anyhow::Result<(bool, &'static str)> {
    if force {
        return Ok((true, "forced re-run"));
    }

    let current_hash = compute_setup_hash(package, config)?;

    match state.entries.get(package) {
        None => Ok((true, "never run")),
        Some(entry) => {
            if entry.script_hash != current_hash {
                Ok((true, "script changed"))
            } else if entry.status == SetupStatus::Failed {
                Ok((true, "previous run failed"))
            } else {
                Ok((false, "already run successfully"))
            }
        }
    }
}
```

### Dependency Resolution

Setup respects two types of dependencies:

1. **Package dependencies** (`depends` field) - Use existing `resolver::resolve_packages()`
2. **Setup dependencies** (`setup_after` field) - Additional ordering constraints

**Algorithm**:
```rust
// 1. Get base dependency order from resolver::resolve_packages()
let base_order = resolver::resolve_packages(root, packages)?;

// 2. Build adjacency list with setup_after constraints
let mut graph: HashMap<String, Vec<String>> = ...;
for pkg in packages {
    if let Some(config) = root.packages.get(pkg) {
        for after in &config.setup_after {
            graph.entry(pkg.clone()).or_default().push(after.clone());
        }
    }
}

// 3. Topological sort respecting both base_order and setup_after
let final_order = topological_sort(base_order, graph)?;
```

**Circular dependency detection**: Use same pattern as `resolver.rs` (track stack, bail with "circular dependency detected: A -> B -> A").

**Missing dependency handling**: `setup_after` references are validated against `root.packages`
(the package must *exist*), not against the filtered set of packages that have `setup` fields.
If a referenced package exists but has no `setup` task, the constraint is a no-op — it simply
doesn't appear in the ordering graph. Only error if the package doesn't exist at all.

### Error Handling

**Setup script failure**:
```rust
bail!("Setup failed for package '{package}': command '{cmd}' exited with code {code}");
```
- Stop processing remaining packages
- Exit with code 1
- Leave partial state (successful setups are recorded)

**Script not found**:
```rust
bail!("Setup script not found for package '{package}': {path}");
```

**Invalid shell**:
```rust
bail!("Invalid setup_shell for package '{package}': shell '{shell}' not found or not executable");
```

## Terminal Output

Uses `crossterm` for colored output (respects `NO_COLOR`). Pattern similar to `status.rs` and `orchestrator.rs` deploy output.

### Standard Output

```
Setup: homebrew
  Command: brew bundle --file=~/.Brewfile --no-lock
  Shell: zsh
  Status: Running...
  ✓ Success (1.5s)

Setup: macos-defaults
  Command: scripts/apply-defaults.sh
  Shell: sh
  Status: Running...
  ✓ Success (0.2s)

Setup: dev-tools
  Command: pip install -r requirements.txt
  Shell: sh
  Status: Running...
  ✗ Failed (exit code 1)

Error: Setup failed for package 'dev-tools'
```

Colors:
- Package name: Cyan (like package names in deploy output)
- ✓ Success: Green
- ✗ Failed: Red
- Field labels: Blue

### Dry Run Output

```
Setup (dry run): homebrew
  Would execute: brew bundle --file=~/.Brewfile --no-lock
  Shell: zsh
  Reason: Never run

Setup (dry run): macos-defaults
  Would execute: scripts/apply-defaults.sh
  Shell: sh
  Reason: Script changed (hash mismatch)

Setup (dry run): postgres
  Would skip: Already run successfully
```

### List Output

```
$ dotm setup --list

Available setup tasks:

homebrew
  Command: brew bundle --file=~/.Brewfile --no-lock
  Status: ✓ Success (last run: 2026-03-31 12:34:56)

macos-defaults
  Command: scripts/apply-defaults.sh
  Status: ⚠ Changed (script modified since last run)

dev-tools
  Command: pip install -r requirements.txt
  Status: ✗ Failed (last run: 2026-03-31 12:36:00)
  Error: Command exited with code 1

postgres
  Command: scripts/init-db.sh
  Status: ○ Not run
```

## Integration Points

### Integration with deploy

The `setup` and `deploy` commands are **independent**:

```bash
# User must explicitly run both
dotm setup
dotm deploy
```

**No automatic triggering**: `deploy` never runs setup, `setup` never runs deploy.

**State isolation**: Setup state and deploy state are completely separate files.

### Integration with check

The `dotm check` command validates setup configuration:

```bash
dotm check
```

**Validations** (add to existing `config::validate_system_packages`):
- `setup_after` references valid packages in `root.packages`
- `setup_shell` (if full path) is executable
- Script files referenced in `setup` exist (if relative path)
- No circular dependencies in `setup_after` + `depends`

### Integration with list

```bash
dotm list packages --verbose
```

Add setup information to package details:
```
homebrew
  Description: Homebrew package manager
  Strategy: copy
  Setup: brew bundle --file=~/.Brewfile --no-lock
  Setup status: ✓ Success (2026-03-31 12:34:56)
```

### Integration with sync (future)

Future enhancement to support:
```bash
dotm sync --setup  # pull → setup → deploy → push
```

## Validation Rules

Add to `config::validate_system_packages()` or create new `config::validate_setup()`:

| Rule | Error Message |
|------|---------------|
| `setup_after` references unknown package | `Package '{pkg}' setup_after unknown package '{ref}'` |
| `setup_after` creates circular dependency | `Circular setup dependency detected: {cycle}` |
| `setup` field is empty string | `Package '{pkg}': setup field cannot be empty` |
| `setup_shell` is full path but not executable | `Package '{pkg}': setup_shell '{shell}' is not executable` |

Script existence validation happens at runtime (since scripts may be created during another package's setup).

## Testing Strategy

### Unit Tests (in new `tests/setup.rs`)

```rust
#[test]
fn should_run_when_never_run() { ... }

#[test]
fn should_skip_when_already_success() { ... }

#[test]
fn should_run_when_script_changed() { ... }

#[test]
fn should_run_when_previous_failed() { ... }

#[test]
fn force_flag_overrides_skip() { ... }

#[test]
fn setup_after_ordering() { ... }

#[test]
fn circular_setup_after_errors() { ... }

#[test]
fn unknown_setup_after_errors() { ... }
```

### Integration Tests (in `tests/e2e.rs` or new file)

```rust
#[test]
fn setup_basic_execution() {
    // Create fixture with package containing setup script
    // Run dotm setup
    // Verify script executed
    // Verify state saved
}

#[test]
fn setup_dry_run() {
    // Run with --dry-run
    // Verify script NOT executed
    // Verify no state saved
}

#[test]
fn setup_respects_dependencies() {
    // Create packages A (depends on B), B (setup_after C), C
    // Run setup
    // Verify execution order: C, B, A
}

#[test]
fn setup_failure_stops_execution() {
    // Create packages A, B (fails), C
    // Run setup
    // Verify A runs, B fails, C skipped
    // Verify state contains A success, B failure
}

#[test]
fn setup_hash_change_detection() {
    // Run setup, verify success
    // Modify script
    // Run setup again
    // Verify re-executes
}
```

### Manual Testing Scenarios

- [ ] Setup with Homebrew Brewfile
- [ ] macOS defaults write commands
- [ ] Setup script with login shell (`setup_shell = "/bin/zsh -l"`)
- [ ] Setup with missing dependencies
- [ ] Setup failure recovery (fix and re-run)
- [ ] System package setup with sudo

## Implementation Checklist

### Config Changes
- [ ] Add `setup: Option<String>` to `PackageConfig` in `config.rs`
- [ ] Add `setup_shell: Option<String>` to `PackageConfig` in `config.rs` (default to `"sh"` at execution site, not via serde default — avoids conflict with `#[derive(Default)]`)
- [ ] Add `setup_after: Vec<String>` to `PackageConfig` in `config.rs`

### New Modules
- [ ] Create `src/setup_state.rs` with `SetupState`, `SetupEntry`, `SetupStatus`
- [ ] Create `src/setup.rs` with `SetupOrchestrator` and execution logic

### CLI Changes
- [ ] Add `Setup` variant to `Commands` enum in `main.rs`
- [ ] Implement setup command handler in `main.rs`

### Core Logic
- [ ] Implement dependency graph builder with `setup_after` support
- [ ] Implement script hash computation (inline vs file)
- [ ] Implement state load/save
- [ ] Implement execution decision logic
- [ ] Implement shell execution (pattern from `hooks::run_hook`)
- [ ] Implement dry-run mode
- [ ] Implement list mode
- [ ] Implement colored terminal output

### Validation
- [ ] Add setup validation to `dotm check` command
- [ ] Validate `setup_after` references
- [ ] Validate setup dependency cycles
- [ ] Validate `setup_shell` executability (if full path)

### Integration
- [ ] Add setup info to `dotm list packages --verbose`

### Testing
- [ ] Write unit tests in `tests/setup.rs`
- [ ] Write integration tests in `tests/e2e.rs`
- [ ] Add test fixtures in `tests/fixtures/setup/`

### Documentation
- [ ] Update README.md with setup documentation
- [ ] Add examples to repository (Homebrew, macOS defaults, etc.)
- [ ] Update CHANGELOG.md

## Future Enhancements

The following are explicitly out of scope for initial implementation:

1. **Teardown/cleanup** (tracked in GitHub issue #5)
   - `teardown` field for undeploy operations
   - State tracking for resources created by setup

2. **Interactive setup**
   - Prompts for user input during setup
   - Confirmation before expensive operations

3. **Parallel execution**
   - Run independent setup tasks concurrently
   - Respect dependency ordering

4. **Retry logic**
   - Automatic retry on transient failures
   - Backoff strategies

5. **Setup logging**
   - Capture stdout/stderr to files
   - Per-package log files in state directory

6. **Conditional setup**
   - Platform-specific setup commands (use platform filtering feature instead)
   - Environment-based conditionals

7. **Integration with sync**
   - `dotm sync --setup` to run setup automatically

## Open Questions

1. ~~Should setup run on first deploy?~~
   - **Decision**: No automatic triggering. User runs `dotm setup` explicitly.

2. ~~Should failed setups block deploy?~~
   - **Decision**: No, setup and deploy are independent.

3. ~~Should we support setup "profiles"?~~
   - **Decision**: Not for initial version, use `--package` filtering.

4. ~~Should state track script content or just hash?~~
   - **Decision**: Hash only (smaller state file, privacy).

## References

- Existing hooks implementation: `src/hooks.rs`
- State management pattern: `src/state.rs`
- Package dependency resolution: `src/resolver.rs`
- Shell expansion: `orchestrator::expand_path()` using `shellexpand` crate
- Terminal output: `status.rs` colored output pattern
