---
name: triage
description: Use when an autopilot unit failed or has a needs-claude note and the owner wants to know why and what to do next.
---

Run `bash tools/dev/triage.sh <unit>` (or `--all` to list every needs-claude note).
If the owner wants a prompt for an AI coding agent, add `--prompt <file>` and show the file path.
Never start an agent or run cargo yourself from this skill; the tool only reads state.

Report to the owner in Vietnamese:
- the failure class and the key evidence lines;
- the suggested next action;
- for test-failure, whether the failing tests include the known flaky ones (rerun them alone first);
- if there is no note or log, say that nothing exists to triage.

Run every command from the repository root.
