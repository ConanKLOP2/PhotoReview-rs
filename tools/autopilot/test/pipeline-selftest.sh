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
W_WIN="$(cygpath -w "$W")"
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
printf 'pub mod u90a;\npub mod u90b;\npub mod u90c;\npub mod u90e;\npub mod u90f;\npub mod u90g;\npub mod u90h;\npub mod u90i;\npub mod u90j;\n' > crates/demo/src/lib.rs
printf 'pub fn run(_x: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90a.rs
printf 'pub fn run(_a: i32, _b: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u90b.rs
for n in 90c 90e 90f 90g 90h 90i; do printf 'pub fn run(_a: i32) -> i32 {\n    todo!()\n}\n' > crates/demo/src/u$n.rs; done
printf 'pub fn run(_a: i32) -> i32 {\n    0\n}\n' > crates/demo/src/u90j.rs   # stub that does NOT call todo!()
printf 'target/\ntarget-mut/\n.claude/worktrees/\n/target-wave-*/\ntools/autopilot/state/\n.codex-*\n' > .gitignore
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
mkc W8-90g merged; mkc W8-90h preexisting; mkc W8-90i redfix; mkc W8-90j redbad; mkc W8-90a sign; mkc W8-90b sub; mkc W8-90c double; mkc W8-90d never; mkc W8-90e inc; mkc W8-90f triple
for n in 1 2 3 4 5; do
  printf 'pub mod w9x%s;\n' "$n" >> crates/demo/src/lib.rs
  printf 'pub fn run() -> i32 {\n    todo!()\n}\n' > "crates/demo/src/w9x$n.rs"
  cat > "contracts/W9-X$n.md" <<X
# W9-X$n
- Baseline C# \`7eaee2a2\`.

## Owned files (the ONLY files the porter may create or modify)
    crates/demo/src/w9x$n.rs
    crates/demo/tests/w9x$n.rs
X
done
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
| W8-90g | g | g | green | Haiku | G | none | B7 |
| W8-90h | h | h | green | Haiku | G | none | B7 |
| W8-90i | i | i | green | Haiku | G | none | B6 |
| W8-90j | j | j | green | Haiku | G | none | B6 |
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
  SID="sess-$RANDOM$RANDOM"; U="$(printf '%s' "$P" | grep -o 'Unit: W[A-Za-z0-9-]*' | head -1 | cut -d' ' -f2)"; U="${U#W8-}"; echo "$U" > "$FS/sess-$SID"
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
  red-tests:90i) tst 90i '#[test]
fn needs_arity() {
    assert_eq!(demo::u90i::run(1, 2), 5);
}';;   # does not compile: run takes one argument
  red-repair:90i) tst 90i '#[test]
fn needs_arity() {
    assert_eq!(demo::u90i::run(1), 5);
}';;
  implement:90i) src 90i 'pub fn run(a: i32) -> i32 {
    a + 4
}';;
  red-tests:90j) tst 90j '#[test]
fn wrong_value() {
    assert_eq!(demo::u90j::run(1), 2);
}';;   # compiles, but fails by assertion (stub returns 0), not through todo!()
  red-repair:90j) :;;
  implement:90j) src 90j 'pub fn run(_a: i32) -> i32 {
    0
}';;
  fixup:90j) tst 90j '#[test]
fn wrong_value() {
    assert_eq!(demo::u90j::run(1), 0);
}'; echo 'C# oracle: Oracle.cs CSharpCase; changed Rust test: wrong_value' > "$O";;
  implement:90h) src 90h 'pub fn run(a: i32) -> i32 {
    a + 10
}';;
  red-tests:90f) tst 90f '#[test]
fn triple() {
    assert_eq!(demo::u90f::run(2), 6);
}';;
  implement:90f) src 90f 'pub fn run(a: i32) -> i32 {
    a * 3
}';;
  red-tests:W9-X1|red-tests:W9-X2|red-tests:W9-X3)
    n="${U#W9-X}"; mkdir -p crates/demo/tests
    printf '%s\n' '#[test]' 'fn returns_value() {' "    assert_eq!(demo::w9x$n::run(), $((10+n)));" '}' > "crates/demo/tests/w9x$n.rs";;
  implement:W9-X1|implement:W9-X2|implement:W9-X3)
    n="${U#W9-X}"; printf 'pub fn run() -> i32 {\n    %s\n}\n' "$((10+n))" > "crates/demo/src/w9x$n.rs";;
