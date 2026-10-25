# Codex scheduler

Run `bash tools/dev/codex-sched.sh run <worktree> <prompt-file> <out-file> [session-id]` for a foreground call. `status` lists slots/quota; `clean` reclaims dead slots.

Environment: `MAX_LANES` (default 2), `QUOTA_MAX` (95), `RESERVE` (10), `CALL_TIMEOUT` (2400 seconds), `USAGE_SCRIPT`, `CODEX_RUN`. Tests also use `PY_BIN`, `QUOTA_SLEEP_OVERRIDE`, `SLOT_SLEEP_OVERRIDE`, `HEARTBEAT_SECONDS`, and an isolated `SCHED_STATE_DIR`.

The owner's limit is 95 % of both the five hour window and the week. A weekly limit stops immediately with state quota and exit 1; only the five hour limit waits. The scheduler gates every call, reserves quota, waits for resets, and caps concurrent lanes.

Use it instead of calling `codex-run.sh` directly: workflow driver agents that return early kill the Codex process. This wrapper holds the lane until completion and resumes usage limited sessions.

Codex stdout is captured in <out-file>.jsonl and stderr in <out-file>.err. The final agent message must be written by Codex to <out-file>; a zero exit without that file fails. Usage-limit retries resume the stream's session at most three times with a continuation prompt. A heartbeat is printed every ten minutes during each call.
