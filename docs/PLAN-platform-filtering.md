# Implementation Plan: Platform Filtering

This document provides step-by-step implementation guidance for platform filtering
as specified in `SPEC-platform-filtering.md`.

## Prerequisites

- Read `SPEC-platform-filtering.md` thoroughly
- Familiarity with dotm codebase (see `CLAUDE.md`)
- Rust 1.87+ with edition 2024 features

## Implementation Order

1. **Platform module** — New module with detection and validation functions
2. **Config schema** — Add `platform` field to `PackageConfig`
3. **Orchestrator filtering** — Skip incompatible packages during deploy
4. **Validation** — Add platform checks to `dotm check`
5. **List integration** — Show platform info in `dotm list packages`
6. **Testing** — Unit and integration tests
7. **Documentation** — README and CHANGELOG

Each step compiles and passes existing tests before moving to the next.

---

## Step 1: Platform Module

**Goal**: Create `src/platform.rs` with platform detection, normalization, and validation.

### 1.1 Create `src/platform.rs`

```rust
/// Canonical platform names used internally.
const VALID_PLATFORMS: &[&str] = &["linux", "darwin", "windows"];

/// Get the current platform identifier, normalized.
///
/// Maps `std::env::consts::OS` to canonical names:
/// - "macos" → "darwin"
/// - "linux" → "linux"
/// - "windows" → "windows"
pub fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

/// Normalize user-facing platform names to canonical form.
/// Accepts "macos" as alias for "darwin".
pub fn normalize_platform(platform: &str) -> &str {
    match platform {
        "macos" | "darwin" => "darwin",
        other => other,
    }
}

/// Check if a platform string is valid (before or after normalization).
pub fn is_valid_platform(platform: &str) -> bool {
    VALID_PLATFORMS.contains(&normalize_platform(platform))
}

/// Check if a package is compatible with the current platform.
/// `None` means no constraint (compatible with everything).
pub fn is_compatible(pkg_platform: Option<&str>) -> bool {
    match pkg_platform {
        None => true,
        Some(platform) => normalize_platform(platform) == current_platform(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_is_valid() {
        assert!(is_valid_platform(current_platform()));
    }

    #[test]
    fn darwin_is_valid() {
        assert!(is_valid_platform("darwin"));
    }

    #[test]
    fn macos_is_valid_alias() {
        assert!(is_valid_platform("macos"));
    }

    #[test]
    fn linux_is_valid() {
        assert!(is_valid_platform("linux"));
    }

    #[test]
    fn windows_is_valid() {
        assert!(is_valid_platform("windows"));
    }

    #[test]
    fn osx_is_invalid() {
        assert!(!is_valid_platform("osx"));
    }

    #[test]
    fn freebsd_is_invalid() {
        assert!(!is_valid_platform("freebsd"));
    }

    #[test]
    fn no_constraint_is_compatible() {
        assert!(is_compatible(None));
    }

    #[test]
    fn matching_platform_is_compatible() {
        assert!(is_compatible(Some(current_platform())));
    }

    #[test]
    fn non_matching_platform_is_incompatible() {
        let other = if current_platform() == "darwin" {
            "linux"
        } else {
            "darwin"
        };
        assert!(!is_compatible(Some(other)));
    }

    #[test]
    fn macos_and_darwin_are_equivalent() {
        assert_eq!(normalize_platform("macos"), normalize_platform("darwin"));
    }

    #[test]
    fn macos_alias_compatible_on_darwin() {
        // If we're on macOS, "macos" should be compatible
        if current_platform() == "darwin" {
            assert!(is_compatible(Some("macos")));
            assert!(is_compatible(Some("darwin")));
        }
    }
}
```

### 1.2 Export from `src/lib.rs`

Add to `src/lib.rs`:

```rust
pub mod platform;
```

### 1.3 Verify

```bash
cargo test platform
```

---

## Step 2: Config Schema

**Goal**: Add `platform` field to `PackageConfig` and add a validation function.

### 2.1 Update `src/config.rs`

