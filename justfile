# Every command humans, agents, and CI use. Run `just` to list them.

set shell := ["bash", "-euo", "pipefail", "-c"]
export COREPACK_ENABLE_DOWNLOAD_PROMPT := "0"

web := "pnpm --dir web"

default:
    @just --list --unsorted

# One-time setup after cloning (or in a new worktree).
setup:
    {{web}} install --frozen-lockfile
    {{web}} exec lefthook install
    cargo fetch --locked

# Run the Rust server (API on 127.0.0.1:8640).
run:
    cargo run --locked -p saffron-server

# Run the web dev server (proxies /api to the Rust server).
dev-web:
    {{web}} dev

# Format everything.
fmt:
    cargo fmt --all
    {{web}} format

# Check formatting without writing.
fmt-check:
    cargo fmt --all --check
    {{web}} format:check

# Lint Rust (clippy, warnings are errors) and web (oxlint).
lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings
    {{web}} lint

# Type-check the web app.
typecheck:
    {{web}} typecheck

# Run all tests.
test:
    cargo nextest run --workspace --locked --no-tests=warn
    {{web}} test

# Opt-in tests that call live market data providers over the internet.
live:
    cargo nextest run --workspace --locked --run-ignored only -E 'binary(live)'

# Dependency policy: licenses, advisories, sources.
deny:
    cargo deny check

# Production build: the web bundle, then the binary that embeds it.
build:
    {{web}} build
    cargo build --release --locked

# The full gate. CI runs exactly this; run it before every push.
gate: fmt-check typecheck lint test deny build
