#!/usr/bin/env bash
# Selftest for tools/dev/pre-pr.sh on a throwaway git repo (no cargo needed: only non-Rust changes are used).
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
R="$tmp/repo"; mkdir -p "$R/tools/dev" "$R/docs"
cp "$HERE/pre-pr.sh" "$R/tools/dev/pre-pr.sh"
cd "$R" && git init -q -b master && git config user.email t@t && git config user.name t
echo base > docs/a.md && git add -A && git commit -q -m base && git update-ref refs/remotes/origin/master HEAD
run(){ out=$(bash tools/dev/pre-pr.sh "$@" 2>&1); rc=$?; }
expect(){ if [ "$rc" -eq "$2" ] && { [ -z "${3:-}" ] || printf '%s' "$out" | grep -q -- "$3"; }; then ok "$1"; else bad "$1 (rc=$rc)"; printf '%s\n' "$out" | tail -n 6; fi; }

run;                                   expect "nothing changed -> 0" 0 "nothing changed"
echo "<<<<<<< ours" >> docs/a.md; run;   expect "conflict marker -> 1" 1 "conflict markers"
echo base > docs/a.md; printf '\xef\xbb\xbfx\n' > docs/b.md; run; expect "BOM -> 1" 1 "BOM"
rm docs/b.md; echo ok > docs/c.md; run; expect "clean md change -> 0" 0 "pre-pr PASSED"
printf 'if then\n' > tools/dev/bad.sh; run; expect "bad shell syntax -> 1" 1 "bash -n"
rm tools/dev/bad.sh
run --bogus;                           expect "unknown option -> 2" 2 "unknown option"
run --list;                            expect "--list prints files" 0 "docs/c.md"
rm -rf "$tmp"
[ "$fails" -eq 0 ] && echo "pre-pr selftest PASSED" || { echo "pre-pr selftest: $fails FAILED"; exit 1; }
