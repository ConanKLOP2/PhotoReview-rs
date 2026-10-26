---
name: post-merge
description: Use after a PR was squash-merged, to verify content is in master and clean up the branch, worktree and target dirs.
---

Per AGENTS.md "Git" and "Build artifact hygiene". Input: the PR number or branch name.

1. `gh pr view <n> --json state,mergedAt,headRefName` must say MERGED. If not, stop.
2. A squash-merged head is not an ancestor of master: run `git diff <head> origin/master --stat` and confirm it shows only unrelated files. If your own files differ, stop and report.
3. Show what would be removed (remote branch, local branch, `.claude/worktrees/<name>`, `target-*` dirs it used) and make sure no running `cargo`/`rustc`/`cargo-mutants` process uses them. Never kill processes you did not start.
4. Remove them: `git worktree remove`, `git branch -D`, `git push origin --delete`, `rm -rf target-<name>` (all allowed in settings).
5. Update `task_on_progress.md` (date, PR merged, next steps) and mention it; do not claim anything unverified.

For a broader inventory use `/janitor` instead. Report in Vietnamese.
