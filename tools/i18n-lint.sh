#!/bin/sh
# Localization lint (platform/docs/localization.md):
#  1. the ratchet: no UI crate gains raw user-visible string literals beyond
#     platform/tools/i18n-literals.baseline (new texts go through t!/tf! and the English bundles (assets/lang/en of any part));
#  2. every key the code uses exists in the English bundle, every bundle is free of
#     errors (placeholders, plural forms, references).
# Usage: sh platform/tools/i18n-lint.sh            check
#        sh platform/tools/i18n-lint.sh --update   lower the baseline to the current counts
set -e
ROOT="${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
cd "$ROOT"

if [ "${1:-}" = "--update" ]; then
  { echo "# Raw user-visible string literals per file (platform/tools/i18n-scan.py). Only goes down."; python3 platform/tools/i18n-scan.py --print; } > platform/tools/i18n-literals.baseline
  echo "i18n-lint: baseline updated"
  exit 0
fi

python3 platform/tools/i18n-scan.py --check
# In one repository of the split (AURELIA_ROOT = the folder that holds platform/ and lands/ side by side)
# there is no workspace at the root: use the platform's own.
MANIFEST=""; [ -f Cargo.toml ] || MANIFEST="--manifest-path platform/Cargo.toml"
cargo ${CARGO_TOOLCHAIN:-+1.98.1} run -q --release $MANIFEST -p aurelia-i18n --bin i18n-check -- keys
cargo ${CARGO_TOOLCHAIN:-+1.98.1} run -q --release $MANIFEST -p aurelia-i18n --bin i18n-check -- lint
