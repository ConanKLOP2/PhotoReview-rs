# Developer status and cleanup

`status.sh` shows quota, disk, processes, pipeline state, claims, PRs, and worktrees.

```bash
bash tools/dev/status.sh
bash tools/dev/status.sh --json
```

`janitor.sh` inventories safe cleanup candidates; it is a dry-run by default.

```bash
bash tools/dev/janitor.sh
bash tools/dev/janitor.sh --apply
bash tools/dev/janitor.sh --apply --claims
```

`--claims` releases only this machine's claims for units already on master.
Override dependencies with `GIT_BIN`, `GH_BIN`, `PY_BIN`, `PS_BIN`, `DU_BIN`, `CMD_BIN`, and `USAGE_BIN` (`DU_TIMEOUT` is adjustable for tests).
Missing external facts are reported as `n/a`.

Janitor never runs `git clean`; it protects dirty or locked worktrees, open PRs,
unmerged branches, integration work during an active pipeline, and `codex-*`
mutation evidence. Review the printed table before applying. Rebuildable target
folders may be removed; branches are deleted only after verified merge.
