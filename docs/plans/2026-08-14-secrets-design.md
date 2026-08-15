# Design: GPG-Backed Secrets

**Date:** 2026-08-14

## Overview

Add secrets support to dotm using GPG as the encryption backend, shelling out to the
system `gpg` binary (no new crypto crates — same external-process pattern as
`git.rs`). Two complementary mechanisms:

1. **Whole-file secrets** — a `.gpg` suffix on a file in a package decrypts to the
   base filename on deploy (e.g. `packages/shell/.netrc.gpg` → `.netrc`).
2. **Secret template vars** — `roles/<name>.secrets.toml.gpg` and
   `hosts/<hostname>.secrets.toml.gpg` decrypt to TOML and merge into the template
   vars context under a `secrets.*` namespace, usable from any `.tera` template.

Recipients (who secrets are encrypted *for*) are declared once in `dotm.toml`.
Decryption always relies on the user's local `gpg` keyring/agent — dotm never
manages or stores private key material.

## Non-goals

- No support for other backends (age, sops, external secret managers) in this pass.
- No redaction of secret content in `dotm diff` / `dotm status --verbose` — a
  drifted secret is diffed exactly like a drifted template (plaintext unified diff
  on screen). This is an accepted, deliberate trade-off for v1 simplicity; revisit
  if it becomes a real leak concern.
- No plaintext-never-touches-disk guarantee for `dotm secrets edit` beyond
  restrictive permissions + best-effort shred; a determined attacker with disk
  forensic access is out of scope.

## Configuration

```toml
# dotm.toml
[secrets]
recipients = ["alice@example.com", "0xABCD1234..."]
```

- Used only for **encryption** (`dotm secrets encrypt/edit/rekey`).
- `check` errors if any `.gpg` file exists under `packages/`, `roles/`, or `hosts/`
  while `[secrets].recipients` is empty.

## Whole-file secrets

### Naming & scanning

- `EntryKind::Secret` is added alongside `Base` / `Override` / `Template`.
- `canonical_target_path` strips a trailing `.gpg` the same way it already strips
  `.tera`.
- `.gpg` combines with `##host.<hostname>` / `##role.<rolename>` suffixes exactly
  like `.tera` does today (e.g. `.netrc.gpg##host.laptop` is a host-specific
  encrypted override). Priority order in `resolve_variant` is extended
  consistently: host-secret > role-secret > secret > (existing tera/override/base
  tiers), following the same suffix-then-extension stripping already used for
  templates.

### Deployment

- `Secret` is never symlinked. In `orchestrator.rs`, the existing
  `use_symlink = !is_system && (kind == Base || kind == Override)` check already
  routes `Secret` through the copy path with no changes needed.
- In `deployer::deploy_copy`, a new `EntryKind::Secret` match arm shells out to
  `gpg --decrypt <source>`, writes the resulting plaintext bytes to `target_path`.
- Immediately after writing, the target's permissions are forced to `0600`,
  *before* any package-level `permissions`/`ownership` resolution runs (which can
  still explicitly override it afterward if configured for that path).

### Drift detection & state

- Content hashing is unchanged: `Secret` is a non-symlink copy kind, so
  `hash::hash_file(&target_path)` already hashes the decrypted plaintext at the
  target, consistent with how templates are hashed post-render.
- Only the SHA256 hash is ever written to `dotm-state.json` — no plaintext. State
  stays safe to commit/share.
- `dotm diff` / `dotm status --verbose` / `dotm adopt` treat `Secret` identically
  to `Template` — no special-casing, including for unified diff output (see
  Non-goals).

## Secret template vars

### File layout

- `roles/<name>.secrets.toml.gpg` — optional, per-role secret vars.
- `hosts/<hostname>.secrets.toml.gpg` — optional, per-host secret vars.
- Absent file ⇒ empty map, not an error.

### Loading

- `loader.rs` gains `load_role_secrets(name)` / `load_host_secrets(hostname)`:
  read the `.gpg` file if present, shell out to `gpg --decrypt`, parse the
  resulting plaintext as TOML into a `Map<String, Value>`.