Add field to `PackageConfig`:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct PackageConfig {
    // ... existing fields ...
    pub platform: Option<String>,
}
```

This is fully backward compatible — existing configs without `platform` deserialize
to `None`, which means "all platforms."

### 2.2 Add validation function

Add to `src/config.rs`:

```rust
pub fn validate_platforms(root: &RootConfig) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, pkg) in &root.packages {
        if let Some(ref platform) = pkg.platform {
            if !crate::platform::is_valid_platform(platform) {
                errors.push(format!(
                    "package '{name}': invalid platform '{platform}'. \
                     Valid platforms: linux, darwin (or macos), windows"
                ));
            }
        }
    }
    errors
}
```

### 2.3 Verify

```bash
just test
```

All existing tests pass (new field is `Option`, defaults to `None`).

---

## Step 3: Orchestrator Filtering

**Goal**: Skip platform-incompatible packages during deploy.

### 3.1 Where to add the filter

The filtering goes in `orchestrator.rs` inside the `deploy()` method's per-package
loop, alongside the existing system mode filter. This is the exact right location
because:

- It's **after** dependency resolution (so the resolver doesn't fail on cross-platform deps)
- It's **before** scanning (so we don't waste time scanning incompatible packages)
- It mirrors the existing `system_mode` filter pattern

The relevant code is in `orchestrator.rs` around line 150:

```rust
for pkg_name in &resolved {
    // Filter packages based on system mode
    let is_system = self
        .loader
        .root()
        .packages
        .get(pkg_name)
        .map(|c| c.system)
        .unwrap_or(false);
    if self.system_mode != is_system {
        continue;
    }

    // ADD PLATFORM FILTER HERE

    let pkg_dir = packages_dir.join(pkg_name);
    // ...
```

### 3.2 Add platform filter

Insert after the system mode filter:

```rust
    // Filter packages based on platform
    if let Some(pkg_config) = self.loader.root().packages.get(pkg_name) {
        if !crate::platform::is_compatible(pkg_config.platform.as_deref()) {
            report.platform_skipped.push(pkg_name.clone());
            continue;
        }
    }
```

### 3.3 Update `DeployReport`

Add a field to track platform-skipped packages:

```rust
#[derive(Debug, Default)]
pub struct DeployReport {
    pub created: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
    pub unchanged: Vec<PathBuf>,
    pub conflicts: Vec<(PathBuf, String)>,
    pub dry_run_actions: Vec<PathBuf>,
    pub orphaned: Vec<PathBuf>,
    pub pruned: Vec<PathBuf>,
    pub platform_skipped: Vec<String>,  // NEW
}
```

### 3.4 Update deploy output in `main.rs`

In the `Commands::Deploy` handler (around line 228), add after the existing
deploy output:

```rust
if !report.platform_skipped.is_empty() {
    println!(
        "Skipped {} packages (platform mismatch): {}",
        report.platform_skipped.len(),
        report.platform_skipped.join(", ")
    );
}
```

### 3.5 Important: Do NOT filter in undeploy/restore/status

These commands operate on deploy state (`dotm-state.json`), not on config.
A user must be able to undeploy files that were deployed on a different platform
(e.g., cleaning up state after migrating between machines). No changes needed
to these commands — they already work on state entries regardless of config.

### 3.6 Verify

```bash
just test
```

Existing tests pass. The new field defaults to empty vec, no behavioral change
for packages without `platform`.

---

## Step 4: Validation

**Goal**: Add platform checks to `dotm check`.

### 4.1 Update check command in `main.rs`

Find the `Commands::Check` handler (line 505) and add after the
`validate_system_packages` call (line 579):

```rust
// Validate system package configuration
errors.extend(dotm::config::validate_system_packages(root));

// Validate platform configuration   // NEW
errors.extend(dotm::config::validate_platforms(root));
```

### 4.2 Add cross-platform dependency warnings

Add after the existing `warn_suggestions` block (around line 550-559):

```rust
if warn_suggestions {
    // Existing suggests warnings...

    // Warn about cross-platform dependencies
    for dep in &pkg_config.depends {
        if let Some(dep_config) = root.packages.get(dep) {
            if let Some(ref dep_platform) = dep_config.platform {
                if pkg_config.platform.is_none() {
                    eprintln!(
                        "warning: package '{}' is platform-agnostic but depends on \
                         '{}' ({} only)",
                        pkg_name, dep, dep_platform
                    );
                }
            }
        }
    }
}
```

### 4.3 Verify

```bash
just test
# Also test manually:
cd ~/dotfiles && dotm check
```

---

## Step 5: List Integration

**Goal**: Show platform info in `dotm list packages`.

### 5.1 Update `src/list.rs`

In `render_packages`, add platform info in both verbose and non-verbose modes.

**Verbose mode** — add after the `system: true` line (around line 32):

```rust
if let Some(ref platform) = pkg.platform {
    let compat = crate::platform::is_compatible(Some(platform));
    let indicator = if compat { "✓" } else { "✗" };
    out.push_str(&format!(
        "  platform: {platform} {indicator}\n"
    ));
} else if verbose {
    out.push_str("  platform: all\n");
}
```

**Non-verbose mode** — optionally append platform tag after description
(around line 36):

```rust
if let Some(ref platform) = root.packages[name].platform {
    out.push_str(&format!(" [{platform}]"));
}
```

This gives output like:

```
zsh — Zsh shell configuration
kde — KDE Plasma desktop configuration [linux]
alfred — Alfred preferences [darwin]
```

And verbose:

```
kde — KDE Plasma desktop configuration
  platform: linux ✓
  depends: util
  strategy: Stage
```

### 5.2 Verify

```bash
cd ~/dotfiles && dotm list packages
cd ~/dotfiles && dotm list packages -v
```

---

## Step 6: Testing

### 6.1 Config parsing tests

Add to `tests/config_parsing.rs`:

```rust
#[test]
fn platform_field_parses() {
    let toml = r#"
[dotm]
target = "~"

[packages.kde]
description = "KDE"
platform = "linux"

[packages.zsh]
description = "Zsh"
"#;
    let root: dotm::config::RootConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        root.packages["kde"].platform.as_deref(),
        Some("linux")
    );
    assert_eq!(root.packages["zsh"].platform, None);
}

