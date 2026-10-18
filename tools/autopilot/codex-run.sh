#!/usr/bin/env bash
# Run one sub-agent task with Codex CLI (GPT-6 Luna, effort low) in a given worktree.
# Usage: tools/autopilot/codex-run.sh <worktree-dir> <prompt-file> [last-message-out-file]
# Prompt is read from the file. Exit code is codex's; a non-zero exit with a rate-limit message means: fall back to Claude.
C="${CODEX_BIN:-$HOME/.codex/.sandbox-bin/codex.exe}"
DIR="$1"; PF="$2"; OUT="${3:-$DIR/.codex-last-message.txt}"
exec "$C" exec -m gpt-6-luna -c 'model_reasoning_effort="low"' -s workspace-write -C "$DIR" -o "$OUT" - < "$PF"
