#!/usr/bin/env bash
# Claude-free pipeline: port the units of one Wave 8 batch with Codex CLI (gpt-6-luna, effort low) under deterministic
# bash orchestration. No Claude tokens are spent. See README.md ("Claude-free pipeline").
# Usage: tools/autopilot/codex-pipeline.sh <wave-tag e.g. wave8-b2> [--dry-run] [--only W8-04a,W8-04b] [--redo W8-04e,W8-06c] [--jobs N (1..4, default 2)]
#        tools/autopilot/codex-pipeline.sh --smoke        (env checks + one cheap real Codex call + one gate on a throwaway unit)
# Env: CODEX_BIN, CODEX_USAGE_SH, PLAN_REL, CODEX_TIMEOUT (s per Codex call, default 2400), SKIP_MUTANTS=1, PIPELINE_NO_PR=1,
#      MUT_JOBS (cargo-mutants --jobs, default 4), MUT_BUDGET_YELLOW (s, default 900), MUT_BUDGET_RED (s, default 1800),
#      MUTANTS_TIMEOUT (s; overrides both budgets), MUT_TEST_TIMEOUT (s per mutant test run, default 90),
#      MIN_DISK_GB (default 20), QUOTA_MAX (percent of the 5 h window never exceeded, default 99), QUOTA_MARGIN (default 1.2),
#      CS_REPO (C# checkout), GATE_NEXTEST=0 (never use cargo-nextest).
set -u
export PATH="$HOME/.cargo/bin:$PATH"
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PYTHONUTF8=1
LIB="python $SELF/lib/codex-lib.py"
TAG=""; DRY=0; ONLY=""; JOBS=2; SMOKE=0; REDO=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1;; --only) shift; ONLY="${1:-}";; --redo) shift; REDO=",${1:-},";; --jobs) shift; JOBS="${1:-2}";; --smoke) SMOKE=1;;
    -*) echo "unknown arg $1" >&2; exit 2;;
    *) [ -z "$TAG" ] && TAG="$1" || { echo "unexpected arg $1" >&2; exit 2; };;
  esac; shift
