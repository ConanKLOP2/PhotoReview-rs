#!/usr/bin/env bash
# Claude-free pipeline: port the units of one Wave 8 batch with Codex CLI (gpt-6-luna, effort low) under deterministic
# bash orchestration. No Claude tokens are spent. See README.md ("Claude-free pipeline").
# Usage: tools/autopilot/codex-pipeline.sh <wave-tag e.g. wave8-b2> [--dry-run] [--only W8-04a,W8-04b]
# Env: CODEX_BIN (codex binary), CODEX_USAGE_SH (quota script), PLAN_REL, MUTANTS_TIMEOUT (s, default 1500),
#      CODEX_TIMEOUT (s per Codex call, default 2400), SKIP_MUTANTS=1, PIPELINE_NO_PR=1.
set -u
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PYTHONUTF8=1
LIB="python $SELF/lib/codex-lib.py"
TAG="${1:-}"; shift || true
[ -n "$TAG" ] || { echo "usage: $0 <wave-tag e.g. wave8-b2> [--dry-run] [--only U1,U2]" >&2; exit 2; }
DRY=0; ONLY=""
while [ $# -gt 0 ]; do
  case "$1" in --dry-run) DRY=1;; --only) shift; ONLY="${1:-}";; *) echo "unknown arg $1" >&2; exit 2;; esac; shift
