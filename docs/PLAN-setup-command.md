# Implementation Plan: dotm setup Command

This document provides step-by-step implementation guidance for the `dotm setup` command as specified in `SPEC-setup-command.md`.

## Prerequisites

- Read `SPEC-setup-command.md` thoroughly
- Familiarity with dotm codebase (see `CLAUDE.md`)
- Rust 1.87+ with edition 2024 features
- Development environment set up (`just test` works)

## Implementation Order

We'll implement in this order to maintain working state at each step:

1. **Config schema** - Add fields, no behavior change
2. **State management** - Create state module, no integration
3. **Core execution** - Setup orchestrator logic
4. **CLI integration** - Wire up command handler
5. **Validation** - Add to `check` command
6. **List integration** - Show setup info in `list` command
7. **Testing** - Unit and integration tests
8. **Documentation** - Update README

Each step should compile and pass existing tests before moving to the next.

---

## Step 1: Config Schema

**Goal**: Add `setup`, `setup_shell`, `setup_after` fields to `PackageConfig` without changing any behavior.

### 1.1 Update `src/config.rs`

Add fields to `PackageConfig`:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct PackageConfig {
    pub description: Option<String>,
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub suggests: Vec<String>,
    pub target: Option<String>,
    pub strategy: Option<DeployStrategy>,
    #[serde(default)]
    pub permissions: HashMap<String, String>,
    #[serde(default)]
    pub system: bool,
    pub owner: Option<String>,
    pub group: Option<String>,
    #[serde(default)]
    pub ownership: HashMap<String, String>,
    #[serde(default)]
    pub preserve: HashMap<String, Vec<String>>,
    pub pre_deploy: Option<String>,
    pub post_deploy: Option<String>,
    pub pre_undeploy: Option<String>,
    pub post_undeploy: Option<String>,
    // NEW FIELDS:
    pub setup: Option<String>,
    pub setup_shell: Option<String>,
    #[serde(default)]
    pub setup_after: Vec<String>,
}
```

**Note**: `setup_shell` is `Option<String>`, not `String` with serde default.
`PackageConfig` derives `Default`, so a bare `String` with `#[serde(default)]` would
give empty string `""` when constructed via `Default::default()` (e.g., in tests).
Default to `"sh"` at the execution site instead.

### 1.2 Verify

```bash
just test
```

All existing tests should pass. The new fields don't affect anything yet.

---

## Step 2: State Management

**Goal**: Create `setup_state.rs` module for tracking setup execution.

### 2.1 Create `src/setup_state.rs`

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SETUP_STATE_FILE: &str = "setup-state.json";
const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SetupState {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(skip)]
    state_dir: PathBuf,
    #[serde(default)]
    entries: HashMap<String, SetupEntry>,
}

