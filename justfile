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
