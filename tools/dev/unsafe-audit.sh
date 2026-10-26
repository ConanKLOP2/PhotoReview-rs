#!/usr/bin/env bash
# unsafe-audit.sh [--update-baseline]: read-only scan of crates/**/src and crates/**/tests for unsafe hygiene.
# Env: PY_BIN (default python), UNSAFE_AUDIT_ROOT, UNSAFE_AUDIT_BASELINE, UNSAFE_ALLOWED_CRATES.
set -u
ROOT="${UNSAFE_AUDIT_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY_BIN=${PY_BIN:-python}
BASELINE="${UNSAFE_AUDIT_BASELINE:-$HERE/unsafe-audit.baseline}"
case "${1:-}" in ""|--update-baseline) ;; *) echo 'usage: unsafe-audit.sh [--update-baseline]' >&2; exit 2 ;; esac
exec "$PY_BIN" -I "$HERE/unsafe-audit.py" "$ROOT" "$BASELINE" ${1:+"$1"}
