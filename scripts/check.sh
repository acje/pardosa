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