#[test]
fn platform_macos_alias_parses() {
    let toml = r#"
[dotm]
target = "~"

[packages.alfred]
description = "Alfred"
platform = "macos"
"#;
    let root: dotm::config::RootConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        root.packages["alfred"].platform.as_deref(),
        Some("macos")
    );
    // Validation should accept it
    let errors = dotm::config::validate_platforms(&root);
    assert!(errors.is_empty());
}

#[test]
fn platform_invalid_value_rejected() {
    let toml = r#"
[dotm]
target = "~"

[packages.foo]
description = "Foo"
platform = "osx"
"#;
    let root: dotm::config::RootConfig = toml::from_str(toml).unwrap();
    let errors = dotm::config::validate_platforms(&root);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("invalid platform"));
    assert!(errors[0].contains("osx"));
}
```

### 6.2 Integration tests for deploy filtering

Add to `tests/e2e.rs` or create new `tests/platform.rs`:

```rust
use std::path::PathBuf;
use tempfile::TempDir;

fn create_platform_fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    // dotm.toml with platform-specific packages
    let dotm_toml = format!(r#"
[dotm]
target = "{target}"

[packages.cross]
description = "Cross-platform package"

[packages.native]
description = "Native package"
platform = "{current}"

[packages.foreign]
description = "Foreign package"
platform = "{foreign}"
"#,
        target = root.join("target").display(),
        current = dotm::platform::current_platform(),
        foreign = if dotm::platform::current_platform() == "darwin" {
            "linux"
        } else {
            "darwin"
        },
    );

    std::fs::write(root.join("dotm.toml"), dotm_toml).unwrap();

    // Create package directories with a file each
    for pkg in &["cross", "native", "foreign"] {
        let pkg_dir = root.join("packages").join(pkg);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join(format!(".{pkg}rc")), "# config\n").unwrap();
    }

    // Create target directory
    std::fs::create_dir_all(root.join("target")).unwrap();

    // Host and role
    std::fs::create_dir(root.join("hosts")).unwrap();
    std::fs::create_dir(root.join("roles")).unwrap();

    std::fs::write(
        root.join("hosts/test-host.toml"),
        "hostname = \"test-host\"\nroles = [\"all\"]\n",
    )
    .unwrap();

    std::fs::write(
        root.join("roles/all.toml"),
        "packages = [\"cross\", \"native\", \"foreign\"]\n",
    )
    .unwrap();

    dir
}

