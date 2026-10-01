#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

echo "==> Running spec-coverage check..."
cargo run --quiet --manifest-path "$ROOT/scripts/Cargo.toml" \
    --bin spec-coverage -- --allow-regime-prose C5.1

echo "==> Running closed-error gate check (non-exhaustive-check / RST-0006)..."
cargo run --quiet --manifest-path "$ROOT/scripts/Cargo.toml" \
    --bin non-exhaustive-check -- \
    "$ROOT/crates/pardosa/src" \
    "$ROOT/crates/pardosa-derive/src" \
    "$ROOT/crates/pardosa-nats/src"

echo "==> Running unsafe-code gate check (forbid-unsafe-total / RST-0005)..."
# Verify crate roots forbid unsafe code
for root_file in "$ROOT"/crates/*/src/lib.rs; do
    if [ -f "$root_file" ]; then
        ! grep -rn '\bunsafe\b' "$root_file" >/dev/null 2>&1 || {
            echo "::error::forbid-unsafe-total: unsafe code found in $root_file (RST-0005)" >&2
            exit 1
        }
    fi
done

echo "==> Running cargo test..."
cargo test --workspace --locked

echo "==> Running cargo clippy..."
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "==> Running cargo fmt check..."
cargo fmt --all -- --check

echo "==> All pardosa verification checks passed."
