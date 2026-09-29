#!/bin/sh
set -eu

# tripwire-regression: Four-step guard proof harness proving gates bite
# 1. plant violation -> 2. observe failure -> 3. revert -> 4. observe clean

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

echo "==> [tripwire-regression] Testing non-exhaustive-check gate bite in pardosa..."
# 1. Plant violation
printf '\n#[derive(thiserror::Error, Debug)]\n#[error("planted")]\n#[non_exhaustive]\npub enum PlantedError {}\n' >> "$root/crates/pardosa/src/lib.rs"

# 2. Observe failure
if sh scripts/check.sh >/dev/null 2>&1; then
  git checkout "$root/crates/pardosa/src/lib.rs"
  echo "::error::tripwire-regression: gate failed to bite on planted #[non_exhaustive]" >&2
  exit 1
fi

# 3. Revert
git checkout "$root/crates/pardosa/src/lib.rs" >/dev/null 2>&1

# 4. Observe clean
sh scripts/check.sh >/dev/null 2>&1 || {
  echo "::error::tripwire-regression: gate failed to pass after revert" >&2
  exit 1
}

echo "OK: tripwire-regression all guard proofs verified clean."
