#!/usr/bin/env bash
# Pre-PR checks for the files this branch changed (AGENTS.md "Before opening the PR"). Read-only: it never edits or commits.
# Usage: bash tools/dev/pre-pr.sh [--base REF] [--no-clippy] [--test] [--list]
#   --base REF    compare against REF (default origin/master); changed = merge-base..HEAD plus uncommitted edits
#   --no-clippy   skip clippy (fmt, markers, audits still run)
#   --test        also run `cargo test -p <crate> --locked` per changed crate (bounded by TEST_TIMEOUT, default 1500 s)
#   --list        print the changed files, the affected crates and the checks that would run, then exit
# Exit 0 = every check that ran passed; 1 = a check failed; 2 = usage/environment error. The last block lists what was NOT run,
# to paste into the PR body. Env: CARGO_TARGET_DIR (default <repo>/target-prepr: delete it when done), PY_BIN.
set -u
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"
BASE=origin/master; CLIPPY=1; TEST=0; LIST=0
while [ $# -gt 0 ]; do
  case "$1" in
    --base) [ $# -ge 2 ] || { echo "pre-pr: --base needs a ref" >&2; exit 2; }; BASE=$2; shift 2;;
    --no-clippy) CLIPPY=0; shift;;
    --test) TEST=1; shift;;
    --list) LIST=1; shift;;
    -h|--help) sed -n 2,10p "$0" | sed 's/^# \{0,1\}//'; exit 0;;
    *) echo "pre-pr: unknown option $1" >&2; exit 2;;
  esac
done
PY=${PY_BIN:-}; [ -n "$PY" ] || { if command -v python >/dev/null 2>&1; then PY=python; else PY=python3; fi; }
MB=$(git merge-base "$BASE" HEAD 2>/dev/null) || { echo "pre-pr: no merge-base with $BASE (git fetch?)" >&2; exit 2; }
FILES=$( { git diff --name-only --diff-filter=ACMR "$MB" HEAD; git diff --name-only --diff-filter=ACMR HEAD; git ls-files --others --exclude-standard; } | sort -u | grep -v '^$' )
[ -n "$FILES" ] || { echo "pre-pr: nothing changed against $BASE"; exit 0; }
CRATES=$(printf '%s\n' "$FILES" | sed -n 's#^crates/\([^/]*\)/.*#\1#p' | sort -u | while read -r c; do [ -f "crates/$c/Cargo.toml" ] && echo "$c"; done)
has() { printf '%s\n' "$FILES" | grep -qE "$1"; }
RS=0; MD=0; SH=0; SK=0; LOCK=0
has '\.rs$' && RS=1; has '\.md$' && MD=1; has '\.sh$' && SH=1
has '^(\.claude|\.agents)/(skills|agents)/' && SK=1
has '(^|/)Cargo\.(toml|lock)$' && LOCK=1
echo "pre-pr: base $BASE ($(printf '%s\n' "$FILES" | wc -l | tr -d ' ') changed files); crates: ${CRATES:-none}" | tr '\n' ' '; echo
if [ "$LIST" = 1 ]; then printf '%s\n' "$FILES"; exit 0; fi

FAILED=0; SKIPPED=""
pass() { echo "ok   $1"; }
fail() { echo "FAIL $1"; FAILED=$((FAILED+1)); }
skip() { echo "skip $1"; SKIPPED="$SKIPPED$1"$'\n'; }
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target-prepr}"
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0

# 1. merge-conflict markers in changed text files
bad=$(printf '%s\n' "$FILES" | while read -r f; do [ -f "$f" ] && grep -InE '^(<<<<<<<|>>>>>>>) ' "$f" 2>/dev/null | sed "s#^#$f:#"; done | head -5)
if [ -z "$bad" ]; then pass "no conflict markers"; else fail "conflict markers:"; echo "$bad"; fi

