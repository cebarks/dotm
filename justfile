# Run the project
run *ARGS:
    cargo run -- {{ARGS}}

# Run tests
test:
    cargo test

# Run clippy
lint:
    cargo clippy -- -D warnings

# Build release
build:
    cargo build --release

# Run tests and clippy
check:
    cargo test && cargo clippy -- -D warnings

# Install to ~/.cargo/bin
install:
    cargo install --path .

# Tag and push a release (e.g. `just release 0.2.0`)
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! echo "{{VERSION}}" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        echo "error: VERSION must be semver (e.g. 1.2.3)" >&2
        exit 1
    fi
    current=$(grep -m1 '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
    if [ "$current" = "{{VERSION}}" ]; then
        echo "error: Cargo.toml is already at version {{VERSION}}" >&2
        exit 1
    fi
    if git rev-parse "v{{VERSION}}" >/dev/null 2>&1; then
        echo "error: tag v{{VERSION}} already exists" >&2
        exit 1
    fi
    sed -i 's/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
    cargo check --quiet
    git add Cargo.toml Cargo.lock
    git commit -m "release v{{VERSION}}"
    git tag "v{{VERSION}}"
    git push && git push --tags
