#!/usr/bin/env bash
# Control-flow test of codex-pipeline.sh with a FAKE Codex (no quota used, no network, no gh).
# Builds a throw-away git repo (bare origin + work clone + tiny cargo crate), copies the pipeline scripts into it and checks:
# success, gate failure + repair, mutant-killing round, session reuse (resume), quota wait and quota estimate, failed unit +
# skipped dependent + needs-claude triage, resume, lock, parallel jobs, tier-based mutation skipping, telemetry, static review
# gates, red-test guard, pack trimming, preflight and --smoke.
# Usage: bash tools/autopilot/test/pipeline-selftest.sh      (takes a few minutes: it runs real cargo and cargo-mutants)
set -u
export PATH="$HOME/.cargo/bin:$PATH"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; SRC="$(cd "$HERE/.." && pwd)"
T="$(mktemp -d)"; ORIGIN="$T/origin.git"; W="$T/work"; FS="$T/fake-state"; mkdir -p "$FS"
FAILS=0
ok() { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }

# ---- tiny C# repo for the preflight check (the pack tolerates a missing baseline)
mkdir -p "$T/cs" && git -C "$T/cs" init -q -b master && git -C "$T/cs" -c user.name=t -c user.email=t@e.com commit -q --allow-empty -m init

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
printf 'pub mod u90a;\npub mod u90b;\npub mod u90c;\npub mod u90e;\npub mod u90f;\n' > crates/demo/src/lib.rs
printf 'pub fn run(_x: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90a.rs
printf 'pub fn run(_a: i32, _b: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90b.rs
for n in 90c 90e 90f; do printf 'pub fn run(_a: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u$n.rs; done
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
mkc W8-90a sign; mkc W8-90b sub; mkc W8-90c double; mkc W8-90d never; mkc W8-90e inc; mkc W8-90f triple
sed -i 's#/\(90.\)\.rs#/u\1.rs#' contracts/W8-90?.md
cat > docs/WAVE-8-PLAN.md <<'X'
| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-90a | a | a | yellow | Sonnet | F | none | B9 |
| W8-90b | b | b | yellow | Sonnet | F | 90a | B9 |
| W8-90c | c | c | red | Sonnet | F | 90a | B9 |
| W8-90d | d | d | yellow | Sonnet | F | 90c | B9 |
| W8-90e | e | e | green | Haiku | G | none | B9 |
| W8-90f | f | f | green | Haiku | G | none | B8 |
X
cp "$SRC/../../rust-toolchain.toml" . 2>/dev/null
cp -r "$SRC"/CODEX-RULES.md "$SRC"/codex-*.sh "$SRC"/lib tools/autopilot/
rm -rf tools/autopilot/lib/__pycache__
cargo generate-lockfile -q 2>/dev/null
git add -A && git commit -q -m init && git remote add origin "$ORIGIN" && git push -q origin master
LIBPY="$W/tools/autopilot/lib/codex-lib.py"

# ---- fake codex: behaves according to the TASK line of the prompt; a resumed session maps sid -> unit
cat > "$T/fake-codex" <<'X'
#!/usr/bin/env bash
FS="${FAKE_STATE:?}"
ARGS=("$@"); RESUME=""; D="$PWD"; O="/dev/null"
[ "${1:-}" = exec ] && [ "${2:-}" = resume ] && RESUME=1
i=0; while [ $i -lt ${#ARGS[@]} ]; do case "${ARGS[$i]}" in -C) i=$((i+1)); D="${ARGS[$i]}";; -o) i=$((i+1)); O="${ARGS[$i]}";; esac; i=$((i+1)); done
P="$(cat)"
if [ -n "$RESUME" ]; then
  SID="${ARGS[$(( ${#ARGS[@]} - 2 ))]}"
  if [ -f "$FS/fail-resume-once" ]; then rm -f "$FS/fail-resume-once"; echo "resume failed" >&2; echo "resume-failed $SID" >> "$FS/calls"; exit 1; fi
  U="$(cat "$FS/sess-$SID" 2>/dev/null)"
else
  SID="sess-$RANDOM$RANDOM"; U="$(printf '%s' "$P" | grep -o 'Unit: W8-90.' | head -1 | sed 's/Unit: W8-//')"; echo "$U" > "$FS/sess-$SID"
fi
TASK="$(printf '%s' "$P" | grep -o 'TASK: [a-z-]*' | head -1 | cut -d' ' -f2)"
echo "{\"type\":\"thread.started\",\"thread_id\":\"$SID\"}"; echo "FAKE TRANSCRIPT NOISE $TASK $U"
echo "$TASK $U ${RESUME:+resumed}" >> "$FS/calls"
if printf '%s' "$P" | grep -q 'single word OK'; then echo OK > "$O"; echo '{"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":1}}'; exit 0; fi
cd "$D" || exit 1
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
  implement:90b) echo "start b $(date +%s)" >> "$FS/times"; src 90b 'pub fn run(a: i32, b: i32) -> i32 {
    a - b + 1
}'; sleep 10; echo "end b $(date +%s)" >> "$FS/times";;   # deliberately wrong first
  repair:90b) src 90b 'pub fn run(a: i32, b: i32) -> i32 {
    a - b
}';;
  red-tests:90c) tst 90c '#[test]