done
case "$JOBS" in 1|2|3|4) ;; *) echo "--jobs must be 1..4" >&2; exit 2;; esac
[ "$SMOKE" = 1 ] || [ -n "$TAG" ] || { echo "usage: $0 <wave-tag e.g. wave8-b2> [--dry-run] [--only U1,U2] [--jobs N] | --smoke" >&2; exit 2; }
BATCH="$(echo "${TAG##*-}" | tr a-z A-Z)"
COMMON="$(git -C "$SELF" rev-parse --path-format=absolute --git-common-dir)"; REPO="$(dirname "$COMMON")"
PLAN_REL="${PLAN_REL:-docs/WAVE-8-PLAN.md}"
USAGE="${CODEX_USAGE_SH:-$SELF/codex-usage.sh}"
CODEX_BIN="${CODEX_BIN:-$HOME/.codex/plugins/.plugin-appserver/codex.exe}"; export CODEX_BIN
CS_REPO="${CS_REPO:-C:/MyProjects/PhotoReview}"; export CS_REPO
STATE_DIR="$SELF/state"; STATE="$STATE_DIR/$TAG.json"; LOGDIR="$STATE_DIR/logs"; NEEDS="$STATE_DIR/needs-claude"
UCACHE="$STATE_DIR/usage.cache"
IBR="integration/$TAG"; INT="$REPO/.claude/worktrees/int-$TAG"
TDB="$REPO/target-wave-pipe-$TAG"
CODEX_TIMEOUT="${CODEX_TIMEOUT:-2400}"; MUT_JOBS="${MUT_JOBS:-4}"
QUOTA_MAX="${QUOTA_MAX:-99}"; QUOTA_MARGIN="${QUOTA_MARGIN:-1.2}"
PLAN="$SELF/../../$PLAN_REL"; [ -f "$INT/$PLAN_REL" ] && PLAN="$INT/$PLAN_REL"
say() { local m; m="[$(date +%H:%M:%S)] $*"; echo "$m"; mkdir -p "$LOGDIR"; echo "$m" >> "$LOGDIR/pipeline-${TAG:-smoke}.log"; }
now() { date +%s; }

# ---------------------------------------------------------------- environment preflight
env_checks() { # $1 = skip-lock-check; prints every problem, returns non-zero when any
  local bad=0 gb
  e() { echo "PREFLIGHT FAIL: $*" >&2; bad=1; }
  [ -f "$CODEX_BIN" ] || e "codex binary not found: $CODEX_BIN (set CODEX_BIN)"
  python -c 'import sys;sys.exit(0 if sys.version_info>=(3,8) else 1)' 2>/dev/null || e "python 3.8+ not working (needed by lib/codex-lib.py)"
  [ "$(bash -c 'echo shell-ok' 2>/dev/null)" = shell-ok ] || e "shell host (bash) cannot run commands"
  git -C "$REPO" rev-parse --git-dir >/dev/null 2>&1 || e "git cannot read the repo $REPO (safe.directory? run: git config --global --add safe.directory '$REPO')"
  if [ -d "$CS_REPO" ]; then
    git -c safe.directory="$CS_REPO" -C "$CS_REPO" rev-parse --git-dir >/dev/null 2>&1 || e "C# repo $CS_REPO is not readable by git (safe.directory)"
  else e "C# repo not found at $CS_REPO (set CS_REPO)"; fi
  cargo --version >/dev/null 2>&1 || e "cargo not on PATH"
  if [ "${SKIP_MUTANTS:-0}" != 1 ]; then cargo mutants --version >/dev/null 2>&1 || e "cargo-mutants not installed (cargo install cargo-mutants) or set SKIP_MUTANTS=1"; fi
  if [ "${PIPELINE_NO_PR:-0}" != 1 ]; then command -v gh >/dev/null 2>&1 || e "gh CLI not found (needed to open the PR) or set PIPELINE_NO_PR=1"; fi
  gb="$(df -Pk "$REPO" 2>/dev/null | awk 'NR==2{print int($4/1048576)}')"
  if [ -n "$gb" ] && [ "$gb" -lt "${MIN_DISK_GB:-20}" ]; then e "only ${gb} GB free on the repo drive (need ${MIN_DISK_GB:-20} GB)"; fi
  if [ -z "${1:-}" ] && [ -d "$STATE_DIR/pipeline.lock" ]; then
    local op; op="$(cat "$STATE_DIR/pipeline.lock/pid" 2>/dev/null)"
    [ -n "$op" ] && kill -0 "$op" 2>/dev/null && e "another pipeline is running (pid $op)"
  fi
  [ -n "${CARGO_BUILD_JOBS:-}" ] || echo "note: CARGO_BUILD_JOBS is not set; on a weak machine set it (e.g. 4) to avoid memory pressure" >&2
  return $bad
}

smoke() {
  env_checks || { echo "smoke: environment checks failed, aborting" >&2; exit 4; }
  local out rc D sid tin tc tout
  out="$(bash "$USAGE" "$QUOTA_MAX" 2>/dev/null </dev/null | head -1)"
  case "$out" in *'"ok": true'*|*'"ok":true'*) echo "smoke: quota ok: $out";; *) echo "smoke: Codex quota not available ($out); not making the real call" >&2; exit 5;; esac
  D="$(mktemp -d)"; mkdir -p "$D/crates/smoke/src" "$D/crates/smoke/tests" "$D/contracts"
  printf '[workspace]\nmembers = ["crates/smoke"]\nresolver = "2"\n' > "$D/Cargo.toml"
  printf '[package]\nname = "smoke"\nversion = "0.1.0"\nedition = "2021"\n' > "$D/crates/smoke/Cargo.toml"
  printf 'pub fn one() -> i32 {\n    1\n}\n' > "$D/crates/smoke/src/lib.rs"
  printf '#[test]\nfn one_is_one() {\n    assert_eq!(smoke::one(), 1);\n}\n' > "$D/crates/smoke/tests/t.rs"
  printf '# W0-00a\n\n## Owned files (the ONLY files the porter may create or modify)\n    crates/smoke/src/lib.rs\n    crates/smoke/tests/t.rs\n' > "$D/contracts/W0-00a.md"
  printf 'target/\n.codex-*\n' > "$D/.gitignore"
  cp "$REPO/rust-toolchain.toml" "$D/" 2>/dev/null
  ( cd "$D" && git init -q -b master && git config user.name smoke && git config user.email smoke@example.com && git add -A && git commit -q -m init ) || { echo "smoke: git in throwaway dir failed" >&2; exit 4; }
  echo "Reply with the single word OK." > "$D/.codex-p.txt"
  timeout 300 bash "$SELF/codex-run.sh" "$D" "$D/.codex-p.txt" "$D/.codex-last-message.txt" > "$D/.codex-log" 2>&1 </dev/null; rc=$?
  IFS='|' read -r sid tin tc tout < <($LIB codexparse "$D/.codex-log" 0 | tr -d '\r')
  if [ "$rc" != 0 ] || ! grep -qi 'ok' "$D/.codex-last-message.txt" 2>/dev/null; then
    echo "smoke: real Codex call FAILED (exit $rc). Log tail:" >&2; tail -n 15 "$D/.codex-log" >&2; exit 6; fi
  [ -n "$sid" ] || { echo "smoke: Codex call worked but no session id in the --json log (resume would not work)" >&2; exit 6; }
  echo "smoke: Codex ok: session $sid, tokens in=$tin cached=$tc out=$tout"
  export CARGO_TARGET_DIR="$D/target"
  bash "$SELF/codex-gate.sh" W0-00a "$D" smoke fast > "$D/.codex-gate.txt" 2>&1 </dev/null || { echo "smoke: gate FAILED on a good unit:" >&2; cat "$D/.codex-gate.txt" >&2; exit 7; }
  printf 'pub fn one() -> i32 {\n    todo!()\n}\n' > "$D/crates/smoke/src/lib.rs"
  bash "$SELF/codex-gate.sh" W0-00a "$D" smoke fast > "$D/.codex-gate.txt" 2>&1 </dev/null && { echo "smoke: gate PASSED a unit with todo!() (static gate broken)" >&2; exit 7; }
  echo "smoke: gate ok (passes a good unit, rejects todo!())"
  rm -rf "$D"
  echo "smoke: ALL OK"; exit 0
}
[ "$SMOKE" = 1 ] && smoke

# ---------------------------------------------------------------- dry run
mapfile -t ORDER < <($LIB order "$PLAN" "$BATCH" "$ONLY" | tr -d '\r')
[ "${#ORDER[@]}" -gt 0 ] || { echo "no units for batch $BATCH in $PLAN_REL" >&2; exit 1; }
if [ "$DRY" = 1 ]; then
  echo "plan: tag=$TAG batch=$BATCH integration=$IBR units=${#ORDER[@]} jobs=$JOBS state=$STATE"
  n=0
  for l in "${ORDER[@]}"; do
    n=$((n+1)); u="${l%%$'\t'*}"; d="${l#*$'\t'}"
    st="$($LIB sget "$STATE" "$u")"; c="$SELF/../../contracts/$u.md"
    printf '%3d. %-8s deps=%-28s state=%-8s mutation=%-6s contract=%s\n' "$n" "$u" "${d:--}" "${st:--}" "$($LIB tier "$PLAN" "$u" | tr -d '\r')" "$([ -f "$c" ] && echo yes || echo MISSING)"
  done
  exit 0
fi

# ---------------------------------------------------------------- lock + cleanup
mkdir -p "$STATE_DIR" "$LOGDIR" "$NEEDS"; LOCK="$STATE_DIR/pipeline.lock"
if ! mkdir "$LOCK" 2>/dev/null; then
  op="$(cat "$LOCK/pid" 2>/dev/null)"
  if [ -n "$op" ] && kill -0 "$op" 2>/dev/null; then echo "another pipeline is running (pid $op)" >&2; exit 3; fi
  rm -rf "$LOCK"; mkdir "$LOCK" || exit 3
fi
echo $$ > "$LOCK/pid"
MAINPID=$$
cleanup_targets() { # target dirs of THIS pipeline only (explicit names), at the very end
  local d
  for d in "$TDB" "$TDB"-*; do [ -d "$d" ] && rm -rf "$d"; done   # explicit prefix of THIS pipeline: -s<slot>, -mut, -x<n> alternates
}
on_exit() { [ "$BASHPID" = "$MAINPID" ] || return 0; cleanup_targets; rm -rf "$LOCK"; }
trap on_exit EXIT; trap 'say "interrupted"; exit 130' INT TERM

