---
name: pre-pr
description: Use before opening or updating a pull request, or when asked whether a branch is ready for review.
---

Run `bash tools/dev/pre-pr.sh` (add `--test` to also run the tests of the changed crates, `--no-clippy` for a quick pass), then answer in Vietnamese in at most 10 lines: which checks failed and how to fix them, and the "NOT RUN" list to put in the PR body. Never edit or commit anything.

Run every command from the repository root.