# 2. whole-file rewrites (BOM, CRLF, mojibake from agents): a changed file whose diff touches nearly every line
big=$(git diff --numstat "$MB" HEAD 2>/dev/null | awk '$1+$2>200 && $1>0 && $2>0 && ($1>0.9*$2 && $1<1.1*$2) {print $3" (+"$1"/-"$2")"}' | head -5)
bom=$(printf '%s\n' "$FILES" | grep -E '\.(rs|md|toml|json|sh|ps1|yml)$' | while read -r f; do [ -f "$f" ] && [ "$(head -c3 "$f" | od -An -tx1 | tr -d ' ')" = "efbbbf" ] && echo "$f"; done | head -5)
if [ -n "$bom" ]; then fail "UTF-8 BOM in: $(echo $bom)"; else pass "no BOM in changed files"; fi
[ -n "$big" ] && echo "warn whole-file rewrite? check git diff --stat: $(echo $big)"

# 3. shell syntax
if [ "$SH" = 1 ]; then
  s=0; for f in $(printf '%s\n' "$FILES" | grep '\.sh$'); do [ -f "$f" ] && { bash -n "$f" 2>&1 || s=1; }; done
  [ "$s" = 0 ] && pass "bash -n on changed .sh" || fail "bash -n on changed .sh"
fi

# 4. rustfmt per package (never --all: os error 206 on Windows)
if [ -n "$CRATES" ]; then
  for c in $CRATES; do
    if out=$(cargo fmt -p "$c" --check 2>&1); then pass "fmt -p $c"; else fail "fmt -p $c (run: cargo fmt -p $c)"; echo "$out" | head -8; fi
  done
elif [ "$RS" = 1 ]; then skip "fmt/clippy: .rs changed outside crates/ (run rustfmt on those files by hand)"; fi

# 5. clippy and tests per package
if [ -n "$CRATES" ]; then
  if [ "$CLIPPY" = 1 ]; then
    for c in $CRATES; do
      if out=$(cargo clippy -p "$c" --all-targets --locked -- -D warnings 2>&1); then pass "clippy -p $c"; else fail "clippy -p $c"; echo "$out" | grep -E '^(error|warning)|-->' | head -12; fi
    done
  else skip "clippy ($CRATES): --no-clippy"; fi
  if [ "$TEST" = 1 ]; then
    for c in $CRATES; do
      if timeout "${TEST_TIMEOUT:-1500}" cargo test -p "$c" --locked >/dev/null 2>&1; then pass "test -p $c"; else fail "test -p $c (rerun it: cargo test -p $c --locked)"; fi
    done
  else skip "cargo test ($CRATES): pass --test (CI runs them)"; fi
fi
if [ "$LOCK" = 1 ]; then
  if cargo metadata --locked --format-version 1 >/dev/null 2>&1; then pass "Cargo.lock matches manifests (--locked)"; else fail "Cargo.lock out of date (cargo metadata --locked)"; fi
fi
[ -n "$CRATES" ] && skip "workspace-wide clippy: other crates that depend on $CRATES are not checked"

# 6. repo audits that CI or reviewers run
if [ "$RS" = 1 ]; then
  if bash tools/dev/unsafe-audit.sh >/dev/null 2>&1; then pass "unsafe-audit"; else fail "unsafe-audit (bash tools/dev/unsafe-audit.sh)"; fi
  # a ported unit must update its PARITY row in the same PR (AGENTS.md "Port workflow")
  if printf '%s\n' "$FILES" | grep -qE '^crates/[^/]+/src/' && ! has '^docs/PARITY\.md$'; then echo "warn crates/*/src changed but docs/PARITY.md did not: is there a ported unit without its row?"; fi
fi
if [ "$MD" = 1 ] || [ "$RS" = 1 ]; then
  errs=$("$PY" -I tools/dev/doc-links.py 2>&1 | grep '^ERROR' | grep -F "$FILES" | head -5)
  [ -z "$errs" ] && pass "doc-links (changed files)" || { fail "doc-links in changed files:"; echo "$errs"; }
fi
if [ "$SK" = 1 ]; then
  bash tools/dev/agents-check.sh >/dev/null 2>&1 && pass "agents-check" || fail "agents-check (bash tools/dev/agents-check.sh)"
fi

echo
if [ -n "$SKIPPED" ]; then echo "NOT RUN (say so in the PR body):"; printf '%s' "$SKIPPED" | sed 's/^/  - /'; fi
if [ "$FAILED" = 0 ]; then echo "pre-pr PASSED"; exit 0; fi
echo "pre-pr: $FAILED check(s) FAILED"; exit 1
