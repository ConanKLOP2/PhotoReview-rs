#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SCHED="$ROOT/tools/dev/codex-sched.sh"
T=$(mktemp -d)
fails=0
check() { if [ "$1" = "$2" ]; then echo "PASS: $3"; else echo "FAIL: $3 expected=$2 actual=$1"; fails=$((fails+1)); fi; }
trap 'rm -rf "$T"' EXIT
export SCHED_STATE_DIR="$T/slots" USAGE_SCRIPT="$T/usage" CODEX_RUN="$T/run"
export CALLS="$T/calls" USAGE_STATE="$T/usage-state" ACTIVE="$T/active"
export QUOTA_SLEEP_OVERRIDE=1 SLOT_SLEEP_OVERRIDE=1 CALL_TIMEOUT=10
cat > "$USAGE_SCRIPT" <<'EOF'
#!/usr/bin/env bash
if [ "${WEEK:-0}" -ge 95 ]; then
  echo '{"ok":true,"five_hour_used":10,"five_hour_resets_at":"2099-01-01 00:00","weekly_used":95,"weekly_resets_at":"2099-01-07 00:00"}'
elif [ -f "$USAGE_STATE" ] && [ "$(cat "$USAGE_STATE")" -gt 0 ]; then
  n=$(cat "$USAGE_STATE"); echo $((n-1)) > "$USAGE_STATE"
  echo '{"ok":false,"five_hour_used":99,"five_hour_resets_at":"2099-01-01 00:00","weekly_used":10,"weekly_resets_at":"2099-01-07 00:00"}'
  exit 1
else
  echo '{"ok":true,"five_hour_used":10,"five_hour_resets_at":"2099-01-01 00:00","weekly_used":10,"weekly_resets_at":"2099-01-07 00:00"}'
fi
EOF
# Intentionally not executable: the real usage script requires bash on Windows.
chmod -x "$USAGE_SCRIPT"
cat > "$CODEX_RUN" <<'EOF'
#!/usr/bin/env bash
sid="${4:-}"
printf 'call|%s|%s\n' "$sid" "$2" >> "$CALLS"
if [ -n "$sid" ]; then
  [ "$sid" = same-session ] || exit 42
  grep -q '^CONTINUE:' "$2" || exit 43
fi
echo '{"type":"thread.started","thread_id":"same-session"}'
case "${MODE:-ok}" in
 resume|fail3|stderr)
  if [ "$MODE" = fail3 ] || [ -z "$sid" ]; then
    if [ "$MODE" = stderr ]; then echo 'usage limit reached' >&2
    else echo '{"type":"error","message":"usage limit reached"}'; fi
    exit 1
  fi;;
 serial)
  if ! mkdir "$ACTIVE" 2>/dev/null; then echo overlap >> "$CALLS"; exit 44; fi
  echo start >> "$CALLS"
  sleep 1
  echo end >> "$CALLS"; rmdir "$ACTIVE";;
 hold)
  sleep 3;;
 failure) echo 'sandbox failure' >&2; exit 7;;
 missing) echo '{"type":"turn.completed","usage":{}}'; exit 0;;
