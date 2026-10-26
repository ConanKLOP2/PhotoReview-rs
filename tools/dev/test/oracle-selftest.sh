#!/usr/bin/env bash
# Never runs the real oracle: the repo root, git and powershell are all fakes inside a mktemp directory.
set -u
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
trap 'rm -rf "$tmp"' EXIT
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
has(){ case "$1" in *"$2"*) return 0;; *) return 1;; esac; }
R="$tmp/root"; mkdir -p "$R/tools/dev" "$R/tests/golden" "$tmp/committed" "$tmp/csharp/native/x64"
cp "$SRC/tools/dev/oracle.sh" "$R/tools/dev/oracle.sh"; : > "$R/tools/oracle.ps1"
: > "$tmp/csharp/native/x64/turbojpeg.dll"; : > "$tmp/csharp/native/x64/libraw.dll"
cat > "$tmp/git.sh" <<'EOF'
#!/bin/sh
case "$*" in
*'rev-parse --path-format=absolute --git-common-dir'*) echo "$FAKE_COMMON";;
*'show HEAD:tests/golden/'*) for a; do :; done; f="${a##*/}"; [ -f "$FAKE_COMMITTED/$f" ] && cat "$FAKE_COMMITTED/$f" || exit 128;;
*) exit 1;;
esac
EOF
cat > "$tmp/ps.sh" <<'EOF'
#!/bin/sh
echo "$*" >> "$FAKE_PSLOG"
[ "${PS_RC:-0}" != 0 ] && exit "$PS_RC"
while [ $# -gt 0 ]; do [ "$1" = -Command ] && cmd=$2; shift; done
n=$(cat "$FAKE_COUNT" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$FAKE_COUNT"
case "${FAKE_MODE:-same}" in
  same) printf '{\n  "csharp_commit": "aaa",\n  "v": 1\n}\n' > "$FAKE_ROOT/tests/golden/$cmd.json";;
  newcommit) printf '{\n  "csharp_commit": "bbb",\n  "v": 1\n}\n' > "$FAKE_ROOT/tests/golden/$cmd.json";;
  flip) printf '{\n  "csharp_commit": "aaa",\n  "v": %s\n}\n' "$n" > "$FAKE_ROOT/tests/golden/$cmd.json";;
  nofile) :;;