done
BATCH="$(echo "${TAG##*-}" | tr a-z A-Z)"
COMMON="$(git -C "$SELF" rev-parse --path-format=absolute --git-common-dir)"; REPO="$(dirname "$COMMON")"
PLAN_REL="${PLAN_REL:-docs/WAVE-8-PLAN.md}"
USAGE="${CODEX_USAGE_SH:-$SELF/codex-usage.sh}"
STATE_DIR="$SELF/state"; STATE="$STATE_DIR/$TAG.json"; LOGDIR="$STATE_DIR/logs"
IBR="integration/$TAG"; INT="$REPO/.claude/worktrees/int-$TAG"
CODEX_TIMEOUT="${CODEX_TIMEOUT:-2400}"; MUTANTS_TIMEOUT="${MUTANTS_TIMEOUT:-1500}"
PLAN="$SELF/../../$PLAN_REL"; [ -f "$INT/$PLAN_REL" ] && PLAN="$INT/$PLAN_REL"
say() { local m; m="[$(date +%H:%M:%S)] $*"; echo "$m"; mkdir -p "$LOGDIR"; echo "$m" >> "$LOGDIR/pipeline-$TAG.log"; }

# ---------------------------------------------------------------- dry run
mapfile -t ORDER < <($LIB order "$PLAN" "$BATCH" "$ONLY" | tr -d '\r')
[ "${#ORDER[@]}" -gt 0 ] || { echo "no units for batch $BATCH in $PLAN_REL" >&2; exit 1; }
if [ "$DRY" = 1 ]; then
  echo "plan: tag=$TAG batch=$BATCH integration=$IBR units=${#ORDER[@]} state=$STATE"
  n=0
  for l in "${ORDER[@]}"; do
    n=$((n+1)); u="${l%%$'\t'*}"; d="${l#*$'\t'}"
    st="$($LIB sget "$STATE" "$u")"; c="$SELF/../../contracts/$u.md"
    printf '%3d. %-8s deps=%-28s state=%-8s contract=%s\n' "$n" "$u" "${d:--}" "${st:--}" "$([ -f "$c" ] && echo yes || echo MISSING)"
  done
  exit 0
fi

# ---------------------------------------------------------------- lock
mkdir -p "$STATE_DIR" "$LOGDIR"; LOCK="$STATE_DIR/pipeline.lock"
if ! mkdir "$LOCK" 2>/dev/null; then
  op="$(cat "$LOCK/pid" 2>/dev/null)"
  if [ -n "$op" ] && kill -0 "$op" 2>/dev/null; then echo "another pipeline is running (pid $op)" >&2; exit 3; fi
  rm -rf "$LOCK"; mkdir "$LOCK" || exit 3
fi
echo $$ > "$LOCK/pid"
trap 'rm -rf "$LOCK"' EXIT; trap 'say "interrupted"; exit 130' INT TERM

# ---------------------------------------------------------------- helpers
wait_quota() {
  local out rc used reset now rt s
  while :; do
    out="$(bash "$USAGE" 99 2>/dev/null </dev/null | head -1; exit "${PIPESTATUS[0]}")"; rc=$?
    [ "$rc" = 0 ] && return 0
    used="$(printf '%s' "$out" | python -c 'import sys,json;print(json.loads(sys.stdin.read()).get("five_hour_used",""))' 2>/dev/null)"
    reset="$(printf '%s' "$out" | python -c 'import sys,json;print(json.loads(sys.stdin.read()).get("five_hour_resets_at",""))' 2>/dev/null)"
    s=300
    if [ -n "$reset" ] && rt="$(date -d "$reset" +%s 2>/dev/null)"; then
      now="$(date +%s)"; s=$((rt - now + 120)); [ "$s" -lt 120 ] && s=120
    fi
    [ "$s" -gt 1800 ] && s=1800
    [ -n "${QUOTA_SLEEP_OVERRIDE:-}" ] && s="$QUOTA_SLEEP_OVERRIDE"
    say "codex quota not available (5h used=${used:-?}, resets=${reset:-?}); sleeping ${s}s (never falling back to Claude)"
    sleep "$s"
  done
}

# codex_call <worktree> <prompt-file> <unit> <label>: waits for quota, runs Codex, retries on rate-limit messages.
codex_call() {
  local wt="$1" pf="$2" u="$3" label="$4" lg="$LOGDIR/$3.log" tries=0 rc sz
  while :; do
    wait_quota
    sz=$(wc -c < "$lg" 2>/dev/null || echo 0)
    echo "=== $label $(date)" >> "$lg"
    timeout "$CODEX_TIMEOUT" bash "$SELF/codex-run.sh" "$wt" "$pf" "$wt/.codex-last-message.txt" >> "$lg" 2>&1 </dev/null
    rc=$?
    if [ "$rc" -ne 0 ] && tail -c +$((sz+1)) "$lg" | tail -n 60 | grep -qiE 'rate.?limit|usage limit|quota|try again (later|in)'; then
      tries=$((tries+1)); say "$u $label: rate-limit message from Codex (try $tries)"
      if [ "$tries" -le 5 ]; then sleep "${QUOTA_SLEEP_OVERRIDE:-60}"; continue; fi
    fi
    say "$u $label: codex exit $rc"
    return "$rc"
  done
}

prompt_head() { # unit crate target
  printf 'Read the file .codex-pack.md in the current directory first: it holds the contract, the plan row, the C# sources, the signatures of dependency units and the rules. Do not explore the repo.\nUnit: %s. Crate: %s. Environment: CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0, CARGO_TARGET_DIR=%s (already exported; keep it). Do not commit.\n\n' "$1" "$2" "$3"
}

gate_loop() { # unit wt crate ; sets GATE_ERR ; returns 0 pass / 1 fail after 3 repair rounds
  local u="$1" wt="$2" crate="$3" round=0 out rc pf
  while :; do
    out="$(bash "$SELF/codex-gate.sh" "$u" "$wt" "$crate" 2>&1 </dev/null)"; rc=$?
    if [ "$rc" = 0 ]; then return 0; fi
    if [ "$round" -ge 3 ]; then say "$u gate still failing after 3 repair rounds"; GATE_ERR="$out"; return 1; fi
    say "$u gate failed (rc $rc), repair round $((round+1))/3"
    round=$((round+1)); pf="$LOGDIR/$u.repair$round.prompt.md"
    { prompt_head "$u" "$crate" "$CARGO_TARGET_DIR"
      printf 'TASK: repair. The machine gate failed. Fix the owned files so it passes (owned files only; do not delete or weaken tests). The gate output (tail) is:\n\n```\n%s\n```\n' "$out"; } > "$pf"
    codex_call "$wt" "$pf" "$u" "repair$round"
  done
}

run_mutants() { # unit wt crate ; sets SURV (count, -1 = skipped) and writes $LOGDIR/$u.survivors.txt
  local u="$1" wt="$2" crate="$3" d="$LOGDIR/$1.mutants" rc
  SURV=-1
  if [ "${SKIP_MUTANTS:-0}" = 1 ] || ! cargo mutants --version >/dev/null 2>&1; then MUT_NOTE="mutants skipped (not installed or SKIP_MUTANTS)"; return; fi
  ( cd "$wt" && git add -A -- . ':!.codex-*' && git diff --cached HEAD > "$LOGDIR/$u.diff" )
  [ -s "$LOGDIR/$u.diff" ] || { MUT_NOTE="mutants skipped (empty diff)"; return; }
  rm -rf "$d"
  # Own target dir: cargo-mutants builds mutated copies whose artifacts must never be reused by the real worktree
  # (metadata hashes of workspace members do not depend on the checkout path, so a shared target dir leaks mutants).
  ( cd "$wt" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR}-mut" timeout "$MUTANTS_TIMEOUT" cargo mutants -p "$crate" --in-diff "$LOGDIR/$u.diff" --timeout "${MUT_TEST_TIMEOUT:-90}" -j1 --no-shuffle --output "$d" ) > "$LOGDIR/$u.mutants.log" 2>&1 </dev/null
  rc=$?
  rm -rf "${CARGO_TARGET_DIR}-mut"
  case "$rc" in 0|2|3) MUT_NOTE="";; 124) MUT_NOTE="mutants time-boxed (${MUTANTS_TIMEOUT}s), partial result";; *) MUT_NOTE="mutants exit $rc"; SURV=-1; return;; esac
  : > "$LOGDIR/$u.survivors.txt"
  [ -f "$d/mutants.out/missed.txt" ] && cp "$d/mutants.out/missed.txt" "$LOGDIR/$u.survivors.txt"
  SURV=$(grep -c . "$LOGDIR/$u.survivors.txt")
}

