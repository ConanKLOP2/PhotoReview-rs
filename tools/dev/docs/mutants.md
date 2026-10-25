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
