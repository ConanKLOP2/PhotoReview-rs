#!/usr/bin/env bash
# Control-flow test of codex-pipeline.sh with a FAKE Codex (no quota used, no network, no gh).
# Builds a throw-away git repo (bare origin + work clone + tiny cargo crate), copies the pipeline scripts into it and checks:
# success, gate failure + repair, mutant-killing round, quota wait, failed unit + skipped dependent, resume, lock.
# Usage: bash tools/autopilot/test/pipeline-selftest.sh      (takes a few minutes: it runs real cargo and cargo-mutants)
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; SRC="$(cd "$HERE/.." && pwd)"
T="$(mktemp -d)"; ORIGIN="$T/origin.git"; W="$T/work"; FS="$T/fake-state"; mkdir -p "$FS"
FAILS=0
ok() { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }

git init -q --bare "$ORIGIN"; git init -q -b master "$W"; cd "$W" || exit 1
git config user.name selftest; git config user.email selftest@example.com
mkdir -p crates/demo/src crates/demo/tests contracts docs tools/autopilot
cat > Cargo.toml <<'X'
[workspace]
members = ["crates/demo"]
resolver = "2"
X
cat > crates/demo/Cargo.toml <<'X'
[package]
name = "demo"
version = "0.1.0"
edition = "2021"
X
printf 'pub mod u90a;\npub mod u90b;\npub mod u90c;\n' > crates/demo/src/lib.rs
printf 'pub fn run(_x: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90a.rs
printf 'pub fn run(_a: i32, _b: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90b.rs
printf 'pub fn run(_a: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90c.rs
printf 'target/\n.claude/worktrees/\n/target-wave-*/\ntools/autopilot/state/\n.codex-*\n' > .gitignore
mkc() { # id title
  cat > "contracts/$1.md" <<X
# $1: $2
- Baseline C# \`d92d6aae\`.

## Owned files (the ONLY files the porter may create or modify)
    crates/demo/src/${1#W8-}.rs
    crates/demo/tests/${1#W8-}.rs   (new)
- Lead-owned: crates/demo/src/lib.rs
X
}
mkc W8-90a sign; mkc W8-90b sub; mkc W8-90c double; mkc W8-90d never
sed -i 's#/\(90.\)\.rs#/u\1.rs#' contracts/W8-90?.md
cat > docs/WAVE-8-PLAN.md <<'X'
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-90a | a | a | G | Haiku | G | none | B9 |
| W8-90b | b | b | G | Haiku | G | 90a | B9 |
| W8-90c | c | c | G | Haiku | G | 90a | B9 |
| W8-90d | d | d | G | Haiku | G | 90c | B9 |
X
cp "$SRC/../../rust-toolchain.toml" . 2>/dev/null
cp -r "$SRC"/CODEX-RULES.md "$SRC"/codex-*.sh "$SRC"/lib tools/autopilot/
rm -rf tools/autopilot/lib/__pycache__
cargo generate-lockfile -q 2>/dev/null
git add -A && git commit -q -m init && git remote add origin "$ORIGIN" && git push -q origin master

# ---- fake codex: behaves according to the TASK/Unit lines of the prompt (stdin)
cat > "$T/fake-codex" <<'X'
#!/usr/bin/env bash
FS="${FAKE_STATE:?}"
while [ $# -gt 0 ]; do case "$1" in -C) shift; D="$1";; -o) shift; O="$1";; esac; shift; done
P="$(cat)"; TASK="$(printf '%s' "$P" | grep -o 'TASK: [a-z-]*' | head -1 | cut -d' ' -f2)"; U="$(printf '%s' "$P" | grep -o 'Unit: W8-90.' | head -1 | sed 's/Unit: W8-//')"
echo "FAKE TRANSCRIPT NOISE $TASK $U"; echo "$TASK $U" >> "$FS/calls"
cd "$D" || exit 1
n=$(grep -c "^$TASK $U\$" "$FS/calls")
src() { printf '%s\n' "$2" > "crates/demo/src/u$1.rs"; }
tst() { mkdir -p crates/demo/tests; printf '%s\n' "$2" > "crates/demo/tests/u$1.rs"; }
case "$TASK:$U" in
  red-tests:90a) tst 90a '#[test]
fn positive() {
    assert_eq!(demo::u90a::run(5), 1);
}';;
  implement:90a) src 90a 'pub fn run(x: i32) -> i32 {
    if x > 0 {
        1
    } else if x < 0 {
        -1
    } else {
        0
    }
}';;
  kill-mutants:90a) tst 90a '#[test]
fn positive() {
    assert_eq!(demo::u90a::run(5), 1);
}

#[test]
fn negative() {
    assert_eq!(demo::u90a::run(-5), -1);
}

#[test]
fn zero() {
    assert_eq!(demo::u90a::run(0), 0);
}';;
  red-tests:90b) tst 90b '#[test]
fn subtracts() {
    assert_eq!(demo::u90b::run(5, 3), 2);
    assert_eq!(demo::u90b::run(3, 5), -2);
}';;
  implement:90b) src 90b 'pub fn run(a: i32, b: i32) -> i32 {
    a - b + 1
}';;   # deliberately wrong first
  repair:90b) src 90b 'pub fn run(a: i32, b: i32) -> i32 {
    a - b
}';;
  red-tests:90c) tst 90c '#[test]
