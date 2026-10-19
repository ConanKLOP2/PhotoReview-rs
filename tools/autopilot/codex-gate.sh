#!/usr/bin/env bash
# Machine checks for one unit. Exit 0 = pass; non-zero = short error tail (<= 80 lines) on stdout.
# Usage: codex-gate.sh <unit> <worktree> <crate>      (set CARGO_TARGET_DIR before calling)
set -u
U="${1:?unit}"; WT="${2:?worktree}"; CRATE="${3:?crate}"
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="python $SELF/lib/codex-lib.py"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}" CARGO_INCREMENTAL=0
cd "$WT" || { echo "gate: no worktree $WT"; exit 9; }
fail() { # step exit-code file
  { echo "GATE FAIL: $1"; cat "$3"; } | tail -n 80; rm -f "$3"; exit "$2"; }
T="$(mktemp)"

# 1. changed files must be inside the contract's owned files (tracked changes + untracked, ignoring .codex-* scratch)
CONTRACT="$WT/contracts/$U.md"; [ -f "$CONTRACT" ] || CONTRACT="$SELF/../../contracts/$U.md"
$LIB owned "$CONTRACT" | tr -d '\r' | sort -u > "$T.owned"
git status --porcelain -uall | sed -e 's/^...//' -e 's/.* -> //' -e 's/^"//' -e 's/"$//' | grep -v '^\.codex-' | sort -u > "$T.changed"
comm -13 "$T.owned" "$T.changed" > "$T"
if [ -s "$T" ]; then
  { echo "Files changed outside the contract's Owned files (revert them: git checkout -- <file> / delete untracked):"; cat "$T"; } > "$T.m"
  rm -f "$T" "$T.owned" "$T.changed"; fail "owned-files check" 1 "$T.m"
fi
rm -f "$T.owned" "$T.changed"

cargo fmt --check > "$T" 2>&1 || fail "cargo fmt --check (run cargo fmt on the owned files)" 2 "$T"
cargo clippy -p "$CRATE" --all-targets -- -D warnings > "$T" 2>&1 || fail "cargo clippy -p $CRATE" 3 "$T"
cargo test -p "$CRATE" > "$T" 2>&1 || {
  # keep failing-test names and assertion output, drop the compile noise
  { grep -nE '^(---- |failures:|test .*FAILED|thread .*panicked|error(\[|:)|  left:|  right:|assertion)' "$T" | head -60; echo ...; tail -n 20 "$T"; } > "$T.f"
  rm -f "$T"; fail "cargo test -p $CRATE" 4 "$T.f"; }
rm -f "$T"; echo "gate ok: $U"
