#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CDPATH= cd -- "$root"

cargo run --quiet --manifest-path "$root/scripts/Cargo.toml" \
    --bin spec-coverage -- --allow-regime-prose C5.1

cargo run --quiet --manifest-path "$root/scripts/Cargo.toml" \
    --bin non-exhaustive-check -- \
    "$root/crates/pardosa/src" \
    "$root/crates/pardosa-derive/src" \
    "$root/crates/pardosa-nats/src"

# forbid-unsafe-total gate wiring (RST-0005)
# Verify crate roots forbid unsafe code
for root_file in "$root"/crates/*/src/lib.rs; do
    if [ -f "$root_file" ]; then
        ! grep -rn '\bunsafe\b' "$root_file" >/dev/null 2>&1 || {
            echo "::error::forbid-unsafe-total: unsafe code found in $root_file (RST-0005)" >&2
            exit 1
        }
    fi
done

