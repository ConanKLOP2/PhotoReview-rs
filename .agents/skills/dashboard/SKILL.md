---
name: dashboard
description: Use when the owner asks for a dashboard, a visual overview or a shareable page of project progress, wave units, quota or PRs.
---

Run `bash tools/dev/dashboard.sh` (optionally `--out FILE`). It is read-only and takes about 40 seconds; it writes one self-contained HTML file, by default `tools/dev/out/dashboard.html`.

Report to the owner in Vietnamese, at most 10 lines:
- the output path and size, and the line the tool printed (wave units merged / total);
- the batch progress B1..B6 and the count of units not yet merged;
- warnings shown on the page: quota STOP, low disk, failed pipeline units, dirty worktrees.

Never publish or upload the file anywhere (no Artifact tool) unless the owner asks for it explicitly.

Run every command from the repository root.
