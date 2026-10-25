#!/usr/bin/env bash
set -u
SELF_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$SELF_DIR/../.." && pwd)"
STATE="${SCHED_STATE_DIR:-$REPO/tools/autopilot/state/sched}"
MAX_LANES="${MAX_LANES:-2}"; QUOTA_MAX="${QUOTA_MAX:-95}"; RESERVE="${RESERVE:-10}"
CALL_TIMEOUT="${CALL_TIMEOUT:-2400}"; USAGE_SCRIPT="${USAGE_SCRIPT:-$REPO/tools/autopilot/codex-usage.sh}"
CODEX_RUN="${CODEX_RUN:-$REPO/tools/autopilot/codex-run.sh}"; PY="${PY_BIN:-python}"
mkdir -p "$STATE"
alive() { kill -0 "$1" 2>/dev/null; }
slot_id() { basename "$1" | sed 's/^slot-//'; }
release_slot() { [ -n "${SLOT:-}" ] && rm -rf "$SLOT"; SLOT=""; }
CALL_PID=""; HEARTBEAT_PID=""
stop_call() {
  if [ -n "$HEARTBEAT_PID" ]; then kill "$HEARTBEAT_PID" 2>/dev/null || :; wait "$HEARTBEAT_PID" 2>/dev/null || :; HEARTBEAT_PID=""; fi
  if [ -n "$CALL_PID" ]; then kill "$CALL_PID" 2>/dev/null || :; wait "$CALL_PID" 2>/dev/null || :; CALL_PID=""; fi
}
cleanup() { trap '' INT TERM HUP; release_slot; stop_call; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP
usage_data() { bash "$USAGE_SCRIPT" "$QUOTA_MAX" 2>/dev/null | "$PY" -I -c 'import json,sys; d=json.load(sys.stdin); print("%s|%s|%s|%s"%(str(d.get("ok",False)).lower(),d.get("five_hour_used",""),d.get("five_hour_resets_at",""),d.get("weekly_used","")))' 2>/dev/null; }
quota_wait() {
  while :; do
    data=$(usage_data); ok=${data%%|*}; rest=${data#*|}; used=${rest%%|*}; rest=${rest#*|}; reset=${rest%%|*}; week=${rest#*|}
    if [ -n "$week" ] && awk -v w="$week" -v q="$QUOTA_MAX" 'BEGIN{exit !(w>=q)}'; then
      echo "SCHED: quota week used=$week max=$QUOTA_MAX stop"
      return 1
    fi
    allowed=0
    if [ "$ok" = true ] && [ -n "$used" ] && awk -v u="$used" -v r="$RESERVE" -v q="$QUOTA_MAX" 'BEGIN{exit !((u+r)<q)}'; then allowed=1; fi
    [ "$allowed" -eq 1 ] && return 0
    seconds="${QUOTA_SLEEP_OVERRIDE:-300}"
    if [ -z "${QUOTA_SLEEP_OVERRIDE:-}" ] && [ -n "$reset" ]; then epoch=$(date -d "$reset" +%s 2>/dev/null || true); now=$(date +%s); [ -n "$epoch" ] && seconds=$((epoch-now+120)); fi
    [ "$seconds" -lt 1 ] && seconds=1; [ "$seconds" -gt 1800 ] && seconds=1800
    echo "SCHED: quota unavailable (used=${used:-?} reset=${reset:-?}); sleeping ${seconds}s"; sleep "$seconds"
  done
}
acquire() {
  waited=0
  while :; do
    n=1; while [ "$n" -le "$MAX_LANES" ]; do
      d="$STATE/slot-$n"
      if mkdir "$d" 2>/dev/null; then SLOT="$d"; printf '%s\n' "$$" > "$d/pid"; printf '%s\n' "$1" > "$d/worktree"; date +%s > "$d/started"; return 0; fi
      if [ -f "$d/pid" ]; then p=$(cat "$d/pid"); if ! alive "$p"; then rm -rf "$d"; fi; fi
      n=$((n+1))
    done
    [ "$waited" -eq 0 ] && echo 'SCHED: waiting for a lane slot'
    if [ $((waited % 300)) -eq 0 ] && [ "$waited" -gt 0 ]; then echo "SCHED: still waiting (${waited}s)"; fi
    sleep "${SLOT_SLEEP_OVERRIDE:-30}"; waited=$((waited+30))
  done
}
thread_id() { "$PY" -I -c 'import json,sys
for line in open(sys.argv[1],encoding="utf-8",errors="replace"):
 try: d=json.loads(line)
 except Exception: continue
 if d.get("type")=="thread.started":
  x=d.get("thread_id") or d.get("thread",{}).get("id")
  if x: print(x); break' "$1" 2>/dev/null; }
contains_quota() { grep -Eiq 'rate.?limit|usage.?limit|quota' "$1" 2>/dev/null; }
run_task() {
  wt="$1" pf="$2" out="$3" sid="${4:-}"; rc=0; state=ok; resumes=0
  acquire "$wt"
  while :; do
    if ! quota_wait; then state=quota; rc=1; break; fi
    # Each invocation has its own stream and errors. Never accept a stale final
    # message from a previous attempt as evidence of successful completion.
    rm -f -- "$out"
    timeout "$CALL_TIMEOUT" bash "$CODEX_RUN" "$wt" "$pf" "$out" "$sid" > "$out.jsonl" 2> "$out.err" &
    CALL_PID=$!
    (
      sleeper=""
      trap '[ -z "$sleeper" ] || kill "$sleeper" 2>/dev/null; exit 0' TERM INT
      while :; do
        sleep "${HEARTBEAT_SECONDS:-600}" & sleeper=$!
        wait "$sleeper" || exit 0
        echo "SCHED: heartbeat session=${sid:-pending} out=$out"
      done
    ) &
    HEARTBEAT_PID=$!
    wait "$CALL_PID"; rc=$?; CALL_PID=""
    stop_call
    [ -n "$sid" ] || sid=$(thread_id "$out.jsonl")
    if [ "$rc" -eq 0 ]; then if [ -f "$out" ]; then state=ok; else state=failed; rc=1; fi; break; fi
    if { contains_quota "$out.jsonl" || contains_quota "$out.err"; } && [ "$resumes" -lt 3 ] && [ -n "$sid" ]; then
      resumes=$((resumes+1)); state=quota
      pf="$out.resume.prompt"; printf '%s\n' 'CONTINUE: the previous call was cut by the usage limit; check git status/diff and finish the task' > "$pf"; continue
    fi
    state=failed
    break
  done
  release_slot; echo "SCHED: $state rc=$rc session=${sid:-none} out=$out"
  [ "$state" = ok ] && return 0; return 1
}
case "${1:-}" in
  run) [ "$#" -ge 4 ] || { echo 'usage: codex-sched.sh run <worktree> <prompt-file> <out-file> [session-id]' >&2; exit 2; }; run_task "$2" "$3" "$4" "${5:-}";;
  status) for d in "$STATE"/slot-*; do [ -d "$d" ] || continue; p=$(cat "$d/pid" 2>/dev/null || echo '?'); start=$(cat "$d/started" 2>/dev/null || echo 0); age=$(($(date +%s)-start)); w=$(cat "$d/worktree" 2>/dev/null || echo '?'); alive "$p" && a=yes || a=no; echo "slot $(slot_id "$d"): pid=$p alive=$a age=${age}s worktree=$w"; done; usage_data | "$PY" -I -c 'import sys; s=sys.stdin.read().strip(); print("quota: "+(s or "unavailable"))';;
  clean) for d in "$STATE"/slot-*; do [ -d "$d" ] || continue; p=$(cat "$d/pid" 2>/dev/null || echo 0); if ! alive "$p"; then rm -rf "$d"; echo "removed $(slot_id "$d")"; fi; done;;
  *) echo 'usage: codex-sched.sh {run|status|clean}' >&2; exit 2;;
esac
