#!/usr/bin/env bash
# Machine checks for one unit. Exit 0 = pass; non-zero = short error tail (<= 60 lines) on stdout. No LLM is involved.
# Usage: codex-gate.sh <unit> <worktree> <crate> [fast|full]      (set CARGO_TARGET_DIR before calling)
#   always: owned-files check, static review gates (unwrap/unsafe/todo/allow/.claude), red-test guard (if GATE_TEST_BASE is set)
#   fast  : cargo fmt -p <crate> -- --check (lib/fmtcheck.sh, never the whole workspace: Windows os error 206), cargo check -p <crate> --all-targets, then ONLY the unit's own test binaries (repair loops)
#   full  : the same fmt check, cargo clippy -p <crate> --all-targets -D warnings, then the unit's own test binaries (the whole crate is tested by the final workspace gate, where other units' stubs may still be red)
# Env: GATE_TEST_BASE=<rev> (commit holding the red tests), GATE_NEXTEST=auto|0|1 (use cargo-nextest when installed).
set -u
U="${1:?unit}"; WT="${2:?worktree}"; CRATE="${3:?crate}"; MODE="${4:-full}"
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="python $SELF/lib/codex-lib.py"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-line-tables-only}" CARGO_INCREMENTAL=0
cd "$WT" || { echo "gate: no worktree $WT"; exit 9; }
fail() { # step exit-code file
  { echo "GATE FAIL: $1"; cat "$3"; } | tail -n 60; rm -f "$3"; exit "$2"; }
T="$(mktemp)"

# 1. changed files must be inside the contract's owned files (tracked changes + untracked, ignoring .codex-* scratch)
CONTRACT="$WT/contracts/$U.md"; [ -f "$CONTRACT" ] || CONTRACT="$SELF/../../contracts/$U.md"
$LIB owned "$CONTRACT" | tr -d '\r' | sort -u > "$T.owned"
git status --porcelain -uall | sed -e 's/^...//' -e 's/.* -> //' -e 's/^"//' -e 's/"$//' | grep -v -E '^(\.codex-|target)' | sort -u > "$T.changed"
comm -13 "$T.owned" "$T.changed" > "$T"
if [ -s "$T" ]; then
  { echo "Files changed outside the contract's Owned files (revert them: git checkout -- <file> / delete untracked):"; cat "$T"; } > "$T.m"
  rm -f "$T" "$T.owned" "$T.changed"; fail "owned-files check" 1 "$T.m"
fi

# 2. cheap deterministic review gates
$LIB lint "$WT" "$T.owned" > "$T.l" 2>&1 || { rm -f "$T" "$T.owned" "$T.changed"; fail "static review gate" 5 "$T.l"; }
if [ -n "${GATE_TEST_BASE:-}" ]; then
  $LIB testguard "$WT" "$GATE_TEST_BASE" "$T.owned" > "$T.l" 2>&1 || { rm -f "$T" "$T.owned" "$T.changed"; fail "red-test guard" 6 "$T.l"; }
fi
rm -f "$T.l"

bash "$SELF/lib/fmtcheck.sh" -p "$CRATE" > "$T" 2>&1 || { rm -f "$T.owned" "$T.changed"; fail "cargo fmt -p $CRATE -- --check (run cargo fmt -p $CRATE)" 2 "$T"; }

NX=0
if [ "${GATE_NEXTEST:-auto}" != 0 ] && cargo nextest --version >/dev/null 2>&1; then NX=1; fi
testfail() { # label
  { grep -nE '^(---- |failures:|test .*FAILED|thread .*panicked|error(\[|:)|  left:|  right:|assertion|     FAIL|        FAIL)' "$T" | head -40; echo ...; tail -n 15 "$T"; } > "$T.f"
  rm -f "$T" "$T.owned" "$T.changed"; fail "$1" 4 "$T.f"; }

if [ "$MODE" = fast ]; then
  cargo check -p "$CRATE" --all-targets > "$T" 2>&1 || { rm -f "$T.owned" "$T.changed"; fail "cargo check -p $CRATE --all-targets" 3 "$T"; }
  TA="$($LIB testargs "$WT" "$CONTRACT" | tr -d '\r')"
  if [ -z "$TA" ]; then TA=""; fi
  if [ "$NX" = 1 ]; then
    # nextest has no --lib/--test mix problem: pass them through
    cargo nextest run -p "$CRATE" --no-tests=pass $TA > "$T" 2>&1 || testfail "cargo nextest run -p $CRATE $TA"
  else
    cargo test -p "$CRATE" $TA > "$T" 2>&1 || testfail "cargo test -p $CRATE $TA"
  fi
else
  TA="$($LIB testargs "$WT" "$CONTRACT" | tr -d '\r')"
  cargo clippy -p "$CRATE" --all-targets -- -D warnings > "$T" 2>&1 || { rm -f "$T.owned" "$T.changed"; fail "cargo clippy -p $CRATE" 3 "$T"; }
  if [ "$NX" = 1 ]; then
    { cargo nextest run -p "$CRATE" --no-tests=pass $TA; } > "$T" 2>&1 || testfail "cargo nextest run -p $CRATE $TA"
  else
    cargo test -p "$CRATE" $TA > "$T" 2>&1 || testfail "cargo test -p $CRATE $TA"
  fi
fi
rm -f "$T" "$T.owned" "$T.changed"; echo "gate ok ($MODE): $U"