env_checks skiplock || { echo "preflight failed, nothing was started" >&2; exit 4; }

# ---------------------------------------------------------------- quota
UOUT=""; URC=0
usage_call() { # [fresh] -> UOUT (json line), URC (0 = Codex may be used, below QUOTA_MAX)
  local ct cj
  if [ "${1:-}" != fresh ] && [ -f "$UCACHE" ]; then
    { read -r ct; read -r cj; } < "$UCACHE" 2>/dev/null
    if [ -n "${ct:-}" ] && [ $(( $(now) - ct )) -lt "${USAGE_CACHE_TTL:-45}" ]; then UOUT="$cj"; URC=0; return; fi
  fi
  UOUT="$(bash "$USAGE" "$QUOTA_MAX" 2>/dev/null </dev/null | head -1; exit "${PIPESTATUS[0]}")"; URC=$?
  [ "$URC" = 0 ] && { printf '%s\n%s\n' "$(now)" "$UOUT" > "$UCACHE.$$" && mv -f "$UCACHE.$$" "$UCACHE"; }
}
quota_used() { local u r; IFS='|' read -r u r < <(printf '%s' "$UOUT" | $LIB used | tr -d '\r'); QUSED="$u"; QRESET="$r"; }

# wait_quota [need-percent-free]: blocks (sleeping until the 5 h reset, never busy-looping, never using Claude) while the 5 h
# window is >= QUOTA_MAX % used or has less than <need> % left.
wait_quota() {
  local need="${1:-0}" rt s why
  while :; do
    usage_call; quota_used; why=""
    if [ "$URC" != 0 ]; then why="used ${QUSED:-?}% >= ${QUOTA_MAX}% or limit reached"
    elif [ "$need" != 0 ] && [ -n "$QUSED" ] && awk -v u="$QUSED" -v n="$need" 'BEGIN{exit !((100-u) < n)}'; then why="only $(awk -v u="$QUSED" 'BEGIN{print 100-u}')% left, need ${need}%"; fi
    [ -z "$why" ] && return 0
    s=300
    if [ -n "$QRESET" ] && rt="$(date -d "$QRESET" +%s 2>/dev/null)"; then s=$(( rt - $(now) + 120 )); [ "$s" -lt 120 ] && s=120; fi
    [ "$s" -gt 1800 ] && s=1800
    [ -n "${QUOTA_SLEEP_OVERRIDE:-}" ] && s="$QUOTA_SLEEP_OVERRIDE"
    say "codex quota not available ($why; resets=${QRESET:-?}); sleeping ${s}s (never falling back to Claude)"
    sleep "$s"
  done
}
start_need() { # percent that must be free before a unit starts: avg cost per unit x QUOTA_MARGIN (0 when unknown), capped at 95
  local est; est="$($LIB avgpct "$STATE" | tr -d '\r')"
  [ -n "$est" ] && awk -v e="$est" -v m="$QUOTA_MARGIN" 'BEGIN{n=e*m; if(n>95)n=95; printf "%.1f", n}' || echo 0
}

# ---------------------------------------------------------------- codex calls (session reuse, telemetry)
prompt_head() { # unit crate target (only sent when no session exists yet)
  printf 'Read the file .codex-pack.md in the current directory first: it holds the contract, the plan row, the C# sources, the signatures of dependency units and the rules. Do not explore the repo.\nUnit: %s. Crate: %s. Environment: CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0, CARGO_TARGET_DIR=%s (already exported; keep it). Do not commit.\n\n' "$1" "$2" "$3"
}

# codex_call <worktree> <prompt-body-file> <unit> <label>
# First call of a unit: head (pack instructions) + body. Later calls: `codex exec resume <SID>` with the short body only.
# Uses/sets the unit-local SID and CUR_CRATE. Adds rounds, tokens and seconds to the state file. Retries on rate-limit messages.
codex_call() {
  local wt="$1" pf="$2" u="$3" label="$4" lg="$LOGDIR/$3.log" tries=0 rc sz t0 sid tin tc tout send used_sid fallback=0
  while :; do
    wait_quota 0
    sz=$(wc -c < "$lg" 2>/dev/null || echo 0); t0=$(now)
    used_sid="$SID"; send="$pf.send"
    if [ -n "$used_sid" ]; then cp "$pf" "$send"; else { prompt_head "$u" "$CUR_CRATE" "$CARGO_TARGET_DIR"; cat "$pf"; } > "$send"; fi
    echo "=== $label $(date) ${used_sid:+resume $used_sid}" >> "$lg"
    timeout "$CODEX_TIMEOUT" bash "$SELF/codex-run.sh" "$wt" "$send" "$wt/.codex-last-message.txt" "$used_sid" >> "$lg" 2>&1 </dev/null
    rc=$?
    IFS='|' read -r sid tin tc tout < <($LIB codexparse "$lg" "$sz" | tr -d '\r')
    $LIB supd "$STATE" "$u" "rounds+=1" "tin+=${tin:-0}" "tcached+=${tc:-0}" "tout+=${tout:-0}" "secs+=$(( $(now) - t0 ))" >/dev/null
    [ -z "$SID" ] && [ -n "$sid" ] && SID="$sid"
    if [ "$rc" -ne 0 ] && tail -c +$((sz+1)) "$lg" | tail -n 60 | grep -qiE 'rate.?limit|usage limit|quota|try again (later|in)'; then
      tries=$((tries+1)); say "$u $label: rate-limit message from Codex (try $tries)"
      if [ "$tries" -le 5 ]; then sleep "${QUOTA_SLEEP_OVERRIDE:-60}"; continue; fi
    elif [ "$rc" -ne 0 ] && [ -n "$used_sid" ] && [ "$fallback" = 0 ]; then
      fallback=1; say "$u $label: resume of session $used_sid failed (exit $rc), falling back to a fresh session with the pack"; SID=""; continue
    fi
    say "$u $label: codex exit $rc (session ${SID:-none}, tokens in=${tin:-0} out=${tout:-0})"
    return "$rc"
  done
}

