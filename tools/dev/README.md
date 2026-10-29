# Developer tools (`tools/dev`)

Every tool has a docs file and, where marked, a Claude Code skill in `.claude/skills/<name>/SKILL.md`. Selftests: `tools/dev/test/<name>-selftest.sh`.

| Tool | Docs | Skill |
|---|---|---|
| `status.sh` quota, disk, processes, pipeline, claims, PRs, worktrees | [docs/status-janitor.md](docs/status-janitor.md) | `/status` |
| `janitor.sh` dry-run cleanup inventory (`--apply`, `--claims`) | [docs/status-janitor.md](docs/status-janitor.md) | `/janitor` |
| `codex-sched.sh` Codex lane scheduler (`MAX_LANES=2`, `QUOTA_MAX=95`) | [SCHED.md](SCHED.md) | none |
| `dashboard.sh` one self-contained HTML page of project state | [docs/dashboard.md](docs/dashboard.md) | `/dashboard` |
| `parity.sh` (+ `parity.py`) PARITY.md statistics and row status | [docs/parity.md](docs/parity.md) | `/parity` |
| `oracle.sh` golden reproducibility check around `tools/oracle.ps1` | [docs/oracle.md](docs/oracle.md) | `/oracle` |
| `triage.sh` classify a failed autopilot unit | [docs/triage.md](docs/triage.md) | `/triage` |
| `mutants.sh` time-boxed cargo-mutants run | [docs/mutants.md](docs/mutants.md) | `/mutants` |
| `native-tests.sh` list and run native (`#[ignore]`) tests | [docs/native-tests.md](docs/native-tests.md) | `/native-tests` |
| [`stage-native.ps1`](stage-native.ps1) verify C# baseline `7eaee2a2` x64 DLL hashes, then stage to `-Destination` (default: this worktree's `target\debug`); `-Check` verifies sources without copying | Run `powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1`; for test executables pass `-Destination target\debug\deps`. Selftest: [test/stage-native-selftest.ps1](test/stage-native-selftest.ps1) | none |
| [`make-synthetic-raw.py`](make-synthetic-raw.py) writes SYNTHETIC minimal DNG files (RGGB CFA, JPEG preview, orientation, preview smaller than sensor, RAW+JPEG pair) into a directory so the RAW path can be exercised without a camera; NOT a camera corpus | `python -I tools/dev/make-synthetic-raw.py [OUTDIR] [--size WxH]`. Selftest: `python -I tools/dev/make-synthetic-raw.py selftest`; Rust evidence: `crates/pr-libraw/tests/synthetic_raw.rs` | none |
| `unsafe-audit.sh` (+ `.py`, `.baseline`) `unsafe` hygiene scan | [docs/unsafe-audit.md](docs/unsafe-audit.md) | `/unsafe-audit` |
| `doc-links.py` markdown link and path checker | (header of the script) | `/doc-links` |
| Release helpers: `fetch-release-native.ps1`, `verify-release-native.ps1`, `stage-release.ps1`, `pack-release.ps1`, `check-release-workflow.ps1` under `tools/` | [release runbook](../../docs/RELEASING.md); selftests [release-selftest.ps1](test/release-selftest.ps1), [release-workflow-selftest.ps1](test/release-workflow-selftest.ps1) | none |
| Checklist skills without a script: after a merge, Codex quota | `.claude/skills/*/SKILL.md` | `/post-merge`, `/codex-quota` |
| `agents-check.sh` validate `.claude/agents/*.md` and the `.agents/skills` copies | [docs/agents-check.md](docs/agents-check.md) | none |
| `pre-pr.sh` checks before a PR (markers, BOM, fmt/clippy per changed crate, audits, NOT-RUN list) | [docs/pre-pr.md](docs/pre-pr.md) | `/pre-pr` |
| `hooks/` rustfmt and Bash-guard hooks (NOT registered; the owner registers them) | [hooks/README.md](hooks/README.md) | none |

Decision note without a tool: [docs/union-merge.md](docs/union-merge.md) (no file uses `merge=union`).

Run a tool as `bash tools/dev/<tool>.sh`; usage and options are in its docs file.
