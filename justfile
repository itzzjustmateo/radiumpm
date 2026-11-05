set shell := ["bash", "-cu"]

default: check

# Format all Rust sources.
fmt:
    cargo fmt --all

# Verify formatting without changing files.
fmt-check:
    cargo fmt --all -- --check

# Run Clippy with warnings treated as errors.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run the full test suite.
test:
    cargo test --all-features

# Everything CI checks.
check: fmt-check lint test

# Build an optimized binary.
build:
    cargo build --release --locked

# Run Radium with arbitrary arguments, for example: just run add typescript
run *args:
    cargo run -- {{args}}

# Show environment diagnostics for the current project.
doctor:
    cargo run -- doctor

clean:
    cargo clean
