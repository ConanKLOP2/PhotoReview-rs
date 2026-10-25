#!/usr/bin/env bash
set -u
SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SELF_DIR/../../.." && pwd)"
PY="${PY_BIN:-python}"
input=$(cat)
file=$(printf '%s' "$input" | "$PY" -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("file_path", ""))' 2>/dev/null) || exit 0
case "$file" in *.rs) ;; *) exit 0;; esac
case "$(cd "$(dirname "$file")" 2>/dev/null && pwd)/$(basename "$file")" in
  "$REPO_ROOT"/*) ;;
  *) exit 0;;
esac
RUSTFMT="${RUSTFMT_BIN:-rustfmt}"
if [ -f "$REPO_ROOT/rustfmt.toml" ]; then
  timeout 20 "$RUSTFMT" --edition 2024 --config-path "$REPO_ROOT/rustfmt.toml" "$file" >/dev/null 2>&1 || :
else
  timeout 20 "$RUSTFMT" --edition 2024 "$file" >/dev/null 2>&1 || :
fi
exit 0