fn doubles() {
    assert_eq!(demo::u90c::run(2), 4);
    assert_eq!(demo::u90c::run(0), 0);
    assert_eq!(demo::u90c::run(-3), -6);
}';;
  implement:90c) src 90c 'pub fn run(a: i32) -> i32 {
    a * 2
}'; echo '// stray edit outside the owned files' >> crates/demo/src/lib.rs;;
  repair:90c) :;;   # never fixes the stray edit
esac
echo "summary: done" > "$O"
X
cat > "$T/fake-usage" <<'X'
#!/usr/bin/env bash
FS="${FAKE_STATE:?}"; echo x >> "$FS/usage-calls"
if [ ! -f "$FS/quota-blocked-once" ]; then touch "$FS/quota-blocked-once"
  echo '{"ok":false,"five_hour_used":100,"five_hour_resets_at":"2000-01-01 00:00","weekly_used":10,"weekly_resets_at":"2000-01-01 00:00"}'; exit 1; fi
echo '{"ok":true,"five_hour_used":10}'; exit 0
X
chmod +x "$T/fake-codex" "$T/fake-usage"
export CODEX_BIN="$T/fake-codex" CODEX_USAGE_SH="$T/fake-usage" FAKE_STATE="$FS" QUOTA_SLEEP_OVERRIDE=1 PIPELINE_NO_PR=1
export MUTANTS_TIMEOUT=600 CODEX_TIMEOUT=120 CS_REPO="$T/no-such-cs-repo"
P="bash tools/autopilot/codex-pipeline.sh test-b9"
STATE="tools/autopilot/state/test-b9.json"; LOGS="tools/autopilot/state/logs"

echo "== dry run"
$P --dry-run > "$T/dry.out"; check "dry-run lists 4 units in dependency order" '[ "$(grep -c "^ *[0-9]*\. W8-90" "$T/dry.out")" = 4 ]'
check "dry-run creates no state/worktree" '[ ! -e "$STATE" ] && [ ! -d .claude/worktrees ]'

echo "== run 1: only W8-90a (success, quota wait, mutant-killing round)"
$P --only W8-90a > "$T/run1.out" 2>&1; rc=$?
check "exit 0" '[ $rc = 0 ]'
check "90a is done" '[ "$(python tools/autopilot/lib/codex-lib.py sget $STATE W8-90a)" = done ]'
check "quota wait was logged" 'grep -q "codex quota not available" "$T/run1.out"'
check "red-tests, implement and a kill-mutants call happened" 'grep -q "^red-tests 90a" $FS/calls && grep -q "^implement 90a" $FS/calls && grep -q "^kill-mutants 90a" $FS/calls'
check "codex transcript is not printed" '! grep -q "FAKE TRANSCRIPT NOISE" "$T/run1.out"'
check "codex transcript is in the log file" 'grep -q "FAKE TRANSCRIPT NOISE" $LOGS/W8-90a.log'
check "merged into integration branch with trailer" 'git log integration/test-b9 --format=%B | grep -q "Co-Authored-By: Codex gpt-6-luna"'
check "unit target dir deleted" '[ ! -d target-wave-pipe-w8-90a ]'

echo "== run 2: full batch (resume 90a, repair 90b, failed 90c, skipped 90d)"
$P > "$T/run2.out" 2>&1; rc=$?
check "90a skipped as already merged" 'grep -q "W8-90a: already merged" "$T/run2.out"'
check "90a not re-run" '[ "$(grep -c "^implement 90a" $FS/calls)" = 1 ]'
check "90b done after a repair round" '[ "$(python tools/autopilot/lib/codex-lib.py sget $STATE W8-90b)" = done ] && grep -q "^repair 90b" $FS/calls'
check "repair prompt carried the gate error tail" 'grep -q "GATE FAIL: cargo test" $LOGS/W8-90b.repair1.prompt.md'
check "90c failed (stray edit never fixed, 3 repair rounds)" '[ "$(python tools/autopilot/lib/codex-lib.py sget $STATE W8-90c)" = failed ] && [ "$(grep -c "^repair 90c" $FS/calls)" = 3 ]'
check "gate error mentions the owned-files check" 'grep -q "owned-files check" $LOGS/W8-90c.gate-error.txt'
check "90d skipped because 90c failed" '[ "$(python tools/autopilot/lib/codex-lib.py sget $STATE W8-90d)" = skipped ]'
check "90c not merged, 90b merged" '! git log integration/test-b9 --format=%s | grep -q "W8-90c" && git log integration/test-b9 --format=%s | grep -q "Merge W8-90b"'
check "workspace gate passed on the integration branch" 'grep -q "workspace gate: OK" "$T/run2.out"'
check "state file is valid JSON with 4 units" '[ "$(python tools/autopilot/lib/codex-lib.py slist $STATE | wc -l)" = 4 ]'

echo "== lock"
sleep 30 & SP=$!; mkdir tools/autopilot/state/pipeline.lock; echo $SP > tools/autopilot/state/pipeline.lock/pid
$P > "$T/lock.out" 2>&1; rc=$?; kill $SP 2>/dev/null; rm -rf tools/autopilot/state/pipeline.lock
check "second pipeline refused (exit 3)" '[ $rc = 3 ]'

echo; if [ "$FAILS" = 0 ]; then echo "selftest PASSED"; else echo "selftest: $FAILS FAILED (work dir kept: $T)"; exit 1; fi
cd /; git -C "$W" worktree prune 2>/dev/null; rm -rf "$T"