esac
if [ "$TASK" = fixup ]; then echo 'C# oracle: Oracle.cs CSharpCase; changed Rust test: wrong_value' > "$O"; else echo "summary: done" > "$O"; fi
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
export MUTANTS_TIMEOUT=600 CODEX_TIMEOUT=120 CS_REPO="$T/cs" MIN_DISK_GB=0
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

echo "== format check (per crate, never the whole workspace on one command line)"
bash tools/autopilot/lib/fmtcheck.sh --all > "$T/fmt1.out" 2>&1; rc=$?
check "fmtcheck --all passes on a formatted workspace" '[ $rc = 0 ]'
cp crates/demo/src/u90a.rs "$T/u90a.bak"; printf 'pub fn   run( _x:i32 )->i32 { todo!() }\n' > crates/demo/src/u90a.rs
bash tools/autopilot/lib/fmtcheck.sh -p demo > "$T/fmt2.out" 2>&1; rc=$?
check "fmtcheck -p <crate> reports an unformatted file (exit 1, names the crate and the file)" '[ $rc = 1 ] && grep -q "== fmt: demo" "$T/fmt2.out" && grep -q "u90a" "$T/fmt2.out"'
FMT_FORCE_BATCH=1 bash tools/autopilot/lib/fmtcheck.sh --all > "$T/fmt3.out" 2>&1; rc=$?
check "the batch fallback (command line too long) also catches it" '[ $rc = 1 ] && grep -q "u90a" "$T/fmt3.out"'
cp "$T/u90a.bak" crates/demo/src/u90a.rs; FMT_FORCE_BATCH=1 bash tools/autopilot/lib/fmtcheck.sh --all > "$T/fmt4.out" 2>&1; rc=$?
check "the batch fallback passes on formatted code" '[ $rc = 0 ]'
check "no codex-pipeline/gate script calls a bare workspace-wide cargo fmt" '! grep -nE "cargo fmt( --all)? *(--check|-- --check)|cargo fmt --check" tools/autopilot/codex-pipeline.sh tools/autopilot/codex-gate.sh | grep -v "^[^:]*:[0-9]*:#" | grep -v "cargo fmt -p"'
git status --short | grep -q . && bad "selftest left the work clone dirty" || ok "work clone clean after the fmt checks"

echo "== red-test output check (helper)"
RC_OK=$'running 2 tests\ntest a ... FAILED\ntest b ... ok\n\nfailures:\n\n---- a stdout ----\n\nthread \'a\' panicked at crates/x/tests/t.rs:3:5:\nnot yet implemented\nnote: run with RUST_BACKTRACE=1\n\n\nfailures:\n    a\n\ntest result: FAILED. 1 passed; 1 failed\n'
printf '%s' "$RC_OK" > "$T/rc1.txt"; python $LIBPY redcheck "$T/rc1.txt" > /dev/null; rc=$?
check "redcheck: failure through todo!() is accepted" '[ $rc = 0 ]'
printf '%s' "${RC_OK//not yet implemented/assertion \`left == right\` failed}" > "$T/rc2.txt"; python $LIBPY redcheck "$T/rc2.txt" > "$T/rc2.out"; rc=$?
check "redcheck: failure by assertion is rejected and named" '[ $rc = 1 ] && grep -q "a fails but not through todo" "$T/rc2.out"'
printf 'error[E0425]: cannot find value `x` in this scope\nerror: could not compile `demo` (test "t") due to 1 previous error\n' > "$T/rc3.txt"; python $LIBPY redcheck "$T/rc3.txt" > /dev/null; rc=$?
check "redcheck: compile error is rejected" '[ $rc = 1 ]'
printf 'test c ... FAILED\n\nfailures:\n    c\n' > "$T/rc4.txt"; python $LIBPY redcheck "$T/rc4.txt" > /dev/null; rc=$?
check "redcheck: failure without any panic output (crash/timeout) is rejected" '[ $rc = 1 ]'

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
check "90c failed after its fixup also left the stray edit" '[ "$($SG W8-90c)" = failed ] && [ "$(grep -c "^repair 90c" $FS/calls)" = 6 ] && [ "$(grep -c "^fixup 90c" $FS/calls)" = 1 ]'
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

