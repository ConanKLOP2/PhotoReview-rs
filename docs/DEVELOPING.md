# Developing

## Prerequisites

- Rust `1.99.0` with `rustfmt`, `clippy`, and target `x86_64-pc-windows-msvc`, as pinned in `rust-toolchain.toml`.
- Visual Studio Build Tools with MSVC and the Windows SDK for Windows builds and native tests.
- Git, GitHub CLI (`gh`), and Bash for the documented scripts and PR workflow.
- Optional: `cargo-mutants` for scoped mutation checks; `dotnet` is used by `tools/oracle.ps1` to build the C# oracle.

## Build and quality commands

CI runs these workspace checks on Rust-relevant changes:

```powershell
cargo fmt -p <pkg> --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --release --locked
cargo test -p pr-core --locked
cargo test --workspace --exclude pr-app --exclude pr-core --exclude pr-platform-win --locked
cargo test -p pr-platform-win --locked
cargo test -p pr-app --lib --bins --locked
cargo test -p pr-app --doc --locked
```

Set `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_INCREMENTAL=0`, and a private `CARGO_TARGET_DIR` for each worktree before local builds. A per-crate local check is `cargo test -p pr-core` (the task rules also recommend `cargo clippy -p pr-core --all-targets -- -D warnings`).

CI sets `RUSTFLAGS=-D warnings` and `CARGO_INCREMENTAL=0`; jobs and test steps have timeouts as hang guards. CI enumerates workspace package names and runs `cargo fmt -p <pkg> --check` once per package. A single `cargo fmt --all --check` can exceed the Windows command-line limit and fail with OS error 206 when `pr-platform-win` and `pr-app` contain enough test files; this reason is documented directly in `.github/workflows/ci.yml`.

## Native and ignored tests

Real-OS tests are marked `#[ignore]` with a `native` or `slow` reason. Use `pr_test_support::TempRoot` and fakes for filesystem tests; never run code that sweeps, empties or deletes from the user's real Recycle Bin. To include ignored tests for a package, use:

```powershell
cargo test -p <pkg> -- --include-ignored
```

## C# oracle and synced assets

The **Golden Generator** role in `docs/MULTI-AGENT-PLAN.md` runs the oracle; porters never edit `tests/golden/`. The C# baseline is `d92d6aae` in `docs/SYNC-LEDGER.md`. `tools/oracle.ps1` uses `%TEMP%\pr-oracle\<sha>` as its work directory. It archives the C# repository at the selected commit into `src`, builds the C# assemblies into `core`, builds `tools/oracle-cs` into `oracle`, then invokes `oracle-cs.exe` for each selected command.

For command `<name>`, the script passes `tests/golden-inputs/<name>.json` as input and `tests/golden/<name>.json` as output, along with the C# commit. `tools/oracle-cs/Program.cs` parses those four command arguments and dispatches the command to its implementation in `tools/oracle-cs/Commands/`. The JSON golden records `generator`, `csharp_commit` and `cases`.

To add a command, add its input file under `tests/golden-inputs/`, add a command implementation under `tools/oracle-cs/Commands/`, and add its dispatch arm in `tools/oracle-cs/Program.cs`; generate the golden with `tools/oracle.ps1 -Command <name>`. Add the Rust test that consumes and checks those cases. In `crates/pr-test-support/src/golden.rs`, `golden::repo_root()` locates the workspace and `golden::load(name)` deserializes the golden into `Golden`, including `cases`. The caller performs the comparison: `lru_trace_golden_matches_csharp` in `crates/pr-core/tests/bounded_lru.rs` reads each case using `i64_of`/`bool_of`, runs the same operations in Rust, and checks results with `assert_eq!`.

`tools/sync-from-csharp.ps1 -Check` verifies synced assets. Resync with `tools/sync-from-csharp.ps1` and record the baseline in `docs/SYNC-LEDGER.md`. Shared fixtures are under `tests/fixtures/`.

`tools/wave.ps1` starts/cleans wave worktrees or runs the per-wave gate (`-Gate`, optionally `-Crate`).

## CI jobs and docs-only pull requests

`.github/workflows/ci.yml` defines:

| Job | Work |
|---|---|
| `changes` | Detects whether Rust checks are needed. |
| `repo-checks` | Runs `tools/sync-from-csharp.ps1 -Check` on Windows. |
| `lint` | Per-package format, workspace Clippy and locked release build. |
| `test (core)` | `pr-core` tests. |
| `test (crates-rest)` | Workspace tests excluding `pr-app`, `pr-core` and `pr-platform-win`. |
| `test (win)` | `pr-platform-win`, `pr-app` unit/doc/binary tests, plus app shard 2. |
| `test (app)` | `pr-app` integration test shards 0, 1 and 3. |
| `build-test` | Required aggregate gate; checks the result of `changes` and applicable Rust jobs. |

The `pr-app` integration files are sorted by name and assigned round-robin to four shards. On a pull request changing only `docs/`, `tools/`, or Markdown files, `changes` sets `rust=false`, so `lint` and `test` are skipped. `repo-checks` still runs, and required `build-test` still runs and succeeds only if `changes` succeeded and both Rust jobs were skipped. Pushes to `master`, manual runs, workflow changes, Cargo files, crate files, fixtures or toolchain changes set `rust=true` and run Rust checks.

## Mutation checks

Use this scoped command pattern (replace the placeholders for the owned crate/file/function/test):

```powershell
cargo mutants -p <crate> --baseline skip --timeout 90 -j 1 --file '**/<path>.rs' -F '<regex>' -- --test <file>
```

Keep mutation runs scoped to risky pure logic and use `-j 1` with a dedicated `CARGO_TARGET_DIR` outside the worktree. Time-box the run (about 25 minutes per unit under `AGENTS.md`) and report `tested / total, caught, missed, unviable, timeouts`, including mutants not run. Never stop another process or clean/delete a target directory another process may use. Apply the mutation testing rules in [AGENTS.md](../AGENTS.md) and [CODEX-RULES.md](../tools/autopilot/CODEX-RULES.md).

## Autopilot entry points

The [autopilot README](../tools/autopilot/README.md) documents these tools:

| Script | Purpose |
|---|---|
| `codex-pipeline.sh` | Drives the Codex unit pipeline and integration gate. |
| `codex-run.sh` | Runs a Codex call for a pipeline step. |
| `codex-gate.sh` | Runs deterministic source and policy gates. |
| `codex-pack.sh` | Builds a unit's context pack. |
| `codex-usage.sh` | Reports Codex usage/quota. |
| `claim.sh` | Claims, lists, releases and collects unit claims. |
| `unit-pr.sh` | Creates a small PR from a completed integration unit. |
| `status.sh` | Displays pipeline state. |

## Multi-machine work and PRs

Follow [MULTI-MACHINE.md](MULTI-MACHINE.md) for machine identity, ownership, claims and hand-off. See [AGENTS.md](../AGENTS.md) for worktree, branch, PR, auto-merge and cleanup rules.
