# Install frontend dependencies
install:
    bun install

# Launch app in dev mode with hot-reload
dev:
    cargo tauri dev

# Build production .app bundle (macOS DMG)
build:
    cargo tauri build

# Run Rust unit tests
test:
    cargo test --manifest-path src-tauri/Cargo.toml

# Type-check Rust without building
check:
    cargo check --manifest-path src-tauri/Cargo.toml

# Type-check TypeScript
check-ts:
    bunx tsc --noEmit -p tsconfig.app.json

# Type-check both Rust and TypeScript
check-all: check check-ts

# Format Rust code (nightly — rustfmt.toml uses nightly-only options)
fmt:
    cargo +nightly fmt --manifest-path src-tauri/Cargo.toml

# Verify Rust formatting without writing — mirrors CI. Nightly because
# rustfmt.toml uses nightly-only options (run `just fmt` to fix).
fmt-check:
    cargo +nightly fmt --manifest-path src-tauri/Cargo.toml --check