esac
EOF
export FAKE_ROOT="$R" FAKE_COMMITTED="$tmp/committed" FAKE_COMMON="$tmp/fake/.git" FAKE_PSLOG="$tmp/pslog" FAKE_COUNT="$tmp/count"
export GIT_BIN="bash $tmp/git.sh" PS_BIN="bash $tmp/ps.sh" ORACLE_CSHARP_REPO="$tmp/csharp" ORACLE_TEMP="$tmp/empty-temp"
GIT_BIN="$tmp/git.sh"; PS_BIN="$tmp/ps.sh"; chmod +x "$tmp/git.sh" "$tmp/ps.sh"
O="bash $R/tools/dev/oracle.sh"
committed_text=$(printf '{\n  "csharp_commit": "aaa",\n  "v": 1\n}\n')
reset(){ rm -f "$tmp/pslog" "$tmp/count" "$R"/tests/golden/*.json "$tmp"/committed/*.json; printf '%s\n' "$committed_text" > "$tmp/committed/alpha.json"; cp "$tmp/committed/alpha.json" "$R/tests/golden/alpha.json"; unset FAKE_MODE PS_RC; }

reset; FAKE_MODE=same; export FAKE_MODE
o=$($O alpha 2>&1); rc=$?
{ [ $rc = 0 ] && has "$o" "alpha: identical (2 runs)" && has "$o" "same as committed golden"; } && ok identical-and-same-as-committed || bad "identical-and-same-as-committed rc=$rc $o"
cmp -s "$tmp/committed/alpha.json" "$R/tests/golden/alpha.json" && ok committed-restored || bad committed-restored
[ "$(grep -c . "$tmp/pslog")" = 2 ] && ok runs-default-2 || bad runs-default-2
has "$(cat "$tmp/pslog")" "-NoProfile -ExecutionPolicy Bypass -File" && has "$(cat "$tmp/pslog")" "-Command alpha" && ! has "$(cat "$tmp/pslog")" "-Commit" && ok ps-args-no-commit || bad "ps-args-no-commit: $(cat "$tmp/pslog")"

reset; FAKE_MODE=newcommit; export FAKE_MODE
o=$($O alpha --runs 3 --commit abc123 2>&1); rc=$?
{ [ $rc = 0 ] && has "$o" "identical (3 runs)" && has "$o" "ONLY in csharp_commit"; } && ok csharp-commit-only-diff || bad "csharp-commit-only-diff rc=$rc $o"
[ "$(grep -c -- '-Commit abc123' "$tmp/pslog")" = 3 ] && ok commit-passed-to-ps || bad "commit-passed-to-ps: $(cat "$tmp/pslog")"
cmp -s "$tmp/committed/alpha.json" "$R/tests/golden/alpha.json" && ok restored-after-diff || bad restored-after-diff

reset; FAKE_MODE=flip; export FAKE_MODE
o=$($O alpha 2>&1); rc=$?
{ [ $rc = 1 ] && has "$o" "DIFFERENT between runs"; } && ok different-runs-detected || bad "different-runs-detected rc=$rc $o"

reset; FAKE_MODE=newcommit; export FAKE_MODE
o=$($O alpha --keep 2>&1)
{ grep -q '"bbb"' "$R/tests/golden/alpha.json" && has "$o" "--keep"; } && ok keep-leaves-regenerated || bad "keep-leaves-regenerated: $o"

reset; FAKE_MODE=same; export FAKE_MODE
o=$($O newcmd 2>&1); rc=$?
{ [ $rc = 0 ] && has "$o" "no committed golden" && [ ! -f "$R/tests/golden/newcmd.json" ]; } && ok new-golden-removed-afterwards || bad "new-golden-removed-afterwards rc=$rc $o"

reset; echo changed >> "$R/tests/golden/alpha.json"
o=$($O alpha 2>&1); rc=$?
{ [ $rc = 1 ] && has "$o" "REFUSED" && [ ! -f "$tmp/pslog" ] && grep -q changed "$R/tests/golden/alpha.json"; } && ok dirty-golden-refused || bad "dirty-golden-refused rc=$rc $o"

reset; PS_RC=3; export PS_RC
o=$($O alpha 2>&1); rc=$?
{ [ $rc = 1 ] && has "$o" "alpha: FAILED"; } && ok ps-failure-reported || bad "ps-failure-reported rc=$rc $o"
cmp -s "$tmp/committed/alpha.json" "$R/tests/golden/alpha.json" && ok restored-after-failure || bad restored-after-failure

reset; FAKE_MODE=nofile; export FAKE_MODE
o=$($O alpha --runs 1 2>&1); rc=$?
# the committed golden is still in tests/golden, so the run "produces" the committed file: single run, no reproducibility claim
{ has "$o" "single run"; } && ok single-run-message || bad "single-run-message: $o"

reset; FAKE_MODE=same; export FAKE_MODE
o=$(ORACLE_CSHARP_REPO="$tmp/nocs" $O alpha 2>&1)
{ has "$o" "WARNING: turbojpeg.dll not found" && has "$o" "WARNING: libraw.dll not found"; } && ok warns-missing-native-dlls || bad "warns-missing-native-dlls: $o"
o=$($O alpha 2>&1)
! has "$o" "WARNING" && ok no-warning-when-dlls-present || bad "no-warning-when-dlls-present: $o"

o=$($O 2>&1); rc=$?; [ $rc = 2 ] && ok usage-no-command || bad "usage-no-command rc=$rc"
o=$($O alpha --runs 0 2>&1); rc=$?; [ $rc = 2 ] && ok usage-bad-runs || bad "usage-bad-runs rc=$rc"
o=$($O '../x' 2>&1); rc=$?; [ $rc = 2 ] && ok rejects-path-like-command || bad "rejects-path-like-command rc=$rc"

if [ $fails = 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; exit 1; fi
