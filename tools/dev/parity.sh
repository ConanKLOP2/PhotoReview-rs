#!/usr/bin/env bash
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PY_BIN=${PY_BIN:-python}
exec "$PY_BIN" -I "$ROOT/tools/dev/parity.py" "$@"