fn doubles() {
    assert_eq!(demo::u90c::run(2), 4);
    assert_eq!(demo::u90c::run(0), 0);
    assert_eq!(demo::u90c::run(-3), -6);
}';;
  implement:90c) echo "start c $(date +%s)" >> "$FS/times"; src 90c 'pub fn run(a: i32) -> i32 {
    a * 2
}'; echo '// stray edit outside the owned files' >> crates/demo/src/lib.rs; sleep 10; echo "end c $(date +%s)" >> "$FS/times";;
  repair:90c) :;;   # never fixes the stray edit
  red-tests:90e) tst 90e '#[test]
fn inc() {
    assert_eq!(demo::u90e::run(1), 2);
}';;
  implement:90e) src 90e 'pub fn run(a: i32) -> i32 {
    a + 1
}';;
  red-tests:90f) tst 90f '#[test]
fn triple() {
    assert_eq!(demo::u90f::run(2), 6);
}';;
  implement:90f) src 90f 'pub fn run(a: i32) -> i32 {
    a * 3
}';;
esac
echo "summary: done" > "$O"
echo '{"type":"turn.completed","usage":{"input_tokens":1000,"cached_input_tokens":200,"output_tokens":50}}'
X
cat > "$T/fake-usage" <<'X'
#!/usr/bin/env bash
FS="${FAKE_STATE:?}"; echo x >> "$FS/usage-calls"
if [ ! -f "$FS/quota-blocked-once" ]; then touch "$FS/quota-blocked-once"
  echo '{"ok":false,"five_hour_used":100,"five_hour_resets_at":"2000-01-01 00:00","weekly_used":10,"weekly_resets_at":"2000-01-01 00:00"}'; exit 1; fi
used=10
if [ -s "$FS/used-seq" ]; then used="$(head -1 "$FS/used-seq")"; sed -i 1d "$FS/used-seq"; fi
if [ "$used" -ge "${1:-99}" ]; then echo "{\"ok\":false,\"five_hour_used\":$used,\"five_hour_resets_at\":\"2000-01-01 00:00\"}"; exit 1; fi
echo "{\"ok\":true,\"five_hour_used\":$used,\"five_hour_resets_at\":\"2000-01-01 00:00\"}"; exit 0
X
chmod +x "$T/fake-codex" "$T/fake-usage"
export CODEX_BIN="$T/fake-codex" CODEX_USAGE_SH="$T/fake-usage" FAKE_STATE="$FS" QUOTA_SLEEP_OVERRIDE=1 PIPELINE_NO_PR=1 USAGE_CACHE_TTL=0
export MUTANTS_TIMEOUT=600 CODEX_TIMEOUT=120 CS_REPO="$T/cs"
P="bash tools/autopilot/codex-pipeline.sh test-b9"
STATE="tools/autopilot/state/test-b9.json"; LOGS="tools/autopilot/state/logs"; SF="python $LIBPY sfield $STATE"; SG="python $LIBPY sget $STATE"

echo "== dry run"
$P --dry-run > "$T/dry.out"; check "dry-run lists 5 units in dependency order" '[ "$(grep -c "^ *[0-9]*\. W8-90" "$T/dry.out")" = 5 ]'
check "dry-run shows the mutation tier (skip for the green unit)" 'grep -q "W8-90e.*mutation=skip" "$T/dry.out" && grep -q "W8-90c.*mutation=red" "$T/dry.out"'
check "dry-run creates no state/worktree" '[ ! -e "$STATE" ] && [ ! -d .claude/worktrees ]'

