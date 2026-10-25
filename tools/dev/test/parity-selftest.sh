#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
trap 'rm -rf "$tmp"' EXIT
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
has(){ case "$1" in *"$2"*) return 0;; *) return 1;; esac; }
PY=${PY_BIN:-python}
cat > "$tmp/PARITY.md" <<'EOF'
# Parity
Intro line with | pipe but no table.

| ID | Status (Todo / Doing / Done / Different) | Rust location | Note |
|---|---|---|---|
| A-1 | Done (W1-01) | `x::a` | fine |
| A-2 | Todo (contract W8-18l..18n) | `x::b` | all merged |
| A-3, A-4 | Partial (W8-18o; W8-30z) | `x::c` | one unit not merged |
| A-5 | Partial: W8-19a | `x::d | e` | pipe inside backticks |
| A-6 | Mystery | `x::f` | unknown status |

| ID | Item | C# source | Notes |
|---|---|---|---|
| M-1 | Menu item | Foo.cs | not a status row |

| Unit | Status | Evidence |
|---|---|---|
| W4-01b decode types | Done | proof |

- bullet breaks the table
| A-7 | Doing (W8-19a) | `x::g` | headerless continuation |
EOF
cat > "$tmp/git.sh" <<'EOF'
#!/bin/sh
case "$*" in
*'log origin/master'*) printf 'aaa1111\tfeat(W8-18l..18n): painter (#1)\nbbb2222\tdocs: lessons\nccc3333\tfeat(W8-19a): framework (#2)\nddd4444\tfeat(W8-18o): secondary (#3)\n';;
*) exit 1;;
esac
EOF
export PARITY_DOC="$tmp/PARITY.md" GIT_BIN="$tmp/git.sh"
P="bash $ROOT/tools/dev/parity.sh"
before=$(sha256sum "$PARITY_DOC" | cut -d' ' -f1)

o=$($P stats 2>&1)
{ has "$o" "status rows: 7" && has "$o" "Done" && has "$o" "unrecognized" && has "$o" "Doing"; } && ok stats-counts || bad "stats-counts: $o"
has "$o" "not counted: 1" && ok stats-plain-table-skipped || bad "stats-plain-table-skipped: $o"
has "$o" "Mystery" && ok stats-lists-unrecognized || bad "stats-lists-unrecognized: $o"
has "$o" "14.3%" && ok stats-percent || bad "stats-percent: $o"

o=$($P unit W8-18m 2>&1)
{ has "$o" "A-2" && has "$o" "aaa1111" && ! has "$o" "ddd4444"; } && ok unit-range-expansion || bad "unit-range-expansion: $o"
o=$($P unit W8-19a 2>&1)
{ has "$o" "A-5" && has "$o" "A-7" && has "$o" "ccc3333"; } && ok unit-rows-and-commits || bad "unit-rows-and-commits: $o"

o=$($P reconcile 2>&1); rc=$?
{ [ $rc = 0 ] && has "$o" "A-2" && has "$o" "A-5" && has "$o" "A-7" && ! has "$o" "A-1 " && ! has "$o" "A-3"; } && ok reconcile-merged-only || bad "reconcile-merged-only: $o"
has "$o" "no file was changed" && ok reconcile-prints-only || bad reconcile-prints-only
[ "$(sha256sum "$PARITY_DOC" | cut -d' ' -f1)" = "$before" ] && ok reconcile-no-edit || bad reconcile-no-edit

o=$(CODEX_LIB=/nonexistent/codex-lib.py $P reconcile 2>&1)
has "$o" "A-2" && ok fallback-expander || bad "fallback-expander: $o"

o=$($P set A-2 Done "PR #1 merged" 2>&1); rc=$?
{ [ $rc = 0 ] && has "$o" "dry-run" && [ "$(sha256sum "$PARITY_DOC" | cut -d' ' -f1)" = "$before" ]; } && ok set-dry-run || bad "set-dry-run: $o"

o=$($P set A-2 Bogus "x" 2>&1); rc=$?
{ [ $rc = 2 ] && has "$o" "unknown status"; } && ok set-refuses-unknown-status || bad "set-refuses-unknown-status rc=$rc $o"
o=$($P set NOPE Done "x" 2>&1); rc=$?
[ $rc = 1 ] && ok set-unknown-row || bad "set-unknown-row rc=$rc $o"
o=$($P set A-2 Done "a|b" 2>&1); rc=$?
[ $rc = 2 ] && ok set-refuses-pipe-in-evidence || bad "set-refuses-pipe-in-evidence rc=$rc"

o=$($P set A-4 Done "PR #3" --apply 2>&1); rc=$?
n=$(grep -c 'Done (W8-18o; W8-30z); PR #3 |' "$PARITY_DOC")
{ [ $rc = 0 ] && [ "$n" = 1 ] && grep -q '^| A-3, A-4 | Done (W8-18o; W8-30z); PR #3 | `x::c` | one unit not merged |$' "$PARITY_DOC"; } && ok set-apply-edits-status-cell-only || bad "set-apply-edits-status-cell-only rc=$rc $o"
grep -q '^| A-1 | Done (W1-01) | `x::a` | fine |$' "$PARITY_DOC" && grep -q '^| A-5 | Partial: W8-19a | `x::d | e` | pipe inside backticks |$' "$PARITY_DOC" && ok set-other-rows-untouched || bad set-other-rows-untouched
[ "$(grep -c '' "$PARITY_DOC")" = "$(grep -c '' "$tmp/PARITY.md")" ] && ! grep -q $'\r' "$PARITY_DOC" && ok set-keeps-lf || bad set-keeps-lf

o=$($P set A-5 Superseded "ADR" --apply 2>&1)
grep -q '^| A-5 | Superseded: W8-19a; ADR | `x::d | e` | pipe inside backticks |$' "$PARITY_DOC" && ok set-backtick-pipe-row || bad "set-backtick-pipe-row: $o"

o=$($P bogus 2>&1); rc=$?
[ $rc = 2 ] && ok usage-rc2 || bad usage-rc2

if [ $fails = 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; exit 1; fi
