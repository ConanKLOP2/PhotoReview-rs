---
name: codex-quota
description: Use when the user asks about Codex quota, free lanes, whether a Codex lane can start now, or why a Codex run is waiting.
---

Run `bash tools/dev/codex-sched.sh status` (slots and quota) and `bash tools/autopilot/codex-usage.sh` if the first shows no quota line. Answer in Vietnamese in at most 8 lines: 5-hour and weekly usage against the 95% ceiling (`QUOTA_MAX`), lanes in use out of `MAX_LANES=2`, dead slots, and the reset time if waiting. A weekly limit hit means stop, not wait. Offer `bash tools/dev/codex-sched.sh clean` for dead slots but run it only after the user agrees. Read-only otherwise; never start a Codex call from this skill.