echo "== preflight"
CS_REPO="$T/nope" $P --only W8-90a > "$T/pf1.out" 2>&1; rc=$?
check "missing C# repo aborts early (exit 4, clear message)" '[ $rc = 4 ] && grep -q "C# repo not found" "$T/pf1.out"'
CODEX_BIN="$T/nope" $P --only W8-90a > "$T/pf2.out" 2>&1; rc=$?
check "missing codex binary aborts early" '[ $rc = 4 ] && grep -q "codex binary not found" "$T/pf2.out"'
MIN_DISK_GB=99999999 $P --only W8-90a > "$T/pf3.out" 2>&1; rc=$?
check "low disk space aborts early" '[ $rc = 4 ] && grep -q "free on the repo drive" "$T/pf3.out"'
check "preflight failures started nothing" '[ ! -d .claude/worktrees ] && [ ! -d tools/autopilot/state/pipeline.lock ]'

echo "== run 1: only W8-90a, --jobs 1 (success, quota wait, session reuse, mutants, telemetry)"
$P --only W8-90a --jobs 1 > "$T/run1.out" 2>&1; rc=$?
check "exit 0" '[ $rc = 0 ]'
check "90a is done" '[ "$($SG W8-90a)" = done ]'
check "quota wait was logged" 'grep -q "codex quota not available" "$T/run1.out"'
check "red-tests, implement and a kill-mutants call happened" 'grep -q "^red-tests 90a" $FS/calls && grep -q "^implement 90a" $FS/calls && grep -q "^kill-mutants 90a" $FS/calls'
check "implement and kill-mutants continued the same session (resume), red-tests did not" '! grep -q "^red-tests 90a resumed" $FS/calls && grep -q "^implement 90a resumed" $FS/calls && grep -q "^kill-mutants 90a resumed" $FS/calls'
check "first prompt carries the pack instruction, resumed prompts do not" 'grep -q "codex-pack.md" $LOGS/W8-90a.red.prompt.md.send && ! grep -q "codex-pack.md" $LOGS/W8-90a.mut1.prompt.md.send'
check "codex transcript is not printed" '! grep -q "FAKE TRANSCRIPT NOISE" "$T/run1.out"'
check "codex transcript is in the log file" 'grep -q "FAKE TRANSCRIPT NOISE" $LOGS/W8-90a.log'
check "merged into integration branch with trailer" 'git log integration/test-b9 --format=%B | grep -q "Co-Authored-By: Codex gpt-6-luna"'
check "red tests frozen in their own commit" 'git log wu/W8-90a --format=%s 2>/dev/null | grep -q "red tests" || git log integration/test-b9 --format=%s | grep -q "red tests"'
check "telemetry: 3 codex rounds, tokens summed from the --json log" '[ "$($SF W8-90a rounds)" = 3 ] && [ "$($SF W8-90a tin)" = 3000 ] && [ "$($SF W8-90a tout)" = 150 ]'
check "telemetry: pack size and seconds recorded, tier yellow" '[ "$($SF W8-90a pack_bytes)" -gt 100 ] && [ "$($SF W8-90a secs)" -ge 0 ] && [ "$($SF W8-90a tier)" = yellow ]'
check "mutants: survivors were killed in a filtered re-run, none left" '[ "$($SF W8-90a killed)" -ge 1 ] && [ "$($SF W8-90a survivors)" = 0 ] && [ -s $LOGS/W8-90a.survivors.prev ]'
check "telemetry table printed" 'grep -q "| Unit | Status | Rounds" "$T/run1.out" && grep -q "| W8-90a | done | 3 |" "$T/run1.out"'
check "target dirs removed only at the end" '[ ! -d target-wave-pipe-test-b9 ] && [ ! -d target-wave-pipe-test-b9-mut ]'