esac
echo '{"type":"item.completed","item":{"type":"agent_message","text":"done"}}'
echo '{"type":"turn.completed","usage":{}}'
printf 'done\n' > "$3"
EOF
printf prompt > "$T/prompt"
run() { timeout 25 bash "$SCHED" run "$T/wt" "$T/prompt" "$T/$1" > "$T/$1.log" 2>&1; }
: > "$CALLS"
MODE=serial MAX_LANES=1 run o1 & p1=$!
MODE=serial MAX_LANES=1 run o2 & p2=$!
wait "$p1"; check "$?" 0 'lane one exits ok'
wait "$p2"; check "$?" 0 'lane two exits ok'
events=$(grep -E '^(start|end|overlap)$' "$CALLS" | tr '\n' ' ')
check "$events" 'start end start end ' 'MAX_LANES=1 serializes actual call lifetimes'
grep -q 'waiting for a lane slot' "$T"/o[12].log; check "$?" 0 'second lane really waited'
: > "$CALLS"
MODE=resume run o3; check "$?" 0 'usage-limited call resumes successfully'
check "$(grep -c '^call|' "$CALLS")" 2 'one initial call and one resume'
grep -q '^call|same-session|.*o3.resume.prompt$' "$CALLS"; check "$?" 0 'same session and resume prompt passed in arguments'
grep -q 'SCHED: ok.*session=same-session' "$T/o3.log"; check "$?" 0 'session comes from stdout thread.started'
grep -q 'thread.started' "$T/o3.jsonl"; check "$?" 0 'stdout stream captured'
check "$(cat "$T/o3")" done 'final message is separate from stream'
grep -q 'thread.started' "$T/o3.log"; check "$?" 1 'Codex stream not printed'
: > "$CALLS"
MODE=fail3 run o4; check "$?" 1 'three failed resumes return failure'
check "$(grep -c '^call|' "$CALLS")" 4 'at most three resumes after initial call'
check "$(grep -c '^call|same-session|' "$CALLS")" 3 'all retries use same session'
grep -q 'SCHED: failed' "$T/o4.log"; check "$?" 0 'exhausted resumes state failed'
echo 1 > "$USAGE_STATE"
MODE=ok run o5; check "$?" 0 'five hour quota recovers'
grep -q 'sleeping 1s' "$T/o5.log"; check "$?" 0 'five hour quota waits using override'
: > "$CALLS"
WEEK=95 run o6; check "$?" 1 'weekly quota exits one'
grep -q 'SCHED: quota week.*stop' "$T/o6.log"; check "$?" 0 'weekly stop explained'
grep -q 'SCHED: quota rc=1' "$T/o6.log"; check "$?" 0 'weekly final state quota'
grep -q sleeping "$T/o6.log"; check "$?" 1 'weekly quota never sleeps'
check "$(wc -l < "$CALLS")" 0 'weekly quota never calls Codex'
mkdir -p "$SCHED_STATE_DIR/slot-1"; echo 99999999 > "$SCHED_STATE_DIR/slot-1/pid"
MAX_LANES=1 MODE=ok run o7; check "$?" 0 'dead-pid slot reclaimed by acquire'
check "$(find "$SCHED_STATE_DIR" -name 'slot-*' | wc -l)" 0 'completed call releases reclaimed slot'
# env resets SIGINT disposition before bash, unlike a bare asynchronous bash.
MODE=hold HEARTBEAT_SECONDS=0.1 timeout --preserve-status -s INT 2 env --default-signal=INT bash "$SCHED" run "$T/wt" "$T/prompt" "$T/o8" > "$T/o8.log" 2>&1
check "$?" 130 'actual SIGINT exits 130'
check "$(find "$SCHED_STATE_DIR" -name 'slot-*' | wc -l)" 0 'SIGINT releases slot'
grep -q heartbeat "$T/o8.log"; check "$?" 0 'heartbeat emitted during active call'
MODE=missing run o9; check "$?" 1 'zero exit without final message fails'
grep -q 'SCHED: failed' "$T/o9.log"; check "$?" 0 'missing output final state failed'
echo stale > "$T/o10"
MODE=missing run o10; check "$?" 1 'stale output cannot hide missing final message'
MODE=failure run o11; check "$?" 1 'ordinary CLI error fails'
grep -q 'SCHED: failed rc=7' "$T/o11.log"; check "$?" 0 'non-quota error never reported ok'
check "$(cat "$T/o11.err")" 'sandbox failure' 'stderr captured separately'
MODE=stderr run o12; check "$?" 0 'quota errors on stderr also resume'
if [ "$fails" -eq 0 ]; then echo 'selftest PASSED'; else echo "selftest $fails FAILED"; fi
exit "$fails"
