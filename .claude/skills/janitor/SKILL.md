---
name: janitor
description: Use when the user asks to inspect or clean leftover worktrees, targets, claims, or mutation artifacts.
---

Run `bash tools/dev/janitor.sh` as a dry-run first and show its table in Vietnamese. Ask the user before running with `--apply`. Explain each kind: worktree, worktree-locked, target, orphan, loose-file, old-ref, temp, and claim.
