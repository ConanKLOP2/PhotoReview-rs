#!/usr/bin/env bash
# Run one sub-agent task with Codex CLI (default model gpt-6.1-sol, effort medium; override with CODEX_MODEL / CODEX_EFFORT) in a given worktree. Output is JSONL (--json): the
# pipeline reads the session id (thread.started) and token usage (turn.completed) from it.
# Usage: tools/autopilot/codex-run.sh <worktree-dir> <prompt-file> [last-message-out-file] [session-id]
# With a session id the previous session is resumed (`codex exec resume`): the pack is not sent again.
C="${CODEX_BIN:-$HOME/.codex/plugins/.plugin-appserver/codex.exe}"
DIR="$1"; PF="$2"; OUT="${3:-$DIR/.codex-last-message.txt}"; SID="${4:-}"
M="${CODEX_MODEL:-gpt-6.1-sol}"; E="${CODEX_EFFORT:-medium}"
if [ -n "$SID" ]; then
  cd "$DIR" || exit 1
  exec "$C" exec resume --json --skip-git-repo-check -m "$M" -c "model_reasoning_effort=\"$E\"" -c 'sandbox_mode="workspace-write"' -o "$OUT" "$SID" - < "$PF"
fi
exec "$C" exec --json --skip-git-repo-check -m "$M" -c "model_reasoning_effort=\"$E\"" -s workspace-write -C "$DIR" -o "$OUT" - < "$PF"
