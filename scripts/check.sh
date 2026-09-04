#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

exec cargo run --quiet --manifest-path "$root/scripts/Cargo.toml" \
    --bin spec-coverage -- --allow-regime-prose C5.1