fn default_version() -> u32 {
    CURRENT_VERSION
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

impl SetupState {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            version: CURRENT_VERSION,
            state_dir: state_dir.to_path_buf(),
            entries: HashMap::new(),
        }
    }

    /// Load state from disk, creating empty state if file doesn't exist
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

    /// Save state to disk
    pub fn save(&self) -> Result<()> {
        if !self.state_dir.exists() {
            std::fs::create_dir_all(&self.state_dir)
                .with_context(|| format!("failed to create state dir: {}", self.state_dir.display()))?;
        }

        let path = self.state_dir.join(SETUP_STATE_FILE);
        let content = serde_json::to_string_pretty(self)
            .context("failed to serialize setup state")?;

        std::fs::write(&path, content)
            .with_context(|| format!("failed to write setup state: {}", path.display()))?;

        Ok(())
    }

    /// Get entry for a package
    pub fn get(&self, package: &str) -> Option<&SetupEntry> {
        self.entries.get(package)
    }

    /// Update or insert entry for a package
    pub fn update(&mut self, package: String, entry: SetupEntry) {
        self.entries.insert(package, entry);
    }

    /// Get all packages with setup state
    pub fn packages(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_state_is_empty() {
        let dir = TempDir::new().unwrap();
        let state = SetupState::new(dir.path());
        assert_eq!(state.version, CURRENT_VERSION);
        assert!(state.entries.is_empty());
    }

    #[test]
    fn load_missing_file_returns_empty_state() {
        let dir = TempDir::new().unwrap();
        let state = SetupState::load(dir.path()).unwrap();
        assert!(state.entries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut state = SetupState::new(dir.path());

        state.update(
            "test-pkg".to_string(),
            SetupEntry {
                last_run: "2026-03-31T12:00:00Z".to_string(),
                script_hash: "abc123".to_string(),
                status: SetupStatus::Success,
                exit_code: 0,
                duration_ms: 100,
                error: None,
            },
        );

        state.save().unwrap();

        let loaded = SetupState::load(dir.path()).unwrap();
        assert_eq!(loaded.entries.len(), 1);
        let entry = loaded.get("test-pkg").unwrap();
        assert_eq!(entry.status, SetupStatus::Success);
        assert_eq!(entry.script_hash, "abc123");
    }
}
```

### 2.2 Export from `src/lib.rs`

Add to `src/lib.rs`:

```rust
pub mod setup_state;
```

### 2.3 Verify

```bash
cargo test setup_state
```

Should see tests passing for the new module.

---

## Step 3: Core Execution Logic

**Goal**: Create `setup.rs` with orchestration and execution.

### 3.1 Create `src/setup.rs`

```rust
use crate::config::{PackageConfig, RootConfig};
use crate::hash;
use crate::loader::ConfigLoader;
use crate::orchestrator::expand_path;
use crate::resolver;
use crate::setup_state::{SetupEntry, SetupState, SetupStatus};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub struct SetupOrchestrator {
    loader: ConfigLoader,
    state_dir: PathBuf,
    package_filter: Option<Vec<String>>,
}

impl SetupOrchestrator {
    pub fn new(dotfiles_dir: &Path, state_dir: &Path) -> Result<Self> {
        let loader = ConfigLoader::new(dotfiles_dir)?;
        Ok(Self {
            loader,
            state_dir: state_dir.to_path_buf(),
            package_filter: None,
        })
    }

    pub fn with_package_filter(mut self, filter: Option<Vec<String>>) -> Self {
        self.package_filter = filter;
        self
    }

    pub fn loader(&self) -> &ConfigLoader {
        &self.loader
    }

    /// Run setup tasks
    pub fn run(
        &self,
        hostname: &str,
        dry_run: bool,
        force: bool,
    ) -> Result<SetupReport> {
        let mut state = SetupState::load(&self.state_dir)?;
        let mut report = SetupReport::default();

        // Resolve packages for this host
        let host_config = self.loader.load_host(hostname)?;
        let mut role_packages = Vec::new();
        for role_name in &host_config.roles {
            let role_config = self.loader.load_role(role_name)?;
            role_packages.extend(role_config.packages);
        }

        // Resolve dependencies
        let pkg_names_refs: Vec<&str> = role_packages.iter().map(|s| s.as_str()).collect();
        let resolved = resolver::resolve_packages(self.loader.root(), &pkg_names_refs)?;

        // Apply package filter if specified
        let packages_to_setup: Vec<String> = if let Some(filter) = &self.package_filter {
            resolved
                .into_iter()
                .filter(|p| filter.contains(p))
                .collect()
        } else {
            resolved
        };

        // Filter to packages with setup field
        let mut setup_packages = Vec::new();
        for pkg_name in &packages_to_setup {
            if let Some(pkg_config) = self.loader.root().packages.get(pkg_name) {
                if pkg_config.setup.is_some() {
                    setup_packages.push(pkg_name.clone());
                }
            }
        }

        // Build setup dependency graph
        let setup_order = self.resolve_setup_order(&setup_packages)?;

        // Execute setup for each package in order
        for pkg_name in setup_order {
            let pkg_config = self.loader.root().packages.get(&pkg_name).unwrap();
            let setup_cmd = pkg_config.setup.as_ref().unwrap();

            let should_run = self.should_run_setup(&pkg_name, pkg_config, &state, force)?;

            if !should_run.0 {
                report.skipped.push((pkg_name.clone(), should_run.1));
                continue;
            }

            if dry_run {
                report.dry_run.push((pkg_name.clone(), setup_cmd.clone(), should_run.1));
                continue;
            }

            // Execute setup
            match self.execute_setup(&pkg_name, pkg_config) {
                Ok(entry) => {
                    state.update(pkg_name.clone(), entry.clone());
                    if entry.status == SetupStatus::Success {
                        report.success.push(pkg_name);
                    } else {
                        report.failed.push((pkg_name, entry.error.clone()));
                        break; // Stop on first failure
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    report.failed.push((pkg_name.clone(), Some(error_msg.clone())));

                    // Record failure in state
                    let hash = self.compute_setup_hash(&pkg_name, pkg_config)?;
                    state.update(
                        pkg_name.clone(),
                        SetupEntry {
                            last_run: chrono::Utc::now().to_rfc3339(),
                            script_hash: hash,
                            status: SetupStatus::Failed,
                            exit_code: 1,
                            duration_ms: 0,
                            error: Some(error_msg),
                        },
                    );
                    break; // Stop on first failure
                }
            }
        }

        // Save state
        if !dry_run {
            state.save()?;
        }

        Ok(report)
    }

    /// List packages with setup tasks and their status
    pub fn list(&self, hostname: &str) -> Result<Vec<SetupListEntry>> {
        let state = SetupState::load(&self.state_dir)?;

        // Resolve packages for this host (same logic as run())
        let host_config = self.loader.load_host(hostname)?;
        let mut role_packages = Vec::new();
        for role_name in &host_config.roles {
            let role_config = self.loader.load_role(role_name)?;
            role_packages.extend(role_config.packages);
        }

        let pkg_names_refs: Vec<&str> = role_packages.iter().map(|s| s.as_str()).collect();
        let resolved = resolver::resolve_packages(self.loader.root(), &pkg_names_refs)?;

        let mut entries = Vec::new();
        for pkg_name in resolved {
            if let Some(pkg_config) = self.loader.root().packages.get(&pkg_name) {
                if let Some(setup_cmd) = &pkg_config.setup {
                    let state_entry = state.get(&pkg_name);
                    let current_hash = self.compute_setup_hash(&pkg_name, pkg_config)?;

                    let status = match state_entry {
                        None => SetupListStatus::NotRun,
                        Some(entry) if entry.script_hash != current_hash => SetupListStatus::Changed,
                        Some(entry) if entry.status == SetupStatus::Failed => {
                            SetupListStatus::Failed(entry.last_run.clone())
                        }
                        Some(entry) => SetupListStatus::Success(entry.last_run.clone()),
                    };

                    entries.push(SetupListEntry {
                        package: pkg_name,
                        command: setup_cmd.clone(),
                        status,
                        error: state_entry.and_then(|e| e.error.clone()),
                    });
                }
            }
        }

        Ok(entries)
    }

    fn should_run_setup(
        &self,
        package: &str,
        config: &PackageConfig,
        state: &SetupState,
        force: bool,
    ) -> Result<(bool, &'static str)> {
        if force {
            return Ok((true, "forced re-run"));
        }

        let current_hash = self.compute_setup_hash(package, config)?;

        match state.get(package) {
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

    fn execute_setup(&self, package: &str, config: &PackageConfig) -> Result<SetupEntry> {
        let setup_cmd = config.setup.as_ref().unwrap();
        let shell_str = config.setup_shell.as_deref().unwrap_or("sh");
        let pkg_dir = self.loader.packages_dir().join(package);

        // Resolve target directory
        let target = if let Some(ref target_path) = config.target {
            PathBuf::from(expand_path(target_path, Some(&format!("package '{package}'")))?)
        } else {
            dirs::home_dir().context("failed to determine home directory")?
        };

        let start = Instant::now();
        let hash = self.compute_setup_hash(package, config)?;

        // Parse shell string: "zsh" → ["zsh"], "/bin/zsh -l" → ["/bin/zsh", "-l"]
        let shell_parts: Vec<&str> = shell_str.split_whitespace().collect();
        let shell_bin = shell_parts.first().context("setup_shell is empty")?;

        let mut cmd = Command::new(shell_bin);
        // Add any extra flags (e.g., "-l" from "/bin/zsh -l")
        for flag in &shell_parts[1..] {
            cmd.arg(flag);
        }
        cmd.arg("-c")
            .arg(setup_cmd)
            .current_dir(&pkg_dir)
            .env("DOTM_PACKAGE", package)
            .env("DOTM_SETUP_ROOT", &target)
            .env("DOTM_PACKAGES_DIR", self.loader.packages_dir());

        // Execute command
        let status = cmd
            .status()
            .with_context(|| {
                format!("failed to execute setup for package '{package}' with shell '{shell_str}'")
            })?;

        let duration = start.elapsed();

        let entry = if status.success() {
            SetupEntry {
                last_run: chrono::Utc::now().to_rfc3339(),
                script_hash: hash,
                status: SetupStatus::Success,
                exit_code: status.code().unwrap_or(0),
                duration_ms: duration.as_millis() as u64,
                error: None,
            }
        } else {
            SetupEntry {
                last_run: chrono::Utc::now().to_rfc3339(),
                script_hash: hash,
                status: SetupStatus::Failed,
                exit_code: status.code().unwrap_or(1),
                duration_ms: duration.as_millis() as u64,
                error: Some(format!(
                    "Command '{}' failed with exit code {}",
                    setup_cmd,
                    status.code().unwrap_or(1)
                )),
            }
        };

        Ok(entry)
    }

    fn compute_setup_hash(&self, package: &str, config: &PackageConfig) -> Result<String> {
        let setup_cmd = config.setup.as_ref().unwrap();
        let pkg_dir = self.loader.packages_dir().join(package);

        // Check if it looks like a file path (relative, no shell metacharacters)
        if setup_cmd.starts_with("./") || setup_cmd.starts_with("scripts/") {
            let script_path = pkg_dir.join(setup_cmd);
            if script_path.exists() && script_path.is_file() {
                return hash::hash_file(&script_path);
            }
        }

        // Otherwise hash the command string itself
        Ok(hash::hash_content(setup_cmd.as_bytes()))
    }

    fn resolve_setup_order(&self, packages: &[String]) -> Result<Vec<String>> {
        // Build dependency graph combining package depends + setup_after
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let pkg_set: HashSet<_> = packages.iter().cloned().collect();

        for pkg_name in packages {
            if let Some(pkg_config) = self.loader.root().packages.get(pkg_name) {
                let mut deps = Vec::new();

                // Add package dependencies (only if they're in the setup set)
                for dep in &pkg_config.depends {
                    if pkg_set.contains(dep) {
                        deps.push(dep.clone());
                    }
                }

                // Add setup_after dependencies
                for after in &pkg_config.setup_after {
                    // Validate that the package exists at all in root config
                    if !self.loader.root().packages.contains_key(after) {
                        bail!(
                            "Package '{}' setup_after references unknown package '{}'",
                            pkg_name,
                            after
                        );
                    }
                    // Only add to graph if the referenced package is in the setup set
                    // (has a setup field and is active). If it's not, the constraint
                    // is a no-op — the referenced package simply has no setup to order against.
                    if pkg_set.contains(after) && !deps.contains(after) {
                        deps.push(after.clone());
                    }
                }

                graph.insert(pkg_name.clone(), deps);
            }
        }

        // Topological sort
        topological_sort(&graph, packages)
    }
}

fn topological_sort(
    graph: &HashMap<String, Vec<String>>,
    packages: &[String],
) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    for pkg in packages {
        if !visited.contains(pkg) {
            topo_visit(pkg, graph, &mut visited, &mut stack, &mut result)?;
        }
    }

    Ok(result)
}

fn topo_visit(
    pkg: &str,
    graph: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
    result: &mut Vec<String>,
) -> Result<()> {
    if stack.contains(&pkg.to_string()) {
        stack.push(pkg.to_string());
        bail!("Circular setup dependency detected: {}", stack.join(" -> "));
    }

    if visited.contains(pkg) {
        return Ok(());
    }

    stack.push(pkg.to_string());

    if let Some(deps) = graph.get(pkg) {
        for dep in deps {
            topo_visit(dep, graph, visited, stack, result)?;
        }
    }

    stack.pop();
    visited.insert(pkg.to_string());
    result.push(pkg.to_string());

    Ok(())
}

#[derive(Debug, Default)]
pub struct SetupReport {
    pub success: Vec<String>,
    pub failed: Vec<(String, Option<String>)>,
    pub skipped: Vec<(String, &'static str)>,
    pub dry_run: Vec<(String, String, &'static str)>,
}

pub struct SetupListEntry {
    pub package: String,
    pub command: String,
    pub status: SetupListStatus,
    pub error: Option<String>,
}

pub enum SetupListStatus {
    NotRun,
    Success(String),  // timestamp
    Failed(String),   // timestamp
    Changed,
}
```

### 3.2 Timestamps

The implementation needs RFC 3339 timestamps for `SetupEntry.last_run`. Options:

- **`jiff` crate** (recommended): Modern, lightweight datetime library. Add `jiff = "0.2"` to `Cargo.toml`. Usage: `jiff::Zoned::now().to_string()`.
- **Manual formatting**: Use `std::time::SystemTime` and format manually (no dep, but tedious).
- **`chrono` crate**: Well-known but heavyweight. Add `chrono = "0.4"` to `Cargo.toml`. Usage: `chrono::Utc::now().to_rfc3339()`.

Choose whichever the project prefers. All subsequent code in this plan uses `Utc::now().to_rfc3339()` as a placeholder — substitute the chosen approach.

### 3.3 Export from `src/lib.rs`

```rust
pub mod setup;
```

### 3.4 Verify compilation

```bash
cargo build
```

Should compile without errors.

---

## Step 4: CLI Integration

**Goal**: Add `Setup` command and wire it up in main.rs.

### 4.1 Add command to `src/main.rs`

Add to `Commands` enum:

```rust
#[derive(clap::Subcommand)]
enum Commands {
    // ... existing commands ...

    /// Run package setup tasks
    Setup {
        /// Target host (defaults to system hostname)
        #[arg(long)]
        host: Option<String>,
        /// Show what would be executed without running
        #[arg(long)]
        dry_run: bool,
        /// Re-run setup even if already executed successfully
        #[arg(short, long)]
        force: bool,
        /// List available setup tasks and their status
        #[arg(short, long)]
        list: bool,
        /// Run setup only for this package (and its dependencies)
        #[arg(short, long)]
        package: Option<String>,
        /// Operate on system packages (requires root)
        #[arg(long)]
        system: bool,
    },

    // ... rest of commands ...
}
```

### 4.2 Add command handler

Add to the match statement in `main()`:

```rust
Commands::Setup {
    host,
    dry_run,
    force,
    list,
    package,
    system,
} => {
    // Match existing hostname resolution pattern from deploy handler
    let hostname = match host {
        Some(h) => h,
        None => hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| {
                eprintln!("error: could not detect hostname, use --host to specify");
                std::process::exit(1);
            }),
    };

    let state_dir = if system {
        check_system_privileges();
        system_state_dir()
    } else {
        dotm_state_dir()
    };

    let mut orch = dotm::setup::SetupOrchestrator::new(&cli.dir, &state_dir)?;
    if let Some(ref filter) = package {
        orch = orch.with_package_filter(Some(vec![filter.clone()]));
    }

    if list {
        let entries = orch.list(&hostname)?;
        println!("Available setup tasks:\n");
        for entry in entries {
            println!("{}", entry.package);
            println!("  Command: {}", entry.command);
            match entry.status {
                dotm::setup::SetupListStatus::NotRun => {
                    println!("  Status: ○ Not run");
                }
                dotm::setup::SetupListStatus::Success(ts) => {
                    println!("  Status: ✓ Success (last run: {})", ts);
                }
                dotm::setup::SetupListStatus::Failed(ts) => {
                    println!("  Status: ✗ Failed (last run: {})", ts);
                    if let Some(err) = entry.error {
                        println!("  Error: {}", err);
                    }
                }
                dotm::setup::SetupListStatus::Changed => {
                    println!("  Status: ⚠ Changed (script modified since last run)");
                }
            }
            println!();
        }
        return Ok(());
    }

    let report = orch.run(&hostname, dry_run, force)?;

    if dry_run {
        println!("Setup (dry run):\n");
        for (pkg, cmd, reason) in report.dry_run {
            println!("{}:", pkg);
            println!("  Would execute: {}", cmd);
            println!("  Reason: {}", reason);
            println!();
        }
        for (pkg, reason) in report.skipped {
            println!("{}:", pkg);
            println!("  Would skip: {}", reason);
            println!();
        }
    } else {
        for pkg in &report.success {
            println!("✓ Setup succeeded: {}", pkg);
        }
        for (pkg, reason) in &report.skipped {
            println!("⊘ Setup skipped: {} ({})", pkg, reason);
        }
        for (pkg, err) in &report.failed {
            eprintln!("✗ Setup failed: {}", pkg);
            if let Some(msg) = err {
                eprintln!("  Error: {}", msg);
            }
        }

        if !report.failed.is_empty() {
            std::process::exit(1);
        }
    }
}
```

### 4.3 Test CLI

```bash
cargo build
./target/debug/dotm setup --help
```

Should show help for the setup command.

---

## Step 5: Validation

**Goal**: Add setup validation to `dotm check`.

### 5.1 Add validation function to `src/config.rs`

```rust
pub fn validate_setup(root: &RootConfig) -> Vec<String> {
    let mut errors = Vec::new();
    let pkg_names: std::collections::HashSet<_> = root.packages.keys().collect();

    for (name, pkg) in &root.packages {
        // Validate setup_after references exist
        for after in &pkg.setup_after {
            if !pkg_names.contains(after) {
                errors.push(format!(
                    "Package '{name}': setup_after references unknown package '{after}'"
                ));
            }
        }

        // Validate setup is not empty string
        if let Some(setup) = &pkg.setup {
            if setup.trim().is_empty() {
                errors.push(format!("Package '{name}': setup field cannot be empty"));
            }
        }

        // Validate setup_shell if it's a full path
        if let Some(ref shell) = pkg.setup_shell {
            // Parse shell string to extract binary (first token)
            let shell_bin = shell.split_whitespace().next().unwrap_or("");
            if shell_bin.starts_with('/') {
                let path = std::path::Path::new(shell_bin);
                if !path.exists() {
                    errors.push(format!(
                        "Package '{name}': setup_shell '{shell_bin}' does not exist"
                    ));
                } else if !is_executable(path) {
                    errors.push(format!(
                        "Package '{name}': setup_shell '{shell_bin}' is not executable"
                    ));
                }
            }
        }
    }

    // Check for circular setup dependencies
    // (This requires access to resolver logic, might be better in a separate validation pass)

    errors
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true // Can't check on non-Unix systems
}
```

### 5.2 Call from check command

In `main.rs`, find the `Commands::Check` handler and add:

```rust
Commands::Check { warn_suggestions } => {
    // ... existing validation ...

    let setup_errors = config::validate_setup(&root);
    if !setup_errors.is_empty() {
        has_error = true;
        eprintln!("✗ Setup configuration\n");
        for err in setup_errors {
            eprintln!("  {err}");
        }
    } else {
        println!("✓ Setup configuration");
    }

    // ... rest of check command ...
}
```

---

## Step 6: List Integration

**Goal**: Show setup info in `dotm list packages --verbose`.

### 6.1 Update `src/list.rs`

Find the `list_packages_verbose()` function (or wherever verbose package listing happens) and add:

```rust
// After showing existing package info:
if let Some(setup) = &pkg.setup {
    println!("  Setup: {}", setup);
    // Optionally show status if we can access state here
}
```

---

## Step 7: Testing

### 7.1 Create `tests/setup.rs`

```rust
use dotm::config::{DotmSettings, PackageConfig, RootConfig};
use dotm::setup::SetupOrchestrator;
use dotm::setup_state::{SetupState, SetupEntry, SetupStatus};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let dotfiles = dir.path();

    // Create dotm.toml
    let dotm_toml = r#"
[dotm]
target = "~"

[packages.test-success]
description = "Test package that succeeds"
setup = "echo 'setup succeeded'"

[packages.test-fail]
description = "Test package that fails"
setup = "exit 1"

[packages.test-script]
description = "Test package with script"
setup = "scripts/test.sh"

[packages.test-deps]
description = "Test package with setup_after"
setup = "echo 'after test-success'"
setup_after = ["test-success"]
"#;
    std::fs::write(dotfiles.join("dotm.toml"), dotm_toml).unwrap();

    // Create packages
    std::fs::create_dir(dotfiles.join("packages")).unwrap();
    std::fs::create_dir(dotfiles.join("packages/test-success")).unwrap();
    std::fs::create_dir(dotfiles.join("packages/test-fail")).unwrap();
    std::fs::create_dir(dotfiles.join("packages/test-deps")).unwrap();

    let test_script_pkg = dotfiles.join("packages/test-script");
    std::fs::create_dir(&test_script_pkg).unwrap();
    std::fs::create_dir(test_script_pkg.join("scripts")).unwrap();
    std::fs::write(
        test_script_pkg.join("scripts/test.sh"),
        "#!/bin/sh\necho 'script executed'\n",
    )
    .unwrap();

    // Create host and role configs
    std::fs::create_dir(dotfiles.join("hosts")).unwrap();
    std::fs::create_dir(dotfiles.join("roles")).unwrap();

    let host_toml = r#"
hostname = "test-host"
roles = ["test"]
"#;
    std::fs::write(dotfiles.join("hosts/test-host.toml"), host_toml).unwrap();

    let role_toml = r#"
packages = ["test-success", "test-fail", "test-script", "test-deps"]
"#;
    std::fs::write(dotfiles.join("roles/test.toml"), role_toml).unwrap();

    dir
}

#[test]
fn setup_success_execution() {
    let fixture = create_test_fixture();
    let state_dir = TempDir::new().unwrap();

    let orch = SetupOrchestrator::new(fixture.path(), state_dir.path()).unwrap();
    let orch = orch.with_package_filter(Some(vec!["test-success".to_string()]));

    let report = orch.run("test-host", false, false).unwrap();

    assert_eq!(report.success.len(), 1);
    assert!(report.success.contains(&"test-success".to_string()));
    assert!(report.failed.is_empty());
}

#[test]
fn setup_failure_stops_execution() {
    let fixture = create_test_fixture();
    let state_dir = TempDir::new().unwrap();

    let orch = SetupOrchestrator::new(fixture.path(), state_dir.path()).unwrap();
    let orch = orch.with_package_filter(Some(vec![
        "test-success".to_string(),
        "test-fail".to_string(),
    ]));

    let report = orch.run("test-host", false, false).unwrap();

    assert_eq!(report.success.len(), 1);
    assert_eq!(report.failed.len(), 1);
}

#[test]
fn setup_dry_run_does_not_execute() {
    let fixture = create_test_fixture();
    let state_dir = TempDir::new().unwrap();

    let orch = SetupOrchestrator::new(fixture.path(), state_dir.path()).unwrap();
    let orch = orch.with_package_filter(Some(vec!["test-success".to_string()]));

    let report = orch.run("test-host", true, false).unwrap();

    assert_eq!(report.dry_run.len(), 1);
    assert!(report.success.is_empty());

    // State should not be saved
    let state = SetupState::load(state_dir.path()).unwrap();
    assert!(state.get("test-success").is_none());
}

#[test]
fn setup_respects_dependencies() {
    let fixture = create_test_fixture();
    let state_dir = TempDir::new().unwrap();

    let orch = SetupOrchestrator::new(fixture.path(), state_dir.path()).unwrap();
    let orch = orch.with_package_filter(Some(vec![
        "test-deps".to_string(),
        "test-success".to_string(),
    ]));

    let report = orch.run("test-host", false, false).unwrap();

    // Both should succeed
    assert_eq!(report.success.len(), 2);

    // test-success should run before test-deps
    assert_eq!(report.success[0], "test-success");
    assert_eq!(report.success[1], "test-deps");
}

#[test]
fn setup_force_reruns() {
    let fixture = create_test_fixture();
    let state_dir = TempDir::new().unwrap();

    let orch = SetupOrchestrator::new(fixture.path(), state_dir.path()).unwrap();
    let orch = orch.with_package_filter(Some(vec!["test-success".to_string()]));

    // First run
    orch.run("test-host", false, false).unwrap();

    // Second run without force - should skip
    let report = orch.run("test-host", false, false).unwrap();
    assert_eq!(report.skipped.len(), 1);

    // Third run with force - should execute
    let report = orch.run("test-host", false, true).unwrap();
    assert_eq!(report.success.len(), 1);
}
```

### 7.2 Run tests

```bash
cargo test setup
```

---

## Step 8: Documentation

### 8.1 Update README.md

Add section after "Hooks" section:

```markdown
## Setup Tasks

Packages can define one-time or occasional setup tasks via the `setup` field:

```toml
[packages.homebrew]
description = "Homebrew package manager"
setup = "brew bundle --file=~/.Brewfile --no-lock"

[packages.macos-defaults]
description = "macOS system preferences"
setup = "scripts/apply-defaults.sh"
setup_shell = "zsh"

[packages.dev-tools]
description = "Development dependencies"
setup = "pip install -r requirements.txt"
setup_after = ["homebrew"]  # Run after homebrew setup
```

### Running Setup

```bash
dotm setup                    # Run all setup tasks for current host
dotm setup --package homebrew # Run only homebrew setup
dotm setup --dry-run          # Show what would run
dotm setup --force            # Re-run even if already successful
dotm setup --list             # Show setup tasks and their status
```

Setup tasks are tracked for idempotency and only re-run when:
- Never run before
- Script content has changed
- Previous run failed
- `--force` flag is used
```

### 8.2 Update CHANGELOG.md

Add to "Unreleased" section:

```markdown
## [Unreleased]

### Added
- **Setup command**: Run one-time package initialization tasks
  - `dotm setup` to execute setup tasks
  - `setup`, `setup_shell`, `setup_after` fields in package config
  - State tracking for idempotent execution
  - Change detection via script hashing
```

---

## Completion Checklist

Use this to track implementation progress:

- [ ] Step 1: Config schema (fields added, compiles)
- [ ] Step 2: State management (module created, tests pass)
- [ ] Step 3: Core execution (setup.rs created, compiles)
- [ ] Step 4: CLI integration (command works)
- [ ] Step 5: Validation (check command updated)
- [ ] Step 6: List integration (shows setup info)
- [ ] Step 7: Testing (tests written and passing)
- [ ] Step 8: Documentation (README and CHANGELOG updated)
- [ ] All tests passing (`just test`)
- [ ] Clippy clean (`just lint`)
- [ ] Manual testing with real dotfiles

---

## Testing Checklist

Manual testing scenarios:

- [ ] Run `dotm setup` with Homebrew Brewfile
- [ ] Run `dotm setup` with macOS defaults script
- [ ] Test `--dry-run` mode
- [ ] Test `--force` flag
- [ ] Test `--list` output
- [ ] Test setup failure handling
- [ ] Test setup dependency ordering
- [ ] Test script hash change detection
- [ ] Test with `--package` filter
- [ ] Test system package setup with sudo

---

## Troubleshooting

### Compilation Errors

**Missing timestamp crate**: Add chosen crate (jiff, chrono, etc.) to Cargo.toml
**Module not found**: Ensure `pub mod setup;` in lib.rs
**Type errors**: Check struct definitions match spec exactly

### Runtime Errors

**State file errors**: Check permissions on state directory
**Script not found**: Verify package directory structure
**Circular dependency**: Check setup_after references

### Test Failures

**Fixture creation fails**: Check TempDir permissions
**Execution fails**: Verify test scripts are executable
**State not saved**: Check dry_run flag isn't accidentally set

---

## Next Steps

After setup command is complete:

1. Implement platform filtering (see PLAN-platform-filtering.md)
2. Create GitHub issue for teardown feature (see docs/ISSUE-setup-teardown.md)
3. Migrate macOS dotfiles packages (see docs/GUIDE-macos-migration.md)