#[test]
fn deploy_skips_foreign_platform() {
    let fixture = create_platform_fixture();
    let state_dir = TempDir::new().unwrap();
    let target = fixture.path().join("target");

    let mut orch = dotm::orchestrator::Orchestrator::new(fixture.path(), &target)
        .unwrap()
        .with_state_dir(state_dir.path());

    let report = orch.deploy("test-host", false, false).unwrap();

    // cross and native should deploy, foreign should be skipped
    assert_eq!(report.platform_skipped.len(), 1);
    assert!(report.platform_skipped.contains(&"foreign".to_string()));

    // Verify files exist for deployed packages
    assert!(target.join(".crossrc").exists() || target.join(".staged").exists());

    // Verify foreign package file was NOT deployed
    assert!(!target.join(".foreignrc").exists());
}

#[test]
fn deploy_dry_run_reports_platform_skips() {
    let fixture = create_platform_fixture();
    let state_dir = TempDir::new().unwrap();
    let target = fixture.path().join("target");

    let mut orch = dotm::orchestrator::Orchestrator::new(fixture.path(), &target)
        .unwrap()
        .with_state_dir(state_dir.path());

    let report = orch.deploy("test-host", true, false).unwrap();

    // Foreign package should still be reported as skipped
    assert_eq!(report.platform_skipped.len(), 1);
    assert!(report.platform_skipped.contains(&"foreign".to_string()));
}

#[test]
fn platform_agnostic_packages_always_deploy() {
    let fixture = create_platform_fixture();
    let state_dir = TempDir::new().unwrap();
    let target = fixture.path().join("target");

    let mut orch = dotm::orchestrator::Orchestrator::new(fixture.path(), &target)
        .unwrap()
        .with_state_dir(state_dir.path());

    let report = orch.deploy("test-host", false, false).unwrap();

    // "cross" has no platform field, should never be in platform_skipped
    assert!(!report.platform_skipped.contains(&"cross".to_string()));
}
```

### 6.3 CLI tests

Add to `tests/cli.rs`:

```rust
#[test]
fn check_rejects_invalid_platform() {
    // Create fixture with invalid platform
    let dir = TempDir::new().unwrap();
    let dotm_toml = r#"
[dotm]
target = "~"

[packages.bad]
description = "Bad platform"
platform = "osx"
"#;
    std::fs::write(dir.path().join("dotm.toml"), dotm_toml).unwrap();
    std::fs::create_dir_all(dir.path().join("packages/bad")).unwrap();

    Command::cargo_bin("dotm")
        .unwrap()
        .arg("-d")
        .arg(dir.path())
        .arg("check")
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid platform"));
}
```

### 6.4 Run all tests

```bash
just check  # runs test + lint
```

---

## Step 7: Documentation

### 7.1 Update README.md

Add a new section after "File Overrides" (or wherever makes sense in the flow):

```markdown
## Platform Filtering

Packages can declare which operating system they require:

```toml
[packages.kde]
description = "KDE Plasma desktop"
platform = "linux"

[packages.alfred]
description = "Alfred preferences"
platform = "darwin"  # "macos" also accepted

[packages.zsh]
description = "Zsh shell configuration"
# No platform field = works everywhere
```

