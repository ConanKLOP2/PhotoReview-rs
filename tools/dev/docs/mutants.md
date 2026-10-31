# mutants.sh

Purpose: time-boxed cargo-mutants run done the way Wave 8 taught us, then a caught/missed/unviable/timeout table.

```bash
bash tools/dev/mutants.sh pr-app crates/pr-app/src/x.rs [more.rs] --tests a,b --budget 1200 --jobs 1 [--rerun-survivors]
```

Fixed flags: `--baseline skip --timeout 90 -j <jobs>`; only the given test binaries (`-- --test a --test b`).
`CARGO_TARGET_DIR=$TEMP/mut-target-<crate>` (outside the worktree: cargo-mutants copies the source tree).
Output under `$TEMP/mut-out-<crate>-XXXXXX` (override `MUT_OUT`): parses `mutants.out/outcomes.json` (missing file is
reported), writes `survivors.txt`; `--rerun-survivors` reruns only those with `-F` regex filters into `<out>/rerun`.
A budget stop (`timeout`) prints "BUDGET EXHAUSTED": the result is partial, never a full verdict.

Env overrides: `CARGO_BIN`, `PY_BIN`, `TIMEOUT_BIN`, `TEMP`, `MUT_OUT`.
Safety: never kills processes by name, never deletes anything, never touches other target dirs.
Run it after the port is merged and while few agents build. Selftest uses a fake cargo: `bash tools/dev/test/mutants-selftest.sh`.

Native tests: `--native` adds `--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`; `--cargo-arg A` and `--cargo-test-arg A` pass extra arguments. A libtest flag without the separator is refused (exit 2). Before any mutant the unmutated `cargo test` runs once with the same arguments and must exit 0 with `running N tests`, N > 0 (else exit 5), because `--baseline skip` reports every mutant caught when cargo rejects the arguments. `--lint-args <raw args>` only checks a command line. See `docs/reviews/AUDIT-MUTATION-FLAG-2026-10-30.md`.

Golden files under cargo-mutants (CL-MUT-GOLDEN-INFRA): `tests/golden` is at the repo root and IS copied, but with a reused `CARGO_TARGET_DIR` the dependency `pr-test-support` stayed "Fresh" from an earlier temp copy, so its compile-time `CARGO_MANIFEST_DIR` pointed at a deleted directory (`no tests/golden above ...`) and every golden test failed. `pr_test_support::golden::repo_root()` now reads `PHOTOREVIEW_GOLDEN_ROOT`, then the run-time `CARGO_MANIFEST_DIR`, then the compile-time one; `mutants.sh` exports `PHOTOREVIEW_GOLDEN_ROOT`, bumps the mtime of the crate's `src`/`tests` (a reused target otherwise keeps artifacts of the last mutant of the previous run and the baseline fails) and prints a WARNING when any log still says `no tests/golden above`. Other tests using `env!("CARGO_MANIFEST_DIR")` directly are fine as long as they live in the mutated crate (rebuilt per copy). See `docs/reviews/CL-MUT-GOLDEN-INFRA-2026-10-31.md`.
