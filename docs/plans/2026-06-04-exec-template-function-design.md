# Design: `exec()` Template Function

**Issue:** #14 (partial — exec function only, env file loading deferred)
**Date:** 2026-06-04

## Overview

Add a Tera template function `exec()` that runs a shell command and substitutes its
stdout into the template at render time.

## Usage

```jinja
export GPG_KEY="{{ exec(command='gpg --list-keys --keyid-format long | head -1') }}"
git_email = {{ exec(command="git config user.email") }}
ssh_key_hash = {{ exec(command="ssh-keygen -lf ~/.ssh/id_ed25519 | awk '{print $2}'") }}
```

## Behavior

- Runs via `$SHELL -c "<command>"` (falls back to `/bin/sh` if `$SHELL` is unset)
- Captures **stdout only** — stderr passes through to the terminal
- **Auto-trims** trailing whitespace from the output
- On failure (non-zero exit or command not found), the template render **fails** and
  the file is skipped with an error message showing the command and exit code
- First time `exec()` is used in a deploy session, a **warning** is printed once:
  *"Template uses exec() to run shell commands — review templates if this repo isn't yours"*

## Implementation

### Changes

**`src/template.rs` only** — no new modules, no config changes, no new dependencies.

1. Add an `ExecFunction` struct implementing `tera::Function`
2. Register it on the Tera instance in `render_template()` before calling `tera.render()`

### ExecFunction

- Extracts the required `command` string argument
- On first invocation per process, prints warning to stderr (`AtomicBool` gate)
- Reads `$SHELL`, falls back to `/bin/sh`
- Runs `std::process::Command::new(shell).arg("-c").arg(&command).output()`
- Non-zero exit → `Err(tera::Error::msg(...))`
- Success → trim trailing whitespace from stdout, return `Ok(Value::String(...))`

### Testing

- **Unit:** `{{ exec(command="echo hello") }}` renders to `"hello"` (trimmed)
- **Unit:** non-zero exit → render returns error
- **Unit:** missing `command` arg → render returns error
- **Integration:** `.tera` fixture using `exec()` deploys correctly in e2e tests