echo "== run 2: full batch, --jobs 2 (resume 90a, parallel b/c/e, repair b, failed c, skipped d, resume fallback)"
touch "$FS/fail-resume-once"
$P --jobs 2 > "$T/run2.out" 2>&1; rc=$?
check "90a skipped as already merged" 'grep -q "W8-90a: already merged" "$T/run2.out"'
check "90a not re-run" '[ "$(grep -c "^implement 90a" $FS/calls)" = 1 ]'
check "90b done after a repair round on the same session" '[ "$($SG W8-90b)" = done ] && grep -q "^repair 90b resumed" $FS/calls'
check "repair prompt carried the gate error tail (<= 60 lines + header)" 'grep -q "GATE FAIL" $LOGS/W8-90b.repair1.prompt.md && [ "$(wc -l < $LOGS/W8-90b.repair1.prompt.md)" -le 66 ]'
check "repair rounds use the fast gate" 'grep -q "^TASK: repair. The machine gate (fast)" $LOGS/W8-90b.repair1.prompt.md'
check "failed resume fell back to a fresh session" 'grep -q "falling back to a fresh session" "$T/run2.out" && grep -q "^resume-failed" $FS/calls'
check "90c failed (stray edit never fixed, 3 repair rounds)" '[ "$($SG W8-90c)" = failed ] && [ "$(grep -c "^repair 90c" $FS/calls)" = 3 ]'
check "gate error mentions the owned-files check" 'grep -q "owned-files check" $LOGS/W8-90c.gate-error.txt'
check "needs-claude triage note exists and is <= 3 KB" 'N=tools/autopilot/state/needs-claude/W8-90c.md; [ -f $N ] && [ "$(wc -c < $N)" -le 3072 ] && grep -q "Error tail" $N && grep -q "git diff --stat" $N && grep -q "Last Codex message" $N'
check "90d skipped because 90c failed" '[ "$($SG W8-90d)" = skipped ]'
check "90e (green) done, mutation skipped, no mutant counters" '[ "$($SG W8-90e)" = done ] && [ "$($SF W8-90e tier)" = skip ] && [ -z "$($SF W8-90e killed)" ]'
check "90c not merged, 90b merged" '! git log integration/test-b9 --format=%s | grep -q "W8-90c" && git log integration/test-b9 --format=%s | grep -q "Merge W8-90b"'
check "units ran in two slots" 'grep -q "start (slot 1" "$T/run2.out" && grep -q "start (slot 2" "$T/run2.out"'
check "b and c ran concurrently (time windows overlap)" 'bs=$(awk "/start b/{print \$3}" $FS/times); be=$(awk "/end b/{print \$3}" $FS/times); cs=$(awk "/start c/{print \$3}" $FS/times | head -1); ce=$(awk "/end c/{print \$3}" $FS/times | head -1); [ "$bs" -lt "$ce" ] && [ "$cs" -lt "$be" ]'
check "workspace gate passed on the integration branch" 'grep -q "workspace gate: OK" "$T/run2.out"'
check "state file is valid JSON with 5 units" '[ "$(python $LIBPY slist $STATE | wc -l)" = 5 ]'
check "per-unit table in the final output has all units" 'grep -q "| W8-90c | failed" "$T/run2.out" && grep -q "| W8-90e | done" "$T/run2.out"'
check "all target dirs removed at the end" '[ -z "$(ls -d target-wave-pipe-* 2>/dev/null)" ]'

echo "== quota estimate: do not start a unit that may not fit"
git -C "$W" fetch -q origin 2>/dev/null
python $LIBPY supd tools/autopilot/state/test-b8.json W8-90x pct=60 >/dev/null
python $LIBPY sset tools/autopilot/state/test-b8.json W8-90x done "" 0 >/dev/null
printf '50\n' > "$FS/used-seq"
bash tools/autopilot/codex-pipeline.sh test-b8 --jobs 1 > "$T/run3.out" 2>&1; rc=$?
check "estimate 60% x 1.2 = 72% needed, only 50% left -> slept instead of starting" 'grep -q "only 50% left, need 72.0%" "$T/run3.out"'
check "unit started after the quota recovered and was merged" '[ "$(python $LIBPY sget tools/autopilot/state/test-b8.json W8-90f)" = done ]'
check "average cost estimate helper" '[ "$(python $LIBPY avgpct tools/autopilot/state/test-b8.json)" != "" ]'