# ---------------------------------------------------------------- gate, repair
# gate_loop <unit> <wt> <crate> <start-mode> <stop-mode>: repair rounds (max 3) on the same Codex session; the error tail is <= 60 lines.
gate_loop() {
  local u="$1" wt="$2" crate="$3" mode="$4" stop="$5" round=0 out rc pf
  while :; do
    out="$(GATE_TEST_BASE="${REDBASE:-}" bash "$SELF/codex-gate.sh" "$u" "$wt" "$crate" "$mode" 2>&1 </dev/null)"; rc=$?
    if [ "$rc" = 0 ]; then
      [ "$mode" = "$stop" ] && return 0
      mode="$stop"; continue
    fi
    if [ "$round" -ge 3 ]; then say "$u gate ($mode) still failing after 3 repair rounds"; GATE_ERR="$out"; return 1; fi
    say "$u gate ($mode) failed (rc $rc), repair round $((round+1))/3"
    round=$((round+1)); pf="$LOGDIR/$u.repair$round.prompt.md"
    { printf 'TASK: repair. The machine gate (%s) failed. Fix the owned files only; never delete or weaken tests. Tail (<= 60 lines):\n\n```\n' "$mode"; printf '%s\n' "$out" | tail -n 60; printf '```\n'; } > "$pf"
    codex_call "$wt" "$pf" "$u" "repair$round"
    mode=fast
  done
}