echo "== rerun of a failed unit whose worktree was moved (used to die with 'worktree add failed')"
git -C "$W" worktree move "$W/.claude/worktrees/wu-w8-90c" "$W/.claude/worktrees/elsewhere" 2>/dev/null
check "precondition: wu/W8-90c is checked out in a worktree at another path" 'git worktree list --porcelain | tr -d "\r" | grep -q "branch refs/heads/wu/W8-90c" && [ -d .claude/worktrees/elsewhere ]'
$P --only W8-90c --jobs 1 > "$T/run2b.out" 2>&1; rc=$?
check "rerun reset the unit instead of failing on the worktree" '! grep -q "worktree add failed" tools/autopilot/state/test-b9.json && [ ! -d .claude/worktrees/elsewhere ]'
check "rerun really started the unit again (Codex red/implement calls, fresh telemetry: rounds < 10)" '[ "$(grep -c "^implement 90c" $FS/calls)" = 2 ] && [ "$($SF W8-90c rounds)" -lt 10 ]'
check "the rerun also gets one failed fixup after three gate repairs" '[ "$(grep -c "^repair 90c" $FS/calls)" = 12 ] && [ "$(grep -c "^fixup 90c" $FS/calls)" = 2 ]'
check "the unit's old unmerged tip was kept under refs/pipeline-old" 'git show-ref --verify -q refs/pipeline-old/W8-90c'
check "the stale triage note was replaced by the new one" '[ -f tools/autopilot/state/needs-claude/W8-90c.md ]'
check "the rerun left other units untouched" '[ "$($SG W8-90b)" = done ] && [ "$($SG W8-90a)" = done ]'

echo "== target dir locked by a stray process (Access is denied on .cargo-build-lock)"
sed -n '/^tdir_free()/,/^}/p' tools/autopilot/codex-pipeline.sh > "$T/tdir.sh"; . "$T/tdir.sh"
check "tdir_free: a dir without a build lock is free" 'tdir_free "$T/td-none"'
if uname -s | grep -qiE 'mingw|msys|cygwin'; then
  mkdir -p "$T/td/debug"; : > "$T/td/debug/.cargo-build-lock"
  python - "$T/td/debug/.cargo-build-lock" <<'PY' &
import ctypes, sys, time
k = ctypes.windll.kernel32
k.CreateFileW.restype = ctypes.c_void_p
h = k.CreateFileW(sys.argv[1], 0xC0000000, 0, None, 3, 0x80, None)   # read+write, share mode 0: nobody else may open it
open(sys.argv[1] + ".held", "w").close()
time.sleep(120)
PY
  HP=$!; for i in 1 2 3 4 5 6 7 8 9 10; do [ -e "$T/td/debug/.cargo-build-lock.held" ] && break; sleep 1; done
  check "tdir_free: a dir whose build lock is held by a stray process is reported busy" '! tdir_free "$T/td"'
  kill $HP 2>/dev/null; wait $HP 2>/dev/null; sleep 1
  check "tdir_free: free again once the stray process is gone" 'tdir_free "$T/td"'
else ok "tdir_free lock-held case skipped (not Windows)"; fi
check "each parallel slot gets its own target dir (-s<slot>) and mutants its own -mut" 'grep -q "TDB-s\$slot" tools/autopilot/codex-pipeline.sh && grep -q "CARGO_TARGET_DIR}-mut" tools/autopilot/codex-pipeline.sh'