echo "== lock"
sleep 30 & SP=$!; mkdir tools/autopilot/state/pipeline.lock; echo $SP > tools/autopilot/state/pipeline.lock/pid
$P > "$T/lock.out" 2>&1; rc=$?; kill $SP 2>/dev/null; rm -rf tools/autopilot/state/pipeline.lock
check "second pipeline refused (exit 3)" '[ $rc = 3 ]'

echo "== smoke (fake codex)"
bash tools/autopilot/codex-pipeline.sh --smoke > "$T/smoke.out" 2>&1; rc=$?
check "smoke passes: env, codex call with session id, gate accepts good unit and rejects todo!()" '[ $rc = 0 ] && grep -q "smoke: Codex ok: session sess-" "$T/smoke.out" && grep -q "gate ok" "$T/smoke.out" && grep -q "ALL OK" "$T/smoke.out"'
CODEX_BIN="$T/nope" bash tools/autopilot/codex-pipeline.sh --smoke > "$T/smoke2.out" 2>&1; rc=$?
check "smoke aborts early when the codex binary is missing" '[ $rc = 4 ] && grep -q "codex binary not found" "$T/smoke2.out"'

echo "== static review gates (lint) and red-test guard"
LW="$T/lint"; mkdir -p "$LW/crates/demo/src" "$LW/crates/demo/tests" "$LW/crates/pr-platform-win/src"; git -C "$LW" init -q -b master
git -C "$LW" config user.name t; git -C "$LW" config user.email t@e.com
printf 'pub fn a() {}\n' > "$LW/crates/demo/src/a.rs"; printf 'pub fn w() {}\n' > "$LW/crates/pr-platform-win/src/w.rs"
printf '#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n' > "$LW/crates/demo/tests/t.rs"
printf '%s\n' crates/demo/src/a.rs crates/pr-platform-win/src/w.rs crates/demo/tests/t.rs > "$T/owned.txt"
git -C "$LW" add -A && git -C "$LW" commit -q -m base; LBASE="$(git -C "$LW" rev-parse HEAD)"
lint_case() { # desc expected-rc file content
  printf '%s' "$4" > "$LW/$3"; python $LIBPY lint "$LW" "$T/owned.txt" > "$T/lint.out" 2>&1; local rc=$?
  git -C "$LW" checkout -q -- .; rm -rf "$LW/.claude"
  if [ "$rc" = "$2" ]; then ok "lint: $1"; else bad "lint: $1 (rc $rc, wanted $2)"; cat "$T/lint.out"; fi; }
lint_case "clean change passes" 0 crates/demo/src/a.rs $'pub fn a() -> i32 {\n    1\n}\n'
lint_case "unwrap on a file-system path is rejected" 1 crates/demo/src/a.rs $'pub fn a() {\n    let s = std::fs::read_to_string("x").unwrap();\n}\n'
lint_case "unwrap in #[cfg(test)] code is allowed" 0 crates/demo/src/a.rs $'pub fn a() {}\n#[cfg(test)]\nmod t {\n    fn f() { let s = std::fs::read_to_string("x").unwrap(); }\n}\n'
lint_case "unsafe outside pr-platform-win is rejected" 1 crates/demo/src/a.rs $'pub fn a() {\n    // SAFETY: no\n    unsafe { f() }\n}\n'
lint_case "unsafe without SAFETY comment is rejected" 1 crates/pr-platform-win/src/w.rs $'pub fn w() {\n    unsafe { f() }\n}\n'
lint_case "unsafe with SAFETY comment in pr-platform-win passes" 0 crates/pr-platform-win/src/w.rs $'pub fn w() {\n    // SAFETY: f has no preconditions.\n    unsafe { f() }\n}\n'
lint_case "todo!() left in owned source is rejected" 1 crates/demo/src/a.rs $'pub fn a() {\n    todo!()\n}\n'
lint_case "unimplemented!() is rejected" 1 crates/demo/src/a.rs $'pub fn a() {\n    unimplemented!()\n}\n'
lint_case "#[allow(] without reason is rejected" 1 crates/demo/src/a.rs $'#[allow(dead_code)]\npub fn a() {}\n'
lint_case "#[allow(] with a reason comment passes" 0 crates/demo/src/a.rs $'// reason: kept for the FFI shim\n#[allow(dead_code)]\npub fn a() {}\n'
mkdir -p "$LW/.claude"; printf 'x' > "$LW/.claude/x.txt"
python $LIBPY lint "$LW" "$T/owned.txt" > "$T/lint.out" 2>&1; rc=$?; rm -rf "$LW/.claude"
check "lint: a .claude file in the change set is rejected" '[ $rc = 1 ] && grep -q "\.claude" "$T/lint.out"'
guard_case() { # desc expected-rc content (empty content = delete the file)
  if [ -z "$3" ]; then rm -f "$LW/crates/demo/tests/t.rs"; else printf '%s' "$3" > "$LW/crates/demo/tests/t.rs"; fi
  python $LIBPY testguard "$LW" "$LBASE" "$T/owned.txt" > "$T/guard.out" 2>&1; local rc=$?; git -C "$LW" checkout -q -- .
  if [ "$rc" = "$2" ]; then ok "testguard: $1"; else bad "testguard: $1 (rc $rc, wanted $2)"; cat "$T/guard.out"; fi; }
