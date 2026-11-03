# Several machines, one port (conventions)

Two or more machines (A, B, ...) can port units in parallel. The only shared state is **git on `origin`** (branches, PRs, claim refs). Everything under
`tools/autopilot/state/` is **local to a machine** and is never copied between machines.

## 1. Names
- Set once per machine, in the shell profile of the user that runs the pipeline: `PR_MACHINE=A` (main machine), `PR_MACHINE=B`, ...
  Fallback order (`claim.sh`): `PR_MACHINE`, then the local file `tools/autopilot/state/machine`, then the host name; stick to one short name per machine because it appears in claims, branch names and commit trailers.

## 2. Ownership (who works on what)
A batch is owned by ONE machine at a time. The lead (the human, or the Claude session on machine A) writes the table below and changes it
in a small PR to `master` before a machine starts a new batch. A machine never works on a unit outside its rows without a claim (section 3).

| Machine | Batch / units | Integration branch | Pipeline tag |
|---|---|---|---|
| A | Wave 8 B5 remainder (18h-18k, 18o-18v, 19d-19m, mutation debts) | `integration/wave8-b5` | `wave8-b5` |
| B | (stopped 2026-10-29: machine B is down; its old B6 claims were superseded and all its units moved to A) | - | - |
| A | Wave 9 external editors W9-E1..E9 after B5; follow contract dependency order (E1 rules before dialogs and Settings), claim each unit before editing | separate master-based `codex/w9-*` worktrees | manual (no shared pipeline tag) |
| A | Wave 9 rename W9-R1..R11; follow contracts dependency order, claim every unit before editing, and start only from merged master dependencies | separate master-based `codex/w9-rename-*` worktrees | manual (no shared pipeline tag) |
| A | As of 2026-10-29 machine A holds EVERYTHING: Wave 8 B6 (W8-17e, W8-20a/20b merged #290-#296, #306; native fixes `W8-17e-native-fixes`), Wave 9 rename R1-R11 (R7-R11 merged #289, #292, #301, #302, #304, #305), W10-N1 (#303). Machine B can return only through a new row here plus fresh claims | `claude/*`, `codex/*` PR branches | manual |

Rules: one pipeline tag per machine; the tag name is the integration branch name (`integration/<tag>`); never run a pipeline with a tag that another
machine owns. Hand-over of a unit = the owner releases the claim (section 3) and the table row is changed.

## 3. Claims (no duplicate work)
`tools/autopilot/claim.sh` keeps one git ref `refs/claims/<unit>` per claimed unit on `origin`; the first push wins (a second push of an unrelated
commit to the same ref is rejected by git), so two machines can never hold the same unit.
```
bash tools/autopilot/claim.sh claim W8-18o "porting via pipeline"   # 0 = mine, 1 = held by another machine, 2 = origin unreachable
bash tools/autopilot/claim.sh list                                  # unit, machine, time, note
bash tools/autopilot/claim.sh release W8-18o                        # give it back (also after a failed unit nobody will retry)
bash tools/autopilot/claim.sh gc                                    # drop claims whose unit is finished on master (feat(<id>): / Merge <id> / Complete <id> subject)
bash tools/autopilot/claim.sh gc --stale                            # the same, and also claims older than 14 days (a crashed machine)
```
- `codex-pipeline.sh` claims every unit before it starts it and marks it `skipped (claimed by <machine>)` when another machine holds it.
  Units claimed by hand (Claude lanes, workflows, manual fixes) must be claimed with the same command BEFORE the first edit, released when the
  PR is merged or abandoned.
- Release only your own claims. `--force` on another machine's claim only after you asked that machine (or the human) and got an answer.
- **A failed unit keeps its claim (P-88 option A, owner decision 2026-11-03).** The pipeline never calls `claim.sh release`: when a unit ends `failed`
  the machine keeps the claim on purpose, so another machine does not port the same unit while the `needs-claude` note is unread or being triaged (the
  other machine gets exit 1 and marks the unit `skipped (claimed by <machine>)`). After the triage, a person (or the Claude session) must give the unit
  back by hand if nobody will retry it at once: `bash tools/autopilot/claim.sh release <unit>` (from the machine that holds it; `--force` only after
  asking that machine). A unit that nobody releases stays locked until `claim.sh gc` finds it finished on master or `claim.sh gc --stale` drops it
  after 14 days. `tools/dev/janitor.sh` releases only claims of this machine whose unit is already on origin/master.
- **`claim.sh gc` (P-89 option A, owner decision 2026-11-03).** `gc` is run only by a person (no script calls it) and frees a claim only when
  a commit SUBJECT on `origin/master` is `feat(<id>): ...` (the id may be one of several in the scope, `feat(A,<id>):`), `Merge <id> ...` or
  `Complete <id> ...`; the same scope rule `codex-lib.py extsat` uses (FIX-G16B M3). An id that is only mentioned in a subject or a commit body, a `test(...)`
  subject, or a subject that says `partial`, `wip` or `red tests` never frees a claim. Age alone frees nothing: a unit that runs longer than 14 days keeps
  its claim. `gc --stale` additionally drops claims older than 14 days; use it for a crashed or abandoned machine, after checking `claim.sh list`.
  Never run `gc` against the real origin to test it: `tools/autopilot/test/claim-selftest.sh` covers it with a throw-away bare origin.

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
- `task_on_progress.md` (this supersedes the "new section per machine and date" wording above): one line per unit (status, PR number); details belong in the PR body and `docs/PARITY.md`; new sections only for decisions or incidents; old sections go verbatim to `docs/archive/`.
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
- Send heavy CPU lanes (`cargo-mutants`, mutation killers, large builds) to the machine with the strongest CPU and enough disk. A small-disk machine runs one worktree at a time, uses one shared mutation target directory outside worktrees, and deletes `target` after each unit. Both machines follow the Codex-first rule in `AGENTS.md`.
- One Codex account: quota is shared. `tools/autopilot/codex-usage.sh` shows it; the limit is 95 % (set by the owner): stop a machine's Codex work above 95 % of the 5 h window or of the week
  (`codex-pipeline.sh` enforces `QUOTA_MAX=95` on the 5 h window only, because `codex-usage.sh` compares only that window; `tools/dev/codex-sched.sh` enforces both windows and stops at once on the weekly limit), and run `--jobs 1` on the second machine unless the quota has room. The number of Codex lanes per machine is sized from that machine's free RAM, CPUs, disk and the quota (AGENTS.md "Sizing parallel agents"); `MAX_LANES` (default 2) and the pipeline `--jobs` (default 2) are only defaults.
- Each machine has its own `CARGO_TARGET_DIR` inside the worktree; mutation targets go outside the worktree.
- A machine that cannot build (disk, `STATUS_DLL_INIT_FAILED`) reports it in the claim note and releases its claims.