echo "== red tests: compile gate with repair rounds, pre-existing red tests, units already merged on the integration branch"
git branch side master && git checkout -q side && mkdir -p crates/demo/tests && printf '#[test]\nfn pre_existing() {\n    assert_eq!(demo::u90h::run(1), 11);\n}\n' > crates/demo/tests/u90h.rs && git add crates/demo/tests/u90h.rs && git commit -q -m 'test(W8-90h): red tests (pre-existing)' && git checkout -q -b integration/test-b7 master && git merge -q --no-ff side -m 'merge(W8-90g)' && git checkout -q master
bash tools/autopilot/codex-pipeline.sh test-b7 --jobs 1 > "$T/run5.out" 2>&1; rc=$?
check "unit merged on the integration branch (merge commit) is marked done without a state file" 'grep -q "W8-90g: found merged on integration/test-b7" "$T/run5.out" && [ "$(python $LIBPY sget tools/autopilot/state/test-b7.json W8-90g)" = done ] && ! grep -q " 90g" $FS/calls'
check "red tests already on the integration branch: no Codex red step, the unit is implemented and merged" '! grep -q "^red-tests 90h" $FS/calls && grep -q "^implement 90h" $FS/calls && [ "$(python $LIBPY sget tools/autopilot/state/test-b7.json W8-90h)" = done ]'
export FIXUP=0; bash tools/autopilot/codex-pipeline.sh test-b6 --jobs 1 > "$T/run6.out" 2>&1; rc=$?; unset FIXUP
check "red tests that do not compile get a repair round and then pass the gate" 'grep -q "^red-tests 90i" $FS/calls && [ "$(grep -c "^red-repair 90i" $FS/calls)" = 1 ] && [ "$(python $LIBPY sget tools/autopilot/state/test-b6.json W8-90i)" = done ]'
check "the repair prompt carried the compiler error" 'grep -q "RED TESTS DO NOT COMPILE" $LOGS/W8-90i.redrepair1.prompt.md && grep -q "TASK: red-repair" $LOGS/W8-90i.redrepair1.prompt.md'
check "red tests are committed on their own after the gate" 'git log integration/test-b6 --format=%s | grep -q "test(W8-90i): red tests"'
check "FIXUP=0 preserves invalid-red failure after 3 repair rounds" '[ "$(python $LIBPY sget tools/autopilot/state/test-b6.json W8-90j)" = failed ] && [ "$(grep -c "^red-repair 90j" $FS/calls)" = 3 ] && ! grep -q "^implement 90j" $FS/calls'
check "triage note for invalid red tests explains why" 'grep -q "not through todo" tools/autopilot/state/needs-claude/W8-90j.md'
check "the red-test guard was not applied during the red phase (no 'red-test guard' failure)" '! grep -q "red-test guard" $LOGS/W8-90i.log $LOGS/W8-90j.log'
FIXUP=1 bash tools/autopilot/codex-pipeline.sh test-b6 --only W8-90j --redo W8-90j --jobs 1 > "$T/run6-fixup.out" 2>&1; rc=$?
check "fixup corrects a red test against the fake C# oracle and completes" '[ "$(python $LIBPY sget tools/autopilot/state/test-b6.json W8-90j)" = done ] && [ "$(python $LIBPY sfield tools/autopilot/state/test-b6.json W8-90j fixup)" = 1 ] && grep -q "^fixup 90j" $FS/calls'
check "fixup correction is its own commit and names the changed test" 'git log integration/test-b6 --format=%s | grep -q "test(W8-90j): corrected tests (Codex fixup)" && [ "$(python $LIBPY sfield tools/autopilot/state/test-b6.json W8-90j fixup_tests)" = wrong_value ]'
check "fixup relaxed guard retains the test and assert counts" '! grep -q "number of test functions decreased\|assert macro occurrences decreased\|marked #\[ignore\]" $LOGS/W8-90j.fixup-guard.txt'

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

