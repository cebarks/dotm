# Specification: Platform Filtering

**Status**: Draft
**Created**: 2026-03-31
**Author**: Anten Skrabec

## Overview

Platform filtering allows packages to declare which operating systems they support, preventing deployment of platform-specific packages on incompatible systems. This eliminates the need for per-host manual package selection when the only difference is the underlying OS.

## Motivation

### Current Limitations

Today, platform-specific packages require manual host configuration:

```toml
# hosts/relativity.toml (Linux workstation)
roles = ["base", "desktop", "gaming"]

# hosts/askrabec-mac.toml (macOS laptop)
roles = ["base", "macos"]  # Must manually exclude kde, gaming, etc.
```

Problems:
- **Maintenance burden**: Each host config must know which packages are platform-specific
- **Role duplication**: Need separate roles for Linux desktop vs macOS desktop
- **Error-prone**: Easy to accidentally include incompatible packages
- **Unclear intent**: No way to see *why* a package isn't deployed on a host

### Solution

Declare platform compatibility at the package level:

```toml
[packages.kde]
description = "KDE Plasma desktop configuration"
platform = "linux"  # Only deploy on Linux

[packages.alfred]
description = "Alfred preferences"
platform = "darwin"  # Only deploy on macOS
```

Now both hosts can use a shared "desktop" role, and dotm filters packages automatically based on the current platform.

## Goals

1. **Declarative platform constraints**: Specify which platforms a package supports
2. **Automatic filtering**: Skip incompatible packages during deployment
3. **Clear messaging**: Inform users why packages are skipped
4. **Validation**: Catch platform errors at `dotm check` time
5. **Backward compatible**: Packages without `platform` field deploy everywhere (existing behavior)

## Non-Goals

- **Cross-platform abstractions**: Packages handle platform differences themselves (via templates, `##host` overrides, etc.)
- **Version constraints**: No OS version filtering (e.g., "macOS 13+")
- **Architecture filtering**: No CPU architecture filtering (ARM vs x86)
- **Conditional deployment within packages**: All files in a package have the same platform constraint

## Configuration Schema

### Package-Level Platform Declaration

```toml
[packages.kde]
description = "KDE Plasma desktop configuration"
platform = "linux"

[packages.gaming]
description = "Gamemode, MangoHud, and gaming configs"
platform = "linux"

[packages.alfred]
description = "Alfred preferences and workflows"
platform = "darwin"

[packages.homebrew]
description = "Homebrew package manager"
platform = "darwin"

[packages.zsh]
description = "Zsh shell configuration"
# No platform field = works on all platforms
```

### Field Definition

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `platform` | string | No | `None` | Restrict package to this platform. Valid values: `"linux"`, `"darwin"`, `"windows"` |

### Platform Detection

Use Rust's `std::env::consts::OS`:

| `std::env::consts::OS` | Valid `platform` value |
|------------------------|------------------------|
| `"linux"` | `"linux"` |
| `"macos"` | `"darwin"` |
| `"windows"` | `"windows"` |

**Note**: `platform = "darwin"` (matches macOS kernel name) not `"macos"` for consistency with other tools (Homebrew, Rust target triples).

### Validation

Platform field accepts these strings: `"linux"`, `"darwin"`, `"windows"`.

`"macos"` is accepted as an alias for `"darwin"` (normalized internally) for ergonomics, since
users may not know the kernel name. All other values produce an error:

```
Error: Package 'foo': invalid platform 'osx'. Valid platforms: linux, darwin (or macos), windows
```

### Backward Compatibility

Packages without a `platform` field deploy on all platforms (existing behavior).

## Behavior

### Deployment Filtering

During `dotm deploy`:

1. Load host config → resolve roles → resolve packages (existing flow)
2. **Filter packages by platform**:
   - If package has `platform` field, check it matches current OS
   - If mismatch, skip package (don't scan, don't deploy)
   - If match or no field, proceed normally
3. Continue with existing deployment flow

### Dependency Handling

**Question**: What if package A depends on platform-specific package B?

**Example**:
```toml
[packages.dev]
depends = ["homebrew"]  # Homebrew is darwin-only

[packages.homebrew]
platform = "darwin"
```

**Behavior options**:

**Option A: Silent skip** (recommended)
- On macOS: Both `dev` and `homebrew` deploy
- On Linux: Only `dev` deploys (silently skip missing dependency)
- Rationale: Package authors can use platform-specific dependencies