# ---------------------------------------------------------------- mutation (cargo mutants --in-diff, owned src files only)
MUT_EXCLUDE=(-E 'impl (\w+::)*(Debug|Display|Default) for' -E '::fmt( |$)')
# run_mutants <unit> <wt> <crate> <tier> [survivors-only]; sets SURV (-1 = not run), KILLED, BUDG, BUDGET_HIT, MUT_NOTE, MUT_USED
run_mutants() {
  local u="$1" wt="$2" crate="$3" tier="$4" only="${5:-}" d="$LOGDIR/$1.mutants" rc budget left t0 ta w st k sv un to bg
  local -a CTA=() FARG=() SRCS=()
  [ -z "$only" ] && { SURV=-1; KILLED=0; BUDG=0; BUDGET_HIT=0; MUT_USED=0; MUT_NOTE=""; }
  if [ "$tier" = skip ]; then MUT_NOTE="mutation skipped (green or golden-covered)"; return; fi
  if [ "${SKIP_MUTANTS:-0}" = 1 ] || ! cargo mutants --version >/dev/null 2>&1; then MUT_NOTE="mutants skipped (not installed or SKIP_MUTANTS)"; return; fi
  budget="${MUTANTS_TIMEOUT:-$([ "$tier" = red ] && echo "${MUT_BUDGET_RED:-1800}" || echo "${MUT_BUDGET_YELLOW:-900}")}"
  left=$((budget - MUT_USED))
  if [ "$left" -lt 30 ]; then BUDGET_HIT=1; MUT_NOTE="mutants budget (${budget}s) exhausted"; return; fi
  ta="$($LIB testargs "$wt" "$wt/contracts/$u.md" | tr -d '\r')"
  [ -n "$ta" ] || { MUT_NOTE="mutants skipped (unit has no own test binaries)"; return; }
  for w in $ta; do CTA+=("--cargo-test-arg=$w"); done
  for w in "${OWN[@]}"; do case "$w" in */src/*.rs) SRCS+=("$w");; esac; done
  ( cd "$wt" && git add -A -- . ':!.codex-*' && git diff --cached HEAD -- "${SRCS[@]}" > "$LOGDIR/$u.diff" )
  [ -s "$LOGDIR/$u.diff" ] || { MUT_NOTE="mutants skipped (empty diff)"; return; }
  if [ -n "$only" ] && [ -s "$LOGDIR/$u.survivors.txt" ]; then
    cp "$LOGDIR/$u.survivors.txt" "$LOGDIR/$u.survivors.prev"; FARG=(-F "$($LIB mutregex "$LOGDIR/$u.survivors.prev" | tr -d '\r')")
  fi
  rm -rf "$d"; t0=$(now)
  # Dedicated mutants target dir (never the unit's own: mutant builds must not leak into the real worktree's artifacts), reused across units.
  ( cd "$wt" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR}-mut" CARGO_PROFILE_DEV_DEBUG=0 timeout "$left" cargo mutants -p "$crate" --in-diff "$LOGDIR/$u.diff" --baseline skip \
      --timeout "${MUT_TEST_TIMEOUT:-90}" -j "$MUT_JOBS" --no-shuffle --output "$d" "${MUT_EXCLUDE[@]}" "${CTA[@]}" "${FARG[@]}" ) > "$LOGDIR/$u.mutants.log" 2>&1 </dev/null
  rc=$?; MUT_USED=$((MUT_USED + $(now) - t0))
  case "$rc" in 0|2|3) ;; 124) BUDGET_HIT=1; MUT_NOTE="mutants budget (${budget}s) exhausted, partial result";; *) MUT_NOTE="mutants exit $rc"; SURV=-1; return;; esac
  read -r k sv un to bg < <($LIB mutstat "$d" | tr -d '\r')
  : > "$LOGDIR/$u.survivors.txt"; [ -f "$d/mutants.out/missed.txt" ] && cp "$d/mutants.out/missed.txt" "$LOGDIR/$u.survivors.txt"
  KILLED=$((KILLED + k)); SURV=$sv
  if [ -n "$only" ]; then BUDG=$((BUDG + bg)); else BUDG=$bg; fi
  [ "$bg" -gt 0 ] && BUDGET_HIT=1
  return 0
}

# ---------------------------------------------------------------- failures
# fail_unit <unit> <wt> <reason> [error-file]: marks the unit failed and writes the (<= 3 KB) needs-claude triage note.
fail_unit() {
  local u="$1" wt="$2" why="$3" ef="${4:-}"
  $LIB triage "$NEEDS/$u.md" "$u" "$why" "$ef" "$wt" "$wt/.codex-last-message.txt" >/dev/null 2>&1
  $LIB sset "$STATE" "$u" failed "$why; triage: $NEEDS/$u.md; worktree kept: $wt" >/dev/null
  $LIB supd "$STATE" "$u" "secs=$(( $(now) - UNIT_T0 ))" >/dev/null
}

with_merge_lock() { # one lock per integration branch
  local l="$STATE_DIR/merge-$TAG.lock" op
  while ! mkdir "$l" 2>/dev/null; do
    op="$(cat "$l/pid" 2>/dev/null)"; { [ -z "$op" ] || ! kill -0 "$op" 2>/dev/null; } && [ -d "$l" ] && [ -n "$op" ] && rm -rf "$l"
    sleep 2
  done
  echo "$BASHPID" > "$l/pid"; "$@"; local rc=$?; rm -rf "$l"; return $rc
}

# ---------------------------------------------------------------- per-slot target dirs, rerun cleanup, red-test gate
# tdir_free <dir>: 0 when no live cargo/rustc holds the dir's build lock. A leftover process of a killed earlier run (or a Codex cargo that
# outlived its timeout) keeps .cargo-build-lock open: the next cargo then dies with "Access is denied" on that file.
tdir_free() {
  local f
  for f in "$1/debug/.cargo-build-lock" "$1/release/.cargo-build-lock" "$1/.cargo-lock"; do
    [ -e "$f" ] || continue
    { : >> "$f"; } 2>/dev/null || return 1
  done
  return 0
}
# pick_target_dir <slot>: one dir per slot (never one dir for two concurrently running units); if the slot's dir is locked by a stray
# process, a fresh alternate (-x<n>) is used instead of failing. Sets CARGO_TARGET_DIR.
pick_target_dir() {
  local slot="$1" base n=0 d
  if [ "$JOBS" -gt 1 ]; then base="$TDB-s$slot"; else base="$TDB"; fi
  d="$base"
  while ! tdir_free "$d" || ! tdir_free "$d-mut"; do n=$((n+1)); say "target dir $d is locked by another process; using $base-x$n"; d="$base-x$n"; done
  export CARGO_TARGET_DIR="$d"
}

# reset_unit <unit> <wt> <branch>: makes a failed/stale unit rerunnable. Removes ANY worktree (wherever it lives) that has the unit's own
# branch checked out, the leftover directory (only under .claude/worktrees/wu-*), the branch (its old tip is kept under
# refs/pipeline-old/<unit> when it held unmerged commits), the stale triage note and the unit's old state/telemetry.
reset_unit() {
  local u="$1" wt="$2" br="$3" p tip
  while IFS= read -r p; do
    [ -n "$p" ] && [ "$p" != "$INT" ] && git -C "$REPO" worktree remove --force --force "$p" >>"$LOGDIR/$u.log" 2>&1
  done < <(git -C "$REPO" worktree list --porcelain | tr -d '\r' | awk -v b="refs/heads/$br" '/^worktree /{w=substr($0,10)} $1=="branch" && $2==b {print w}')
  case "$wt" in "$REPO"/.claude/worktrees/wu-*) [ -d "$wt" ] && rm -rf "$wt";; esac
  git -C "$REPO" worktree prune
  if git -C "$REPO" show-ref --verify --quiet "refs/heads/$br"; then
    tip="$(git -C "$REPO" rev-parse "$br")"
    git -C "$REPO" merge-base --is-ancestor "$tip" "$IBR" 2>/dev/null || git -C "$REPO" update-ref "refs/pipeline-old/$u" "$tip"
    git -C "$REPO" branch -D "$br" >>"$LOGDIR/$u.log" 2>&1
  fi
  rm -f "$NEEDS/$u.md"
  $LIB sreset "$STATE" "$u" >/dev/null
}

# red_check <unit> <wt> <crate>: 0 = the owned test files exist, only they changed, compile (`cargo test --no-run`) and every failing
# test fails ONLY through todo!(). Otherwise prints a short error tail. The red-test guard (GATE_TEST_BASE) is NOT used here.
red_check() {
  local u="$1" wt="$2" crate="$3" f bad="" ta out T
  T="$LOGDIR/$u.red.out"
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    case "$f" in .codex-*) continue;; esac
    printf '%s\n' "${OWN[@]}" | grep -qxF "$f" || bad="$bad$f"$'\n'
    case "$f" in */tests/*) ;; *) bad="$bad$f (not a test file: red tests must not touch src)"$'\n';; esac
  done < <(cd "$wt" && git status --porcelain -uall | sed -e 's/^...//' -e 's/.* -> //' -e 's/^"//' -e 's/"$//')
  if [ -n "$bad" ]; then printf 'Files changed that are not owned test files (revert them):\n%s' "$bad"; return 1; fi
  ta="$($LIB testargs "$wt" "$wt/contracts/$u.md" | tr -d '\r')"
  [ -n "$ta" ] || { echo "No owned test file exists yet: create the owned files under tests/."; return 1; }
  ( cd "$wt" && cargo test -p "$crate" $ta --no-run ) > "$T" 2>&1 </dev/null || { echo "RED TESTS DO NOT COMPILE (cargo test -p $crate $ta --no-run):"; grep -vE '^\s*(Compiling|Finished|Blocking|Fresh|Downloaded)' "$T" | tail -n 55; return 1; }
  ( cd "$wt" && timeout "${RED_RUN_TIMEOUT:-900}" cargo test -p "$crate" $ta --no-fail-fast ) > "$T" 2>&1 </dev/null
  out="$($LIB redcheck "$T" 2>&1)" || { printf '%s\n' "$out" | tail -n 40; return 1; }
  return 0
}
# red_loop <unit> <wt> <crate>: red_check with up to 3 Codex repair rounds on the same session. Sets RED_ERR when it gives up.
red_loop() {
  local u="$1" wt="$2" crate="$3" round=0 out pf
  while :; do
    out="$(red_check "$u" "$wt" "$crate")" && return 0
    if [ "$round" -ge 3 ]; then say "$u red tests still invalid after 3 repair rounds"; RED_ERR="$out"; return 1; fi
    round=$((round+1)); say "$u red tests invalid, repair round $round/3"
    pf="$LOGDIR/$u.redrepair$round.prompt.md"
    { printf 'TASK: red-repair. The red tests are not valid yet. Edit ONLY the owned test files under tests/ (never src). They must compile and fail only because the stubs call todo!(); use only items that really exist (read the stub and the dependency signatures in the pack, do not invent APIs). Tail (<= 60 lines):\n\n```\n'; printf '%s\n' "$out" | tail -n 60; printf '```\n'; } > "$pf"
    codex_call "$wt" "$pf" "$u" "red-repair$round"
  done
}

# ---------------------------------------------------------------- one unit
# run_unit <unit> <slot> : state done/partial = merged, failed otherwise. Runs in a subshell (its variables are private).
run_unit() {
  local u="$1" slot="$2" lcu wt br crate c pf has_t=0 f round tier pct0 pct1 packout pb
  UNIT_T0=$(now); SID=""; CUR_CRATE=""; REDBASE=""
  lcu="$(echo "$u" | tr A-Z a-z)"; wt="$REPO/.claude/worktrees/wu-$lcu"; br="wu/$u"
  # one target dir per integration tag, shared by units run one after another (deps compile once); one per slot when jobs > 1
  pick_target_dir "$slot"
  export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0
  : > "$LOGDIR/$u.log"
  usage_call fresh; quota_used; pct0="$QUSED"
  reset_unit "$u" "$wt" "$br"
  git -C "$REPO" worktree add -q -b "$br" "$wt" "$IBR" >>"$LOGDIR/$u.log" 2>&1 || { fail_unit "$u" "$wt" "worktree add failed"; return 1; }
  # the Codex sandbox may write only to its cwd (per-workspace SID, an ACL grant on a shared dir does not work): per-unit target INSIDE the worktree
  export CARGO_TARGET_DIR="$wt/target"
  c="$wt/contracts/$u.md"; [ -f "$c" ] || { fail_unit "$u" "$wt" "no contract $c"; return 1; }
  mapfile -t OWN < <($LIB owned "$c" | tr -d '\r')
  [ "${#OWN[@]}" -gt 0 ] || { fail_unit "$u" "$wt" "contract lists no owned files"; return 1; }
  crate="$(printf '%s\n' "${OWN[@]}" | grep -m1 '^crates/' | cut -d/ -f2)"; CUR_CRATE="$crate"
  [ -n "$crate" ] || { fail_unit "$u" "$wt" "cannot derive crate"; return 1; }
  tier="$($LIB tier "$PLAN" "$u" | tr -d '\r')"; $LIB supd "$STATE" "$u" "tier=$tier" >/dev/null
  # the pack is built exactly once per unit and sent only in the first Codex call; later calls resume the session
  packout="$(bash "$SELF/codex-pack.sh" "$u" "$wt" 2>>"$LOGDIR/$u.log")" || { fail_unit "$u" "$wt" "pack failed"; return 1; }
  pb="$(printf '%s' "$packout" | sed -n 's/.*(\([0-9]*\) bytes).*/\1/p' | tail -1)"; $LIB supd "$STATE" "$u" "pack_bytes=${pb:-0}" >/dev/null
  say "$u: pack ${pb:-?} bytes, mutation tier $tier"

  for f in "${OWN[@]}"; do case "$f" in */tests/*) [ -f "$wt/$f" ] && has_t=1;; esac; done
  # red tests already on the integration branch (e.g. written by Claude): the Codex red step is skipped entirely
  if [ "$has_t" = 0 ]; then
    pf="$LOGDIR/$u.red.prompt.md"
    printf 'TASK: red-tests. Write ONLY the owned test files under tests/ (they do not exist yet), porting the C# tests named in the contract. Do not edit any file under src/. The tests must compile against the stubs and fail only because the stubs are not implemented (todo!() or wrong values), never because of compile errors. Confirm with `cargo test -p %s --no-run`.\n' "$crate" > "$pf"
    codex_call "$wt" "$pf" "$u" red-tests
    # compile gate: the red tests must compile and fail only through todo!() (up to 3 repair rounds; no red-test guard in this phase)
    RED_ERR=""
    if ! red_loop "$u" "$wt" "$crate"; then
      printf '%s\n' "$RED_ERR" > "$LOGDIR/$u.gate-error.txt"; fail_unit "$u" "$wt" "red tests invalid after 3 repair rounds" "$LOGDIR/$u.gate-error.txt"; return 1
    fi
    # freeze the red tests in their own commit: later gates verify they are only ever extended, never weakened
    ( cd "$wt" && git add -A -- . ':!.codex-*' && git commit -q --allow-empty -m "test($u): red tests (Codex pipeline)" ) >>"$LOGDIR/$u.log" 2>&1
  fi
  REDBASE="$(git -C "$wt" rev-parse HEAD)"
  pf="$LOGDIR/$u.impl.prompt.md"
  printf 'TASK: implement. Implement the owned src files per the contract and the C# source so that every test of this unit passes. Do not edit the test files unless a test provably contradicts the C# source (say so in the summary). Run cargo fmt, then `cargo check -p %s --all-targets` and the unit tests until green.\n' "$crate" > "$pf"
  codex_call "$wt" "$pf" "$u" implement

  GATE_ERR=""
  if ! gate_loop "$u" "$wt" "$crate" fast fast; then
    printf '%s\n' "$GATE_ERR" > "$LOGDIR/$u.gate-error.txt"; fail_unit "$u" "$wt" "gate failed after 3 repair rounds" "$LOGDIR/$u.gate-error.txt"; return 1
  fi

  # full gate (clippy + the unit's own test binaries) BEFORE mutants: when it fails, the ~15 min mutants run is not wasted on code
  # that is about to change. It runs again after the mutant rounds only if those rounds changed the tests.
  GATE_ERR=""
  if ! gate_loop "$u" "$wt" "$crate" full full; then
    printf '%s\n' "$GATE_ERR" > "$LOGDIR/$u.gate-error.txt"; fail_unit "$u" "$wt" "gate (clippy + crate tests) failed after 3 repair rounds" "$LOGDIR/$u.gate-error.txt"; return 1
  fi

  run_mutants "$u" "$wt" "$crate" "$tier"; round=0
  while [ "$SURV" -gt 0 ] && [ "$round" -lt 2 ] && [ "$BUDGET_HIT" = 0 ]; do
    round=$((round+1)); say "$u: $SURV surviving mutants, test round $round/2"
    ( cd "$wt" && git add -A -- . ':!.codex-*' && git stash create > "$LOGDIR/$u.snap" )
    pf="$LOGDIR/$u.mut$round.prompt.md"
    { printf 'TASK: kill-mutants. These mutants of the owned src files survived (no test failed). Add or strengthen tests ONLY in the owned test files so each is caught (exact values, edge cases, error paths); do not edit src. Survivors:\n\n```\n'; head -n 60 "$LOGDIR/$u.survivors.txt"; printf '```\n'; } > "$pf"
    codex_call "$wt" "$pf" "$u" "mutants$round"
    if ! GATE_TEST_BASE="$REDBASE" bash "$SELF/codex-gate.sh" "$u" "$wt" "$crate" fast > "$LOGDIR/$u.mut$round.gate.txt" 2>&1 </dev/null; then
      say "$u: gate failed after mutant tests; restoring previous state"
      ( cd "$wt" && git reset -q --hard HEAD && git clean -fdq -e '.codex-*' && git stash apply -q --index "$(cat "$LOGDIR/$u.snap")" ) >>"$LOGDIR/$u.log" 2>&1
      break
    fi
    run_mutants "$u" "$wt" "$crate" "$tier" only
  done
  [ "$SURV" -lt 0 ] && SURV=0

  # final full gate only when the mutant rounds added or changed tests (otherwise the gate above already covers this exact tree)
  if [ "$round" -gt 0 ]; then
    GATE_ERR=""
    if ! gate_loop "$u" "$wt" "$crate" full full; then
      printf '%s\n' "$GATE_ERR" > "$LOGDIR/$u.gate-error.txt"; fail_unit "$u" "$wt" "final gate (clippy + crate tests) failed after 3 repair rounds" "$LOGDIR/$u.gate-error.txt"; return 1
    fi
  fi

  ( cd "$wt" && git add -A -- . ':!.codex-*' && git commit -q -m "feat($u): port $u via Codex pipeline" -m "Co-Authored-By: Codex gpt-6-luna <noreply@openai.com>" ) >>"$LOGDIR/$u.log" 2>&1 \
    || { fail_unit "$u" "$wt" "commit failed"; return 1; }
  merge_unit() { git -C "$INT" merge --no-ff -q "$br" -m "Merge $u (Codex pipeline)" >>"$LOGDIR/$u.log" 2>&1 || { git -C "$INT" merge --abort >/dev/null 2>&1; return 1; }; }
  if ! with_merge_lock merge_unit; then
    fail_unit "$u" "$wt" "merge into $IBR conflicted; branch $br kept"; return 1
  fi
  usage_call fresh; quota_used; pct1="$QUSED"
  [ -n "$pct0" ] && [ -n "$pct1" ] && $LIB supd "$STATE" "$u" "pct=$(awk -v a="$pct0" -v b="$pct1" 'BEGIN{d=b-a; if(d<0)d=0; print d}')" >/dev/null
  local mn="${MUT_NOTE:-}"; [ "$BUDGET_HIT" = 1 ] && [ -z "$mn" ] && mn="mutants budget exhausted"
  [ "$tier" != skip ] && $LIB supd "$STATE" "$u" "killed=$KILLED" "budgeted=$BUDG" >/dev/null
  $LIB supd "$STATE" "$u" "secs=$(( $(now) - UNIT_T0 ))" >/dev/null
  if [ "$SURV" -gt 0 ]; then $LIB sset "$STATE" "$u" partial "$SURV surviving mutants${mn:+; $mn}" "$SURV" >/dev/null
  elif [ "$BUDGET_HIT" = 1 ]; then $LIB sset "$STATE" "$u" partial "${mn:-mutants budget exhausted}" 0 >/dev/null
  else $LIB sset "$STATE" "$u" done "$mn" 0 >/dev/null; fi
  git -C "$REPO" worktree remove --force "$wt" >/dev/null 2>&1
  return 0
}

# ---------------------------------------------------------------- integration branch
git -C "$REPO" fetch -q origin 2>/dev/null
if [ ! -d "$INT" ]; then
  git -C "$REPO" worktree prune
  if git -C "$REPO" show-ref --verify --quiet "refs/heads/$IBR"; then git -C "$REPO" worktree add -q "$INT" "$IBR"
  else git -C "$REPO" worktree add -q -b "$IBR" "$INT" origin/master; fi || { echo "cannot create integration worktree" >&2; exit 1; }
fi
PLAN="$INT/$PLAN_REL"
mapfile -t ORDER < <($LIB order "$PLAN" "$BATCH" "$ONLY" | tr -d '\r')
say "pipeline $TAG: ${#ORDER[@]} units, integration $IBR, jobs $JOBS"
# units already merged on the integration branch (merge commits since origin/master, e.g. "Merge W8-04a (Codex pipeline)" or
# "merge(W8-15c)") are marked done, so no seeded state file is needed after a restart or a manual merge
MERGED_ON_IBR="$(git -C "$REPO" log --merges --format=%s origin/master.."$IBR" 2>/dev/null | grep -oE 'W[0-9]+-[0-9]+[a-z]+' | sort -u)"
for l in "${ORDER[@]}"; do
  u="${l%%$'\t'*}"
  if [ -n "$REDO" ] && case "$REDO" in *",$u,"*) true;; *) false;; esac; then $LIB sset "$STATE" "$u" failed "redo requested" 0 >/dev/null; say "$u: redo requested, state reset"; continue; fi
  case "$($LIB sget "$STATE" "$u")" in done|partial) continue;; esac
  if printf '%s\n' "$MERGED_ON_IBR" | grep -qxF "$u"; then $LIB sset "$STATE" "$u" done "already merged on $IBR" 0 >/dev/null; say "$u: found merged on $IBR, marked done"; fi
done

# ---------------------------------------------------------------- scheduler (independent units concurrently, <= JOBS)
declare -A LIVE=(); SLOTPID=(); SLOTU=()
PENDING=("${ORDER[@]}")
for l in "${PENDING[@]}"; do LIVE["${l%%$'\t'*}"]=1; done
running=0
reap() {
  local i p
  for i in "${!SLOTPID[@]}"; do
    p="${SLOTPID[$i]}"
    if [ -n "$p" ] && ! kill -0 "$p" 2>/dev/null; then
      wait "$p" 2>/dev/null; unset "LIVE[${SLOTU[$i]}]"
      say "${SLOTU[$i]}: $($LIB sget "$STATE" "${SLOTU[$i]}") $($LIB sfield "$STATE" "${SLOTU[$i]}" note | tr -d '\r')"
      SLOTPID[$i]=""; running=$((running-1))
    fi
  done
}
free_slot() { local i; for ((i=1;i<=JOBS;i++)); do [ -z "${SLOTPID[$i]:-}" ] && { echo "$i"; return; }; done; }

while :; do
  reap
  REMAIN=(); started=0
  for l in "${PENDING[@]}"; do
    u="${l%%$'\t'*}"; deps="${l#*$'\t'}"
    case "$($LIB sget "$STATE" "$u")" in done|partial) say "$u: already merged ($($LIB sget "$STATE" "$u")), skipping"; unset "LIVE[$u]"; continue;; esac
    bad=""; waitdep=0
    for d in ${deps//,/ }; do
      if [ -n "${LIVE[$d]:-}" ]; then waitdep=1; continue; fi
      ds="$($LIB sget "$STATE" "$d")"
      case "$ds" in done|partial) ;; *) bad="$d (${ds:-not run})";; esac
    done
    if [ -n "$bad" ]; then $LIB sset "$STATE" "$u" skipped "dependency $bad is not merged" >/dev/null; say "$u: skipped, dependency $bad"; unset "LIVE[$u]"; continue; fi
    if [ "$waitdep" = 1 ] || [ "$running" -ge "$JOBS" ]; then REMAIN+=("$l"); continue; fi
    need="$(start_need)"; wait_quota "$need"
    slot="$(free_slot)"; say "$u: start (slot $slot, quota need ${need}%)"
    ( run_unit "$u" "$slot" ) &
    SLOTPID[$slot]=$!; SLOTU[$slot]="$u"; running=$((running+1)); started=$((started+1))
  done
  PENDING=("${REMAIN[@]}")
  [ "${#PENDING[@]}" -eq 0 ] && [ "$running" -eq 0 ] && break
  if [ "$running" -eq 0 ] && [ "$started" -eq 0 ]; then say "scheduler: ${#PENDING[@]} units cannot start; stopping"; break; fi
  [ "$running" -gt 0 ] && wait -n 2>/dev/null
done

# ---------------------------------------------------------------- workspace gate, push, PR
merged=$($LIB slist "$STATE" | awk -F'\t' '$2=="done"||$2=="partial"' | grep -c .)
say "telemetry:"; $LIB table "$STATE"
[ "$merged" -gt 0 ] || { say "no unit merged; nothing to push"; $LIB slist "$STATE"; exit 1; }
export CARGO_TARGET_DIR="$TDB" CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0
WG="$LOGDIR/workspace-gate-$TAG.log"; WGOK=1
say "workspace gate on $IBR"
( cd "$INT" && bash "$SELF/lib/fmtcheck.sh" --all && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked ) > "$WG" 2>&1 </dev/null || WGOK=0
say "workspace gate: $([ $WGOK = 1 ] && echo OK || echo FAILED)"
if [ "${PIPELINE_NO_PR:-0}" = 1 ]; then say "PIPELINE_NO_PR=1: not pushing"; exit $((1-WGOK)); fi

BODY="$LOGDIR/pr-body-$TAG.md"
{
  echo "Wave 8 batch $BATCH ported by the Claude-free Codex pipeline (Codex gpt-6-luna, effort low). Review the diff before merging."; echo
  echo "Workspace gate (fmt, clippy --workspace -D warnings, test --workspace): $([ $WGOK = 1 ] && echo PASSED || echo '**FAILED**, auto-merge not enabled')"
  if [ $WGOK = 0 ]; then echo '```'; tail -n 40 "$WG"; echo '```'; fi
  echo; echo "## Telemetry"; $LIB table "$STATE"
  echo; echo "## Units"
  $LIB slist "$STATE" | awk -F'\t' '{printf "- %s: **%s**%s\n",$1,$2,($4!=""?" - "$4:"")}'
  echo; echo "## Surviving mutants (tests missing)"
  any=0
  for u in $($LIB slist "$STATE" | awk -F'\t' '$2=="partial" && $3>0{print $1}'); do
    any=1; echo "- $u"; echo '```'; head -n 15 "$LOGDIR/$u.survivors.txt" 2>/dev/null; echo '```'
  done
  [ $any = 0 ] && echo "none"
  echo; echo "## Needs Claude (failed or skipped units)"
  fl=$($LIB slist "$STATE" | awk -F'\t' '$2=="failed"||$2=="skipped"{printf "- %s: %s - %s\n",$1,$2,$4}')
  if [ -n "$fl" ]; then echo "$fl"; else echo "none"; fi
  for u in $($LIB slist "$STATE" | awk -F'\t' '$2=="failed"{print $1}'); do
    [ -f "$NEEDS/$u.md" ] && { echo; echo "<details><summary>$u triage</summary>"; echo; cat "$NEEDS/$u.md"; echo; echo "</details>"; }
  done
  echo; echo "🤖 Generated with [Claude Code](https://claude.com/claude-code)"
} > "$BODY"
git -C "$INT" push -q -u origin "$IBR" >>"$LOGDIR/pipeline-$TAG.log" 2>&1 || { say "push failed"; exit 1; }
if ( cd "$INT" && gh pr view "$IBR" >/dev/null 2>&1 ); then
  ( cd "$INT" && gh pr edit "$IBR" --body-file "$BODY" >/dev/null 2>&1 )
else
  ( cd "$INT" && gh pr create --base master --head "$IBR" --title "feat: Wave 8 $BATCH ported by Codex pipeline ($TAG)" --body-file "$BODY" ) >>"$LOGDIR/pipeline-$TAG.log" 2>&1 || say "gh pr create failed"
fi
if [ $WGOK = 1 ]; then ( cd "$INT" && gh pr merge "$IBR" --auto --squash ) >>"$LOGDIR/pipeline-$TAG.log" 2>&1; fi
say "done. Summary:"; $LIB slist "$STATE"