echo "== Wave 9 plan parsing and external dependencies"
cat > docs/WAVE-9-PLAN-fake.md <<'X'
| Unit | C# files | Rust | Col | Test | Depends on |
|---|---|---|---|---|---|
| W9-X1 | a | a | yellow | F | none |
| W9-X2 | b | b | yellow | F | X1 |
| W9-X3 | c | c | yellow | F | X1, W8-90a |
| W9-X4 | d | d | yellow | F | W8-77z |
| W9-X5 | e | e | yellow | F | X4 |
| W9-X6 | f | f | yellow | F | W8-18h..k; W8-14c/14e/14f; `W8-99q`; W8-18o (stub); Not E8 |
X
git add docs/WAVE-9-PLAN-fake.md && git commit -q -m "Add fake Wave 9 plan" && git push -q origin master
check "Wave 9 plan auto lookup finds rows by tag lane" '[ "$(python $LIBPY planfor "$W" wave9-x)" = docs/WAVE-9-PLAN-fake.md ]'
cat > docs/WAVE-9-PLAN-fake-e.md <<'X'
| Unit | C# files | Rust | Col | Test | Depends on |
|---|---|---|---|---|---|
| W9-E1 | a | a | yellow | F | none |
X
cat > docs/WAVE-9-PLAN-fake-r.md <<'X'
| Unit | C# files | Rust | Col | Test | Depends on |
|---|---|---|---|---|---|
| W9-R1 | a | a | yellow | F | none |
X
check "planfor finds the Wave 9 E lane plan" '[ "$(python $LIBPY planfor "$W" wave9-e)" = docs/WAVE-9-PLAN-fake-e.md ]'
check "planfor finds the Wave 9 R lane plan" '[ "$(python $LIBPY planfor "$W" wave9-r)" = docs/WAVE-9-PLAN-fake-r.md ]'
W9_ORDER_EXPECTED="$(printf 'W9-X1\t\nW9-X2\tW9-X1\nW9-X3\tW9-X1\nW9-X4\t\nW9-X5\tW9-X4\nW9-X6\t')"
check "Wave 9 dependency order lists same-lane dependencies only" '[ "$(python $LIBPY order docs/WAVE-9-PLAN-fake.md X)" = "$W9_ORDER_EXPECTED" ]'
check "Wave 9 deps returns its same-lane unit edge" '[ "$(python $LIBPY deps docs/WAVE-9-PLAN-fake.md W9-X2)" = W9-X1 ]'
W9_EXT_EXPECTED="$(printf 'W8-18h\nW8-18i\nW8-18j\nW8-18k\nW8-14c\nW8-14e\nW8-14f\nW8-18o')"
check "Wave 9 extdeps expands ranges and slash lists, ignores backticks and Not tail" '[ "$(python $LIBPY extdeps docs/WAVE-9-PLAN-fake.md W9-X6)" = "$W9_EXT_EXPECTED" ]'
W8_ORDER_EXPECTED="$(printf 'W8-90a\t\nW8-90b\tW8-90a\nW8-90c\tW8-90a\nW8-90d\tW8-90c\nW8-90e\t')"
check "Wave 8 order output remains byte-for-byte stable" '[ "$(python $LIBPY order docs/WAVE-8-PLAN.md B9)" = "$W8_ORDER_EXPECTED" ]'
check "Wave 8 row output remains byte-for-byte stable" '[ "$(python $LIBPY row docs/WAVE-8-PLAN.md W8-90a)" = "| W8-90a | a | a | yellow | Sonnet | F | none | B9 |" ]'
check "Wave 8 deps and tier output remain byte-for-byte stable" '[ "$(python $LIBPY deps docs/WAVE-8-PLAN.md W8-90b)" = W8-90a ] && [ "$(python $LIBPY tier docs/WAVE-8-PLAN.md W8-90a)" = yellow ]'
check "missing Wave 9 plan reports lane-specific error" 'python $LIBPY planfor "$W" wave9-z > "$T/noplan.out" 2>&1; [ $? = 1 ] && grep -q "no Wave 9 plan with rows W9-Z\* in docs/" "$T/noplan.out"'
cp docs/WAVE-9-PLAN-fake.md docs/WAVE-9-PLAN-fake2.md
check "ambiguous Wave 9 plan lookup lists matching files" 'python $LIBPY planfor "$W" wave9-x > "$T/ambig.out" 2>&1; [ $? = 1 ] && grep -q "WAVE-9-PLAN-fake2.md" "$T/ambig.out"'
rm docs/WAVE-9-PLAN-fake2.md
python $LIBPY sset tools/autopilot/state/test-other.json W8-90a done "other tag" 0
check "external dependency is satisfied by another tag state" '[ -z "$(python $LIBPY extsat "$W" tools/autopilot/state W8-90a)" ]'
check "external dependency absent from master and all state remains unsatisfied" '[ "$(python $LIBPY extsat "$W" tools/autopilot/state W8-77z)" = W8-77z ]'
git -C "$W" -c user.name=selftest -c user.email=selftest@example.com commit -q --allow-empty -m "Complete W8-18p layout unit"
git -C "$W" push -q origin master
check "external dependency is satisfied by a token in origin/master history" '[ -z "$(python $LIBPY extsat "$W_WIN" tools/autopilot/state W8-18p)" ]'
git -C "$W" -c user.name=selftest -c user.email=selftest@example.com commit -q --allow-empty -m "feat(W8-18l..18n): fake range implementation"
git -C "$W" -c user.name=selftest -c user.email=selftest@example.com commit -q --allow-empty -m "perf(W8-18x..z): fake short-range implementation"
git -C "$W" -c user.name=selftest -c user.email=selftest@example.com commit -q --allow-empty -m "fix(W8-18a, W8-18c): fake listed implementation"
git -C "$W" -c user.name=selftest -c user.email=selftest@example.com commit -q --allow-empty -m "refactor(W8-18p/18q): fake slash-list implementation"
git -C "$W" -c user.name=selftest -c user.email=selftest@example.com commit -q --allow-empty -m "docs(W8-B5): contracts (W8-18a..18v)"
git -C "$W" push -q origin master
check "external dependency range with full endpoint matches a unit inside it" '[ -z "$(python $LIBPY extsat "$W_WIN" tools/autopilot/state W8-18m)" ]'
check "external dependency range with short endpoint matches a unit inside it" '[ -z "$(python $LIBPY extsat "$W_WIN" tools/autopilot/state W8-18y)" ]'
check "external dependency comma-list subject matches a listed unit" '[ -z "$(python $LIBPY extsat "$W_WIN" tools/autopilot/state W8-18c)" ]'
check "external dependency slash-list subject matches a listed unit" '[ -z "$(python $LIBPY extsat "$W_WIN" tools/autopilot/state W8-18q)" ]'
check "docs commit mentioning a unit range does not satisfy it" '[ "$(python $LIBPY extsat "$W_WIN" tools/autopilot/state W8-18s)" = W8-18s ]'
check "W9 mutation tier reads six-column Test and Col cells" '[ "$(python $LIBPY tier docs/WAVE-9-PLAN-fake.md W9-X1)" = yellow ]'
echo "== Wave 9 pipeline dry-run and fake Codex run"
bash tools/autopilot/codex-pipeline.sh wave9-x --dry-run > "$T/w9-dry.out" 2>&1; rc=$?
check "wave9-x auto-detects its plan and dry-run shows internal and external deps" '[ $rc = 0 ] && grep -q "W9-X3.*deps=W9-X1.*extdeps=-" "$T/w9-dry.out" && grep -q "W9-X4.*extdeps=W8-77z" "$T/w9-dry.out"'
bash tools/autopilot/codex-pipeline.sh wave9-x --plan docs/WAVE-9-PLAN-fake.md --dry-run > "$T/w9-cli-plan.out" 2>&1; rc=$?
check "--plan selects a Wave 9 plan" '[ $rc = 0 ] && grep -q "units=6" "$T/w9-cli-plan.out"'
cat > docs/WAVE-9-PLAN-alt.md <<'X'
| Unit | C# files | Rust | Col | Test | Depends on |
|---|---|---|---|---|---|
| W9-X1 | a | a | yellow | F | none |
X
PLAN_REL=docs/WAVE-9-PLAN-alt.md bash tools/autopilot/codex-pipeline.sh wave9-x --dry-run > "$T/w9-env-plan.out" 2>&1; rc=$?
check "PLAN_REL overrides the automatically selected plan" '[ $rc = 0 ] && grep -q "units=1" "$T/w9-env-plan.out"'
rm docs/WAVE-9-PLAN-alt.md
MIN_DISK_GB=0 SKIP_MUTANTS=1 PIPELINE_NO_PR=1 bash tools/autopilot/codex-pipeline.sh wave9-x --only W9-X1,W9-X2,W9-X3,W9-X4,W9-X5 --jobs 1 > "$T/w9-run.out" 2>&1; rc=$?
check "W9 fake Codex run finishes X1-X3 and skips unsatisfied X4 and dependent X5" '[ $rc = 0 ] && [ "$(python $LIBPY sget tools/autopilot/state/wave9-x.json W9-X1)" = done ] && [ "$(python $LIBPY sget tools/autopilot/state/wave9-x.json W9-X2)" = done ] && [ "$(python $LIBPY sget tools/autopilot/state/wave9-x.json W9-X3)" = done ] && [ "$(python $LIBPY sget tools/autopilot/state/wave9-x.json W9-X4)" = skipped ] && [ "$(python $LIBPY sget tools/autopilot/state/wave9-x.json W9-X5)" = skipped ]'
check "unsatisfied external dependency skip includes the waiting note" 'grep -q "waiting for W8-77z (not on master)" "$T/w9-run.out" && grep -q "W9-X4.*skipped" "$T/w9-run.out" && grep -q "W9-X5.*skipped" "$T/w9-run.out"'
check "Wave 9 units merge into integration/wave9-x" 'git log integration/wave9-x --format=%s | grep -q "Merge W9-X1" && git log integration/wave9-x --format=%s | grep -q "Merge W9-X3"'
check "Wave 9 pipeline uses Wave 9 PR title and body wording" 'grep -q "Wave \$wave batch \$BATCH" tools/autopilot/codex-pipeline.sh && grep -q "feat: Wave \$wave" tools/autopilot/codex-pipeline.sh'

echo; if [ "$FAILS" = 0 ]; then echo "selftest PASSED"; else echo "selftest: $FAILS FAILED (work dir kept: $T)"; exit 1; fi
cd /; git -C "$W" worktree prune 2>/dev/null; rm -rf "$T"
