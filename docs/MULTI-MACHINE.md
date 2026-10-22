# Several machines, one port (conventions)

Two or more machines (A, B, ...) can port units in parallel. The only shared state is **git on `origin`** (branches, PRs, claim refs). Everything under
`tools/autopilot/state/` is **local to a machine** and is never copied between machines.

## 1. Names
- Set once per machine, in the shell profile of the user that runs the pipeline: `PR_MACHINE=A` (main machine), `PR_MACHINE=B`, ...
  Default is the host name; stick to one short name per machine because it appears in claims, branch names and commit trailers.

## 2. Ownership (who works on what)
A batch is owned by ONE machine at a time. The lead (the human, or the Claude session on machine A) writes the table below and changes it
in a small PR to `master` before a machine starts a new batch. A machine never works on a unit outside its rows without a claim (section 3).

| Machine | Batch / units | Integration branch | Pipeline tag |
|---|---|---|---|
| A | Wave 8 B5 remainder (18h-18k, 18o-18v, 19d-19m, mutation debts) | `integration/wave8-b5` | `wave8-b5` |
| B | Wave 8 B6 (17a-17e, 20a; 20b last, after B5) | `integration/wave8-b6` | `wave8-b6` |

Rules: one pipeline tag per machine; the tag name is the integration branch name (`integration/<tag>`); never run a pipeline with a tag that another
machine owns. Hand-over of a unit = the owner releases the claim (section 3) and the table row is changed.

## 3. Claims (no duplicate work)
`tools/autopilot/claim.sh` keeps one git ref `refs/claims/<unit>` per claimed unit on `origin`; the first push wins (a second push of an unrelated
commit to the same ref is rejected by git), so two machines can never hold the same unit.
```
bash tools/autopilot/claim.sh claim W8-18o "porting via pipeline"   # 0 = mine, 1 = held by another machine, 2 = origin unreachable
bash tools/autopilot/claim.sh list                                  # unit, machine, time, note
bash tools/autopilot/claim.sh release W8-18o                        # give it back (also after a failed unit nobody will retry)
bash tools/autopilot/claim.sh gc                                    # drop claims whose unit is already on master or older than 14 days
```
- `codex-pipeline.sh` claims every unit before it starts it and marks it `skipped (claimed by <machine>)` when another machine holds it.
  Units claimed by hand (Claude lanes, workflows, manual fixes) must be claimed with the same command BEFORE the first edit, released when the
  PR is merged or abandoned.
- Release only your own claims. `--force` on another machine's claim only after you asked that machine (or the human) and got an answer.
- Run `claim.sh gc` from time to time; it never touches a claim whose unit has not reached `master`.

## 4. Branches and PRs
- Worktrees and `wu/<unit>` branches are local; push only the integration branch, `claude/<topic>` lane branches and PR branches.
- Every PR targets `master`, has squash auto-merge, and says in its first line which machine opened it and which units it carries
  (`Machine B: W8-17a, 17b, 17c`). Unit commit trailer stays `Co-Authored-By: ...`; the machine name goes in the PR body.
- Before starting work, a machine runs `git fetch` and checks `master` for the units it plans (a unit that already appears in `git log origin/master`
  is done: do not redo it). After a PR of ANOTHER machine merges, merge `origin/master` into your integration branch (never rebase, never force-push).
- Dependencies across machines: a unit may start only when its dependencies are on `master` (not just on someone's integration branch).
  `codex-pipeline.sh` reads dependency status from its own state file, so for a dependency owned by the other machine mark it done in YOUR
  state only after it is merged to master: `python tools/autopilot/lib/codex-lib.py sset tools/autopilot/state/<tag>.json <unit> done "on master" 0`.

## 5. Shared files (conflicts are expected, keep them small)
- `docs/PARITY.md`: edit only the rows of your own units. `task_on_progress.md`: add a NEW section per machine and per date
  (`## Wave 8 B5, machine A, 2026-10-22`), never rewrite another machine's section. `contracts/README.md`: only append to your batch section.
- `Cargo.lock` / `Cargo.toml`: ask the human before touching dependencies; if both machines need one, the machine that merges first wins and the
  other merges master.
- On a merge conflict in a shared file merge `origin/master` in, keep BOTH sides, run the gate again.

## 6. Messages between machines
There is no live channel; use git:
- the claim note (`claim.sh claim <unit> "<note>"`) says what the machine is doing and what it needs;
- the PR description lists what was NOT run and what the next machine must know;
- anything that needs a decision goes to the human (a row in `docs/PROPOSED-DECISIONS.md`, a PR comment). A machine never waits silently: write the
  question where the other machine and the human will see it, then take the recommended option (standing rule: do not stop).

## 7. Resources
- One Codex account: quota is shared. `tools/autopilot/codex-usage.sh` shows it; the limit is 95 % (set by the owner): stop a machine's Codex work above 95 % of the 5 h window or of the week
  (the pipeline enforces it with `QUOTA_MAX=95`), and run `--jobs 1` on the second machine unless the quota has room.
- Each machine has its own `CARGO_TARGET_DIR` inside the worktree; mutation targets go outside the worktree.
- A machine that cannot build (disk, `STATUS_DLL_INIT_FAILED`) reports it in the claim note and releases its claims.