- Decrypted secrets vars are cached in-memory per `ConfigLoader` instance for the
  duration of a single run (parallel to how `root` config is loaded once),
  avoiding repeated `gpg` invocations (and repeated agent/passphrase prompts)
  across multiple template renders in one deploy.

### Merge & namespace

`vars::resolve_vars` extends the existing merge chain:

```
merged = {}
for role in host.roles: merged = merge(merged, role.vars)
merged = merge(merged, host.vars)
for role in host.roles: merged = merge(merged, role.secrets)   # NEW
merged = merge(merged, host.secrets)                            # NEW
```

Secrets are inserted under an explicit `secrets` key rather than flattened into
the top-level namespace, so templates write `{{ secrets.stripe_key }}`. This
keeps provenance visible and avoids a plain var silently shadowing (or being
shadowed by) a secret of the same name.

## `dotm secrets` CLI

Three subcommands, generic over any file path — the same commands handle both
whole-file secrets and `*.secrets.toml.gpg` vars files; the only difference is
which naming convention the caller uses for the source path.

```
dotm secrets encrypt <path>     # gpg --encrypt --recipient <r>... -o <path>.gpg <path>
dotm secrets edit <path.gpg>    # decrypt to 0600 tmpfile, open $EDITOR, re-encrypt, shred tmpfile
dotm secrets rekey              # re-encrypt every discovered .gpg file for current recipients
```

- `encrypt`: resolves recipients from `dotm.toml [secrets].recipients`, invokes
  `gpg --encrypt --recipient <r1> --recipient <r2> ... -o <path>.gpg <path>`. Warns
  if the plaintext source is left in place afterward (leaving both would create a
  scanner collision — two variants resolving to the same canonical target).
- `edit`: the primary day-to-day workflow — decrypts to a temp file (`0600`,
  preferring tmpfs when available), opens `$EDITOR`, re-encrypts on save, shreds
  the temp file. Plaintext is never intentionally left on disk in the repo.
- `rekey`: walks all discovered `.gpg` files across `packages/`, `roles/`, and
  `hosts/`, re-encrypting each for the current `[secrets].recipients` list. Needed
  when recipients change (key added/revoked).

## Validation (`dotm check`)

- Error: `[secrets].recipients` is empty while any `.gpg` file exists anywhere
  under `packages/`, `roles/`, or `hosts/`.
- Warning: a `.gpg` secret has a plaintext sibling resolving to the same
  canonical target at the same priority tier (accidental-leak / stale-plaintext
  risk after `encrypt` without cleanup).

## Error handling

- `gpg` binary missing from `PATH`: fail fast with a clear message
  (`"gpg binary not found in PATH — required for secrets support"`), for both
  deploy-time decryption and all `dotm secrets` subcommands.
- Decryption failure (missing/wrong key, no agent, revoked key): surfaces as a
  normal per-file error in the deploy pipeline, aborting that run's remaining
  actions the same way any other file error does today; partial state is still
  saved per the existing Phase 5 logic in `orchestrator.rs`.

## Out of scope for `dotm add`

`dotm add` is unchanged — it copies an existing file into a package as-is. Users
run `dotm secrets encrypt` afterward if they want the added file encrypted.

## Dependencies

None added. `gpg` is invoked via `std::process::Command`, matching the existing
`git.rs` external-process pattern.

## Testing

- **Unit:** extend `scanner.rs` tests for `.gpg` stripping in
  `canonical_target_path`, and for `.gpg` combined with `##host`/`##role` suffixes
  in `resolve_variant`.
- **Integration:** a `secrets` fixture package, with a real GPG keypair generated
  at test-setup time into a temporary `GNUPGHOME` (not the developer's real
  keyring). Covers: deploy decrypts correctly with `0600` permissions on the
  target; drift detection on a modified decrypted target; `rekey` re-encrypts
  correctly for a new recipient list.
- Secrets integration tests are skipped (not hard-failed) when `gpg` is not found
  in `PATH`, via a small presence-check helper, so `cargo test` still passes on
  machines without gpg installed. CI should have `gpg` available so the tests
  actually run there.