**Option B: Error**
- On Linux: Fail with "package 'dev' depends on 'homebrew' which is not available on this platform"
- Rationale: Explicit about incompatible dependencies
- Problem: Prevents using shared packages that have optional platform deps

**Option C: Require conditional dependencies**
- Add new syntax: `depends_darwin = ["homebrew"]`
- Rationale: Most explicit
- Problem: Increases complexity significantly

**Decision**: **Option A** (silent skip). Allows flexible cross-platform package design without new syntax.

**Validation**: `dotm check` can warn about cross-platform dependencies with `--warn-suggestions` flag.

## Command-Line Interface

### Existing Commands

Platform filtering applies automatically to config-driven commands:

```bash
dotm deploy        # Only deploy platform-compatible packages
dotm setup         # Only run setup for platform-compatible packages
dotm check         # Validate platforms, warn about cross-platform deps
dotm list packages # Show all packages with platform compatibility
```

**Not filtered** (these operate on deploy state, not config — you must be able to
undeploy/restore files that were deployed on a different platform):

```bash
dotm undeploy      # Operates on deploy state, not config
dotm restore       # Operates on deploy state, not config
dotm status        # Shows deployed files regardless of current platform
```

### No New Flags

Platform filtering is automatic based on current OS. No flags needed.

## Terminal Output

### Deploy Output

When packages are skipped due to platform:

```
Deploying for host 'askrabec-mac'

Skipped (platform): kde (linux only)
Skipped (platform): gaming (linux only)
Skipped (platform): pipewire (linux only)

Deploying: zsh
Deploying: ssh
Deploying: util
Deploying: homebrew
Deploying: alfred

Deployed 5 packages (3 skipped: platform mismatch)
```

**Verbose mode** (`-v` if added later):
```
Skipped: kde
  Reason: Platform mismatch (requires linux, current: darwin)
```

### List Output

```bash
dotm list packages
```

Shows platform compatibility:

```
Available packages:

zsh
  Description: Zsh shell configuration
  Platform: all

kde
  Description: KDE Plasma desktop
  Platform: linux

alfred
  Description: Alfred preferences
  Platform: darwin

homebrew
  Description: Homebrew package manager
  Platform: darwin
```

With `--verbose`:
```
homebrew
  Description: Homebrew package manager
  Platform: darwin
  Current system: darwin ✓ Compatible
  Strategy: copy
```

On Linux:
```
homebrew
  Description: Homebrew package manager
  Platform: darwin
  Current system: linux ✗ Incompatible (skipped)
  Strategy: copy
```

### Check Output

```bash
dotm check
```

Validates platform field values:

```
Checking configuration...
✓ Package dependencies
✓ Host and role references
✓ System package configuration
✗ Platform constraints

Error: Package 'foo': invalid platform 'osx'. Valid platforms: linux, darwin, windows
```

With `--warn-suggestions`:
```
Warning: Package 'dev' depends on 'homebrew' (darwin only) but is platform-agnostic
  Consider adding 'platform = "darwin"' to package 'dev' or making the dependency optional
```

## Implementation Details

### Config Changes

```rust
// config.rs
#[derive(Debug, Default, Deserialize)]
pub struct PackageConfig {
    // ... existing fields ...
    pub platform: Option<String>,
}
```

### Platform Detection

```rust
// New module: src/platform.rs

/// Canonical platform names used internally.
const VALID_PLATFORMS: &[&str] = &["linux", "darwin", "windows"];

/// Get the current platform identifier, normalized.
pub fn current_platform() -> &'static str {
    normalize_platform(std::env::consts::OS)
}

/// Normalize platform names: "macos" → "darwin". All others pass through.
pub fn normalize_platform(platform: &str) -> &'static str {
    match platform {
        "macos" | "darwin" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        // Return as-is for unknown values (validation catches these)
        _ => platform.leak(),  // NOTE: only called on config values, bounded set
    }
}

/// Check if a platform string is valid (after normalization).
pub fn is_valid_platform(platform: &str) -> bool {
    VALID_PLATFORMS.contains(&normalize_platform(platform))
}

/// Check if a package is compatible with the current platform.
pub fn is_compatible(pkg_platform: Option<&str>) -> bool {
    match pkg_platform {
        None => true,  // No constraint = available everywhere
        Some(platform) => normalize_platform(platform) == current_platform(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn osx_is_invalid() {
        assert!(!is_valid_platform("osx"));
    }

    #[test]
    fn no_constraint_is_compatible() {
        assert!(is_compatible(None));
    }

    #[test]
    fn matching_platform_is_compatible() {
        // current_platform() is already normalized, so this always works
        assert!(is_compatible(Some(current_platform())));
    }

    #[test]
    fn macos_and_darwin_are_equivalent() {
        assert_eq!(normalize_platform("macos"), normalize_platform("darwin"));
    }
}
```

