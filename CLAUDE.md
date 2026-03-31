# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

dotm is a dotfile manager written in Rust (edition 2024) that deploys configuration files via symlinks and copies, with composable roles, Tera templates, host-specific overrides, and dependency resolution.

The crate is published as `dotm-rs` but the library name and binary are both `dotm`. The project is structured as a lib+bin crate (`src/lib.rs` re-exports all modules, `src/main.rs` handles CLI).

## Build & Development Commands

```bash
just test              # cargo test
just lint              # cargo clippy -- -D warnings
just check             # test + lint combined
just build             # cargo build --release
just install           # cargo install --path .
just run <ARGS>        # cargo run -- <ARGS>
just release <VER>     # bump Cargo.toml, tag, and push (e.g. just release 1.1.0)

# Run a single test by name
cargo test <test_name>

# Run a specific integration test file
cargo test --test e2e
cargo test --test orchestrator
```

Clippy is run with `-D warnings` (all warnings are errors). Requires Rust 1.87+ (edition 2024 features like let chains are used).

## Architecture

### Deployment Pipeline

The deploy flow is orchestrated by `orchestrator.rs` in five phases:

1. **Scan** (`scanner.rs`) — Recursively collects files in each package, groups them by canonical path (stripping `##` suffixes and `.tera` extensions), and selects the highest-priority variant per file
2. **Collision detection** — Checks that no two packages deploy to the same staged path
3. **State loading** (`state.rs`) — Loads previous deployment state for drift detection
4. **Deploy** (`deployer.rs`) — Deploys each file using one of two strategies:
   - **Stage** (default): copy/render to `.staged/`, symlink target → `.staged/`
   - **Copy**: copy directly to target
5. **State save** — Persists deployment state (target, staged path, source, SHA256 hash, kind, package) to JSON

### Configuration Hierarchy

- `dotm.toml` — Root config declaring packages, loaded by `loader.rs`
- `hosts/<hostname>.toml` — Host config selecting roles, with optional vars
- `roles/<name>.toml` — Role config listing packages, with optional vars
- Variable precedence: host vars > role vars (last listed role wins among roles), merged by `vars.rs`
- Dependency resolution with circular dependency detection in `resolver.rs`

### File Override Priority

| Pattern | Priority | Kind | Deployed as |
|---------|----------|------|-------------|
| `file##host.<hostname>` | 1 (highest) | Override | Copy |
| `file##role.<rolename>` | 2 | Override | Copy |
| `file.tera` | 3 | Template | Rendered & copy |
| `file` | 4 (lowest) | Base | Symlink |

Only the highest-priority matching variant is deployed per canonical path.

### State & Drift Detection

State is persisted at `$XDG_STATE_HOME/dotm/dotm-state.json`. Each entry tracks a SHA256 hash of deployed content. On re-deploy, if the staged file's hash differs from the recorded hash, it's flagged as modified (skip unless `--force`). State also powers `undeploy` and `status`.

### Key Modules

| Module | Responsibility |
|--------|---------------|
| `main.rs` | CLI (clap derive), command handlers, state dir resolution |
| `orchestrator.rs` | Central deploy/undeploy coordination |
| `scanner.rs` | File discovery and override resolution |
| `deployer.rs` | Stage/copy deployment strategies |
| `state.rs` | State persistence, drift detection, undeploy |
| `status.rs` | Status rendering (default/verbose/short modes, colored output) |
| `adopt.rs` | Interactive hunk-by-hunk acceptance of drifted changes |
| `template.rs` | Tera template rendering with TOML vars → Tera context |
| `config.rs` | Data model definitions (RootConfig, HostConfig, RoleConfig, etc.) |
| `diff.rs` | Unified diff formatting using `similar` crate |
| `git.rs` | Git operations (commit, push, pull, status) via `gix` + `git` CLI |
| `hash.rs` | SHA-256 hashing for content and files |
| `hooks.rs` | Pre/post deploy/undeploy shell hook execution |
| `list.rs` | Rendering for `list packages/roles/hosts` commands |
| `metadata.rs` | File ownership/permissions resolution and application |

### Testing

Integration tests live in `tests/` with fixtures in `tests/fixtures/`. Tests use `tempfile` for isolated temporary directories and copy fixture trees into them. CLI tests in `tests/cli.rs` use `assert_cmd` + `predicates` for binary-level testing. The fixtures include `basic/` (simple deploy) and `overrides/` (host/role overrides and templates) scenarios. Several modules also have `#[cfg(test)] mod tests` inline unit tests.

## Conventions

- Error handling uses `anyhow::Result` throughout (application, not library)
- Terminal coloring via `crossterm`, respects `NO_COLOR` env var and tty detection
- Status command uses exit codes: 0 = all ok, 1 = problems detected
