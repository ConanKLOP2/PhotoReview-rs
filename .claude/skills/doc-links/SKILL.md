---
name: doc-links
description: Use when the user asks to check docs for dead links, dead paths or broken anchors, or after editing markdown under docs/ or the root *.md files.
---

Run `python -I tools/dev/doc-links.py --quiet` from the repo root (add `--all` for every markdown file). Report in Vietnamese: number of errors, each as file:line and the missing target, and PROPOSED items separately (they are not errors). Fix dead paths in the docs yourself only when the user asks; this skill is read-only. If `docs/INDEX.md` is out of date after adding or moving a doc, say so.