**Implementation note**: The `leak()` call in `normalize_platform` for unknown values is safe
because this function is only called on config-parsed strings (bounded, small set). In practice,
validation rejects unknown values before they reach `normalize_platform` at runtime. A cleaner
alternative is to return `&str` with lifetime tied to the input for the unknown case, or simply
return `String`. Choose whichever fits the codebase style.

### Orchestrator Changes

```rust
// orchestrator.rs

pub fn deploy(&mut self, hostname: &str, dry_run: bool, force: bool) -> Result<DeployReport> {
    // ... existing code ...

    // After resolving packages, before scanning:
    let compatible_packages: Vec<String> = pkg_names
        .into_iter()
        .filter(|pkg_name| {
            let pkg_config = self.loader.root().packages.get(pkg_name.as_str());
            match pkg_config {
                Some(cfg) => {
                    let compat = crate::platform::is_compatible(cfg.platform.as_deref());
                    if !compat && !dry_run {
                        eprintln!("Skipped (platform): {pkg_name} ({} only)",
                                  cfg.platform.as_deref().unwrap_or("unknown"));
                    }
                    compat
                }
                None => true,  // Unknown package, let resolver handle error
            }
        })
        .collect();

    // Continue with compatible_packages instead of pkg_names
    // ... rest of existing code ...
}
```

### Validation Changes

```rust
// config.rs

pub fn validate_platforms(root: &RootConfig) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, pkg) in &root.packages {
        if let Some(platform) = &pkg.platform {
            if !crate::platform::is_valid_platform(platform) {
                errors.push(format!(
                    "Package '{name}': invalid platform '{platform}'. Valid platforms: linux, darwin (or macos), windows"
                ));
            }
        }
    }
    errors
}

// Add call to validate_platforms() in check command handler
```

### List Command Changes

```rust
// list.rs

// Add platform info to package listing
pub fn list_packages(loader: &ConfigLoader, verbose: bool) {
    // ... existing code ...

    if verbose {
        if let Some(platform) = &pkg.platform {
            println!("  Platform: {platform}");
            let current = crate::platform::current_platform();
            let compat = crate::platform::is_compatible(Some(platform));
            if compat {
                println!("  Current system: {current} ✓ Compatible");
            } else {
                println!("  Current system: {current} ✗ Incompatible (skipped)");
            }
        } else {
            println!("  Platform: all");
        }
    }

    // ... rest of existing code ...
}
```

## Testing Strategy

### Unit Tests

```rust
// tests/platform.rs

#[test]
fn current_platform_returns_valid_value() {
    let platform = dotm::platform::current_platform();
    assert!(dotm::platform::is_valid_platform(platform));
}

#[test]
fn no_platform_constraint_is_compatible() {
    assert!(dotm::platform::is_compatible(None));
}

#[test]
fn matching_platform_is_compatible() {
    let current = dotm::platform::current_platform();
    assert!(dotm::platform::is_compatible(Some(current)));
}

#[test]
fn non_matching_platform_is_incompatible() {
    let current = dotm::platform::current_platform();
    let other = if current == "darwin" { "linux" } else { "darwin" };
    assert!(!dotm::platform::is_compatible(Some(other)));
}

#[test]
fn validate_rejects_invalid_platform() {
    let root = make_root_with_package("foo", Some("invalid"));
    let errors = validate_platforms(&root);
    assert!(!errors.is_empty());
    assert!(errors[0].contains("invalid platform"));
}

#[test]
fn validate_accepts_valid_platforms() {
    for platform in &["linux", "darwin", "macos", "windows"] {
        let root = make_root_with_package("foo", Some(platform));
        let errors = validate_platforms(&root);
        assert!(errors.is_empty(), "platform '{platform}' should be valid");
    }
}
```

### Integration Tests