guard_case "adding an assertion is allowed" 0 $'#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(3, 3);\n    assert_eq!(2, 2);\n}\n'
guard_case "adding a new test fn is allowed" 0 $'#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n\n#[test]\nfn two() {\n    assert_eq!(2, 2);\n}\n'
guard_case "removing an assertion is rejected" 1 $'#[test]\nfn one() {\n    assert_eq!(1, 1);\n}\n'
guard_case "changing an assertion is rejected" 1 $'#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 3);\n}\n'
guard_case "adding #[ignore] is rejected" 1 $'#[test]\n#[ignore]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n'
guard_case "removing the test fn is rejected" 1 $'fn other() {}\n'
guard_case "deleting the test file is rejected" 1 ""

echo "== pack trimming, triage size, helpers"
printf '\xef\xbb\xbf// Copyright (c) Someone\n// Licensed under MIT\n\n\nusing System;\n\n\n\n/// <summary>doc</summary>\nnamespace X {   \n    class A { }\n}\n' | python $LIBPY trimcs 0 > "$T/trim.out"
check "trimcs strips license header, doc lines, blank runs, trailing spaces" '! grep -q "Copyright" "$T/trim.out" && ! grep -q "summary" "$T/trim.out" && head -1 "$T/trim.out" | grep -q "^using System;" && [ "$(grep -c "^$" "$T/trim.out")" -le 1 ] && ! grep -q " $" "$T/trim.out"'
python -c "print('class A {}\n' * 5000)" | python $LIBPY trimcs 2000 > "$T/trim2.out"
check "trimcs caps the size and says so" '[ "$(wc -c < "$T/trim2.out")" -lt 2400 ] && grep -q "truncated by pack" "$T/trim2.out"'
printf 'pub fn a() {}\npub fn a() {}\nimpl X {}\npub fn b() {}\n// c\n' > "$T/sig.rs"
check "sigs de-duplicates" '[ "$(python $LIBPY sigs $T/sig.rs | grep -c "pub fn a")" = 1 ]'
python -c "print('error: boom ' * 4000)" > "$T/bigerr.txt"; printf 'test x::y ... FAILED\n' >> "$T/bigerr.txt"
python $LIBPY triage "$T/tri.md" W8-99a "reason" "$T/bigerr.txt" "$W" ""
check "triage note is capped at 3 KB" '[ "$(wc -c < "$T/tri.md")" -le 3072 ]'
check "tier helper" '[ "$(python $LIBPY tier docs/WAVE-8-PLAN.md W8-90a)" = yellow ] && [ "$(python $LIBPY tier docs/WAVE-8-PLAN.md W8-90c)" = red ] && [ "$(python $LIBPY tier docs/WAVE-8-PLAN.md W8-90e)" = skip ]'
printf '{"type":"thread.started","thread_id":"abc"}\nnoise\n{"type":"turn.completed","usage":{"input_tokens":5,"cached_input_tokens":2,"output_tokens":3}}\n{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}\n' > "$T/cx.log"
check "codexparse reads session id and sums usage" '[ "$(python $LIBPY codexparse $T/cx.log 0)" = "abc|6|2|4" ]'

echo; if [ "$FAILS" = 0 ]; then echo "selftest PASSED"; else echo "selftest: $FAILS FAILED (work dir kept: $T)"; exit 1; fi
cd /; git -C "$W" worktree prune 2>/dev/null; rm -rf "$T"
