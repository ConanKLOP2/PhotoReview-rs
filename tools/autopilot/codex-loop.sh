#!/usr/bin/env bash
# Keep Codex working on one worktree until the gate passes, checking every INTERVAL seconds.
# Each round: run the gate (clippy + the given test targets). If it fails, resume the same Codex session with the gate output
# and ask it to fix the rest. Stops when the gate is green, or after MAX_ROUNDS rounds (so it cannot run forever).
# Usage: tools/autopilot/codex-loop.sh <worktree-dir> <task-prompt-file> <test-target>[,<test-target>...]
# Env: MAX_ROUNDS (default 8), INTERVAL (seconds between rounds, default 900), ROUND_TIMEOUT (default 2400)
# Log: <worktree-dir>/.codex-loop.log
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
DIR="$1"; TASK="$2"; TARGETS="$3"
MAX_ROUNDS="${MAX_ROUNDS:-8}"; INTERVAL="${INTERVAL:-900}"; ROUND_TIMEOUT="${ROUND_TIMEOUT:-2400}"
LOG="$DIR/.codex-loop.log"
LAST="$DIR/.codex-last-message.txt"
GATE_OUT="$DIR/.codex-gate-out.txt"
SID=""

log() { echo "[$(date '+%H:%M:%S')] $*" | tee -a "$LOG"; }

run_gate() {
  : > "$GATE_OUT"
  ( cd "$DIR" || exit 1
    cargo clippy -p pr-app --all-targets --locked -- -D warnings || exit 1
    for t in ${TARGETS//,/ }; do
      cargo test -p pr-app --locked --test "$t" || exit 1
    done ) > "$GATE_OUT" 2>&1
}

for round in $(seq 1 "$MAX_ROUNDS"); do
  if [ "$round" -eq 1 ]; then
    PROMPT="$TASK"
    log "round $round: first run (new session)"
  else
    PROMPT="$DIR/.codex-loop-prompt.md"
    {
      echo "The gate is still failing. Output tail (clippy + tests):"
      echo '```'
      tail -n 120 "$GATE_OUT"
      echo '```'
      echo "Fix the remaining failures. Do not change test assertions. Do not commit or push."
      echo "Read files with line ranges only; never print a whole diff."
      echo "Finish with a short message: what you changed, and the last lines of each command."
    } > "$PROMPT"
    log "round $round: resume session $SID with gate output"
  fi

  if [ -z "$SID" ]; then
    timeout "$ROUND_TIMEOUT" bash "$HERE/codex-run.sh" "$DIR" "$PROMPT" "$LAST" > "$DIR/.codex-loop-round$round.jsonl" 2>&1
  else
    timeout "$ROUND_TIMEOUT" bash "$HERE/codex-run.sh" "$DIR" "$PROMPT" "$LAST" "$SID" > "$DIR/.codex-loop-round$round.jsonl" 2>&1
  fi
  rc=$?
  NEW_SID=$(grep -o '"thread_id":"[^"]*"' "$DIR/.codex-loop-round$round.jsonl" | head -1 | cut -d'"' -f4)
  [ -n "$NEW_SID" ] && SID="$NEW_SID"
  log "round $round: codex exit $rc, session ${SID:-none}"

  if run_gate; then
    log "GATE GREEN after round $round. Done."
    exit 0
  fi
  log "round $round: gate failed; last lines:"
  tail -n 6 "$GATE_OUT" | tee -a "$LOG"

  if [ "$round" -lt "$MAX_ROUNDS" ]; then
    log "sleeping ${INTERVAL}s before round $((round + 1))"
    sleep "$INTERVAL"
  fi
done

log "STOPPED: gate still failing after $MAX_ROUNDS rounds. See $GATE_OUT and $LOG."
exit 1