```rust
// tests/e2e.rs or new tests/platform.rs

#[test]
fn deploy_skips_incompatible_packages() {
    // Create fixture with linux-only and darwin-only packages
    // Run deploy on current platform
    // Verify only compatible packages deployed
    // Verify incompatible packages skipped
}

#[test]
fn list_shows_platform_compatibility() {
    // Create fixture with platform-specific packages
    // Run dotm list packages
    // Verify output shows platform info
}

#[test]
fn check_validates_platform_field() {
    // Create fixture with invalid platform value
    // Run dotm check
    // Verify error reported
}

#[test]
fn platform_agnostic_package_deploys_everywhere() {
    // Create package without platform field
    // Verify it would deploy on any platform (test on current)
}
```

## Validation Rules

Add to `config::validate_platforms()`:

| Rule | Error Message |
|------|---------------|
| `platform` is not one of: `linux`, `darwin`/`macos`, `windows` | `Package '{name}': invalid platform '{value}'. Valid platforms: linux, darwin (or macos), windows` |

Add to `dotm check --warn-suggestions`:

| Condition | Warning Message |
|-----------|-----------------|
| Package with no platform constraint depends on platform-specific package | `Package '{pkg}' depends on '{dep}' ({platform} only) but is platform-agnostic` |

## Migration Strategy

### Existing Dotfiles

1. No changes required for existing packages (backward compatible)
2. Gradually add `platform` fields to OS-specific packages:
   ```bash
   # Identify Linux-specific packages
   grep -l "KDE\|Plasma\|gamemode" packages/*/dotm.toml

   # Add platform = "linux" to each
   ```

3. Simplify host configs:
   ```toml
   # Before (manual exclusion)
   roles = ["base", "macos"]

   # After (automatic filtering)
   roles = ["base", "desktop"]  # KDE auto-skipped on macOS
   ```

### Adding New Platform-Specific Packages

Always declare platform when creating OS-specific packages:

```bash
dotm init alfred
cat >> dotm.toml << 'EOF'
[packages.alfred]
description = "Alfred preferences"
platform = "darwin"
EOF
```

## Implementation Checklist

### Config Changes
- [ ] Add `platform: Option<String>` to `PackageConfig` in `config.rs`
- [ ] Add `validate_platforms()` function in `config.rs`

### New Module
- [ ] Create `src/platform.rs` with platform detection and validation
- [ ] Export from `src/lib.rs`

### Orchestrator Changes
- [ ] Filter packages by platform in `deploy()` before scanning
- [ ] Track skipped packages for reporting
- [ ] Add platform skip messages to output

### List Command Changes
- [ ] Add platform field to package listing
- [ ] Add compatibility indicator in verbose mode

### Check Command Changes
- [ ] Call `validate_platforms()` in check command
- [ ] Add warning for cross-platform dependencies with `--warn-suggestions`

### Testing
- [ ] Write unit tests in `src/platform.rs` (inline `mod tests`)
- [ ] Write validation tests in `tests/config_parsing.rs`
- [ ] Write integration tests in `tests/e2e.rs` or new `tests/platform.rs`

### Documentation
- [ ] Update README.md with platform filtering section
- [ ] Add examples of platform-specific packages
- [ ] Update CHANGELOG.md

## Future Enhancements

Out of scope for initial implementation:

1. **Platform aliases**: `platform = "unix"` matching both Linux and macOS
2. **Multi-platform support**: `platforms = ["linux", "darwin"]` array syntax
3. **Version constraints**: `platform = "darwin >= 13"` for OS version filtering
4. **Architecture filtering**: `arch = "aarch64"` for ARM-specific packages
5. **Platform-specific dependencies**: `depends_linux = ["systemd"]` syntax
6. **Conditional files within packages**: `file##platform.linux` suffix
7. **Runtime platform detection**: Query actual OS features instead of just name

## Open Questions

1. ~~Should we support "macos" as alias for "darwin"?~~
   - **Decision**: Yes. Accept both `"darwin"` and `"macos"`, normalize to `"darwin"` internally. Users shouldn't need to know kernel names.

2. ~~How to handle cross-platform dependencies?~~
   - **Decision**: Silent skip (Option A). Packages can depend on platform-specific deps.

3. ~~Should platform filtering apply to system packages?~~
   - **Decision**: Yes, same behavior for user and system packages.

4. ~~Should we warn about unused platforms in check?~~
   - **Decision**: No. Package may be used on multiple hosts with different platforms.

## References

- Rust platform detection: `std::env::consts::OS`
- Existing package filtering: `orchestrator.rs` system package filtering
- Validation pattern: `config::validate_system_packages()`
- Terminal output: `status.rs` colored output, `orchestrator.rs` deploy messages