# run_unit <unit> : returns 0 = merged (state done/partial), 1 = failed (state failed)
run_unit() {
  local u="$1" lcu wt br crate c pf has_t=0 f round
  lcu="$(echo "$u" | tr A-Z a-z)"; wt="$REPO/.claude/worktrees/wu-$lcu"; br="wu/$u"
  export CARGO_TARGET_DIR="$REPO/target-wave-pipe-$lcu" CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0
  : > "$LOGDIR/$u.log"
  # fresh worktree from the integration branch (only worktrees/branches this pipeline created are removed)
  [ -d "$wt" ] && git -C "$REPO" worktree remove --force "$wt" >/dev/null 2>&1
  git -C "$REPO" worktree prune
  git -C "$REPO" show-ref --verify --quiet "refs/heads/$br" && git -C "$REPO" branch -D "$br" >/dev/null 2>&1
  git -C "$REPO" worktree add -q -b "$br" "$wt" "$IBR" >>"$LOGDIR/$u.log" 2>&1 || { $LIB sset "$STATE" "$u" failed "worktree add failed"; return 1; }
  c="$wt/contracts/$u.md"; [ -f "$c" ] || { $LIB sset "$STATE" "$u" failed "no contract $c"; return 1; }
  mapfile -t OWN < <($LIB owned "$c" | tr -d '\r')
  [ "${#OWN[@]}" -gt 0 ] || { $LIB sset "$STATE" "$u" failed "contract lists no owned files"; return 1; }
  crate="$(printf '%s\n' "${OWN[@]}" | grep -m1 '^crates/' | cut -d/ -f2)"
  [ -n "$crate" ] || { $LIB sset "$STATE" "$u" failed "cannot derive crate"; return 1; }
  bash "$SELF/codex-pack.sh" "$u" "$wt" >>"$LOGDIR/$u.log" 2>&1 || { $LIB sset "$STATE" "$u" failed "pack failed"; return 1; }

  for f in "${OWN[@]}"; do case "$f" in */tests/*) [ -f "$wt/$f" ] && has_t=1;; esac; done
  if [ "$has_t" = 0 ]; then
    pf="$LOGDIR/$u.red.prompt.md"
    { prompt_head "$u" "$crate" "$CARGO_TARGET_DIR"
      printf 'TASK: red-tests. Write ONLY the owned test files under tests/ (they do not exist yet), porting the C# tests named in the contract. Do not edit any file under src/. The tests must compile against the stubs and fail only because the stubs are not implemented (todo!() or wrong values), never because of compile errors. Confirm with `cargo test -p %s --no-run`.\n' "$crate"; } > "$pf"
    codex_call "$wt" "$pf" "$u" red-tests
  fi
  pf="$LOGDIR/$u.impl.prompt.md"
  { prompt_head "$u" "$crate" "$CARGO_TARGET_DIR"
    printf 'TASK: implement. Implement the owned src files per the contract and the C# source so that every test of this unit passes. Do not edit the test files unless a test provably contradicts the C# source (say so in the summary). Run cargo fmt, then `cargo clippy -p %s --all-targets -- -D warnings` and `cargo test -p %s` until green.\n' "$crate" "$crate"; } > "$pf"
  codex_call "$wt" "$pf" "$u" implement

  GATE_ERR=""
  if ! gate_loop "$u" "$wt" "$crate"; then
    printf '%s\n' "$GATE_ERR" > "$LOGDIR/$u.gate-error.txt"
    $LIB sset "$STATE" "$u" failed "gate failed after 3 repair rounds (see $LOGDIR/$u.gate-error.txt); worktree kept: $wt"; rm -rf "$CARGO_TARGET_DIR"; return 1
  fi

  MUT_NOTE=""; run_mutants "$u" "$wt" "$crate"; round=0
  while [ "$SURV" -gt 0 ] && [ "$round" -lt 2 ]; do
    round=$((round+1)); say "$u: $SURV surviving mutants, test round $round/2"
    ( cd "$wt" && git add -A -- . ':!.codex-*' && git stash create > "$LOGDIR/$u.snap" )
    pf="$LOGDIR/$u.mut$round.prompt.md"
    { prompt_head "$u" "$crate" "$CARGO_TARGET_DIR"
      printf 'TASK: kill-mutants. cargo-mutants applied small code changes to the owned src files and NO test failed (surviving mutants). Add or strengthen tests ONLY in the owned test files under tests/ so each mutant would be caught (assert exact values, edge cases, error paths). Do not edit src files. Surviving mutants:\n\n```\n'; head -n 60 "$LOGDIR/$u.survivors.txt"; printf '```\n'; } > "$pf"
    codex_call "$wt" "$pf" "$u" "mutants$round"
    if ! bash "$SELF/codex-gate.sh" "$u" "$wt" "$crate" > "$LOGDIR/$u.mut$round.gate.txt" 2>&1 </dev/null; then
      say "$u: gate failed after mutant tests; restoring previous state"
      ( cd "$wt" && git reset -q --hard HEAD && git clean -fdq -e '.codex-*' && git stash apply -q --index "$(cat "$LOGDIR/$u.snap")" ) >>"$LOGDIR/$u.log" 2>&1
      break
    fi
    run_mutants "$u" "$wt" "$crate"
  done
  [ "$SURV" -lt 0 ] && SURV=0

  ( cd "$wt" && git add -A -- . ':!.codex-*' && git commit -q -m "feat($u): port $u via Codex pipeline" -m "Co-Authored-By: Codex gpt-6-luna <noreply@openai.com>" ) >>"$LOGDIR/$u.log" 2>&1 \
    || { $LIB sset "$STATE" "$u" failed "commit failed"; rm -rf "$CARGO_TARGET_DIR"; return 1; }
  if ! git -C "$INT" merge --no-ff -q "$br" -m "Merge $u (Codex pipeline)" >>"$LOGDIR/$u.log" 2>&1; then
    git -C "$INT" merge --abort >/dev/null 2>&1
    $LIB sset "$STATE" "$u" failed "merge into $IBR conflicted; branch $br kept"; rm -rf "$CARGO_TARGET_DIR"; return 1
  fi
  if [ "$SURV" -gt 0 ]; then $LIB sset "$STATE" "$u" partial "$SURV surviving mutants${MUT_NOTE:+; $MUT_NOTE}" "$SURV"
  else $LIB sset "$STATE" "$u" done "${MUT_NOTE:-}" 0; fi
  rm -rf "$CARGO_TARGET_DIR"
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
say "pipeline $TAG: ${#ORDER[@]} units, integration $IBR"

for l in "${ORDER[@]}"; do
  u="${l%%$'\t'*}"; deps="${l#*$'\t'}"
  st="$($LIB sget "$STATE" "$u")"
  case "$st" in done|partial) say "$u: already merged ($st), skipping"; continue;; esac
  bad=""
  for d in ${deps//,/ }; do
    ds="$($LIB sget "$STATE" "$d")"
    case "$ds" in done|partial) ;; *) bad="$d (${ds:-not run})";; esac
  done
  if [ -n "$bad" ]; then $LIB sset "$STATE" "$u" skipped "dependency $bad is not merged"; say "$u: skipped, dependency $bad"; continue; fi
  say "$u: start"
  run_unit "$u"; say "$u: $($LIB sget "$STATE" "$u")"
done

# ---------------------------------------------------------------- workspace gate, push, PR
merged=$($LIB slist "$STATE" | awk -F'\t' '$2=="done"||$2=="partial"' | grep -c .)
[ "$merged" -gt 0 ] || { say "no unit merged; nothing to push"; $LIB slist "$STATE"; exit 1; }
export CARGO_TARGET_DIR="$REPO/target-wave-pipe-int-$TAG" CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0
WG="$LOGDIR/workspace-gate-$TAG.log"; WGOK=1
say "workspace gate on $IBR"
( cd "$INT" && cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked ) > "$WG" 2>&1 </dev/null || WGOK=0
rm -rf "$CARGO_TARGET_DIR"
say "workspace gate: $([ $WGOK = 1 ] && echo OK || echo FAILED)"
if [ "${PIPELINE_NO_PR:-0}" = 1 ]; then say "PIPELINE_NO_PR=1: not pushing"; exit $((1-WGOK)); fi

BODY="$LOGDIR/pr-body-$TAG.md"
{
  echo "Wave 8 batch $BATCH ported by the Claude-free Codex pipeline (Codex gpt-6-luna, effort low). Review the diff before merging."; echo
  echo "Workspace gate (fmt, clippy --workspace -D warnings, test --workspace): $([ $WGOK = 1 ] && echo PASSED || echo '**FAILED**, auto-merge not enabled')"
  if [ $WGOK = 0 ]; then echo '```'; tail -n 40 "$WG"; echo '```'; fi
  echo; echo "## Units"
  $LIB slist "$STATE" | awk -F'\t' '{printf "- %s: **%s**%s\n",$1,$2,($4!=""?" - "$4:"")}'
  echo; echo "## Surviving mutants (tests missing)"
  any=0
  for u in $($LIB slist "$STATE" | awk -F'\t' '$2=="partial"{print $1}'); do
    any=1; echo "- $u"; echo '```'; head -n 15 "$LOGDIR/$u.survivors.txt" 2>/dev/null; echo '```'
  done
  [ $any = 0 ] && echo "none"
  echo; echo "## Needs Claude (failed or skipped units)"
  fl=$($LIB slist "$STATE" | awk -F'\t' '$2=="failed"||$2=="skipped"{printf "- %s: %s - %s\n",$1,$2,$4}')
  if [ -n "$fl" ]; then echo "$fl"; else echo "none"; fi
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