During `dotm deploy`, packages whose platform doesn't match the current OS are
automatically skipped. This lets you use shared roles across hosts without manually
excluding incompatible packages.

Valid platform values: `linux`, `darwin` (or `macos`), `windows`.
```

### 7.2 Update comparison table

In the README comparison table, add a row:

```markdown
| Platform filtering | Yes | No | No | No |
```

### 7.3 Update CHANGELOG.md

```markdown
## [Unreleased]

### Added
- **Platform filtering**: Packages can declare `platform = "linux"` / `"darwin"` / `"windows"` to restrict deployment to matching operating systems. Incompatible packages are automatically skipped during deploy. `"macos"` accepted as alias for `"darwin"`.
```

---

## Completion Checklist

- [ ] Step 1: Platform module (created, inline tests pass)
- [ ] Step 2: Config schema (`platform` field added, compiles)
- [ ] Step 3: Orchestrator filtering (packages skipped, report updated)
- [ ] Step 4: Validation (check command catches invalid platforms)
- [ ] Step 5: List integration (shows platform info)
- [ ] Step 6: Testing (all tests written and passing)
- [ ] Step 7: Documentation (README and CHANGELOG updated)
- [ ] All tests passing (`just test`)
- [ ] Clippy clean (`just lint`)
- [ ] Manual testing with real dotfiles

---

## Manual Testing Checklist

After implementation, test with the actual dotfiles repo:

- [ ] Add `platform = "linux"` to kde, gaming, pipewire packages in `~/dotfiles/dotm.toml`
- [ ] Add `platform = "darwin"` to any new macOS packages
- [ ] Run `dotm deploy --dry-run` on macOS — verify linux packages are skipped
- [ ] Run `dotm list packages -v` — verify platform labels shown
- [ ] Run `dotm check` — verify no errors
- [ ] Run `dotm status` — verify it still works (no platform filtering on state)
- [ ] Run `dotm check --warn-suggestions` — verify cross-platform dep warnings

---

## Integration with Setup Command

If implementing platform filtering and setup command in the same release,
ensure that `setup.rs` also filters by platform. The `SetupOrchestrator.run()`
method should filter packages the same way as the deploy orchestrator:

```rust
// In setup.rs, after resolving packages and filtering to those with setup field:
let setup_packages: Vec<String> = setup_packages
    .into_iter()
    .filter(|pkg_name| {
        let pkg_config = self.loader.root().packages.get(pkg_name);
        match pkg_config {
            Some(cfg) => crate::platform::is_compatible(cfg.platform.as_deref()),
            None => true,
        }
    })
    .collect();
```

---

## Notes

### Why filter in the per-package loop, not before resolution?

The dependency resolver (`resolver::resolve_packages`) validates that all referenced
packages exist. If we filtered *before* resolution, cross-platform dependencies would
cause resolver errors:

```toml
[packages.dev]
depends = ["homebrew"]  # homebrew is darwin-only

[packages.homebrew]
platform = "darwin"
```

On Linux, filtering before resolution would remove `homebrew` from the package list,
then the resolver would fail with "package 'dev' depends on unknown package 'homebrew'".

By filtering *after* resolution (in the per-package loop), the resolver succeeds,
and then `homebrew` is silently skipped during the deploy phase. `dev` still deploys
its files, it just doesn't get `homebrew`'s files alongside it.

### `normalize_platform` lifetime

The `normalize_platform` function returns `&str` borrowed from the input for unknown
values. This works because the function only processes config-parsed strings that live
long enough. For the known cases (`"macos"`, `"darwin"`, `"linux"`, `"windows"`), it
returns `&'static str` literals.

If this causes borrow issues, switch to returning `String` or `Cow<'_, str>`.

## References

- Existing system mode filter: `orchestrator.rs:150-161`
- Validation pattern: `config::validate_system_packages()`
- List rendering: `list.rs:render_packages()`
- Check command: `main.rs:505-590`
- Platform detection: `std::env::consts::OS`
