# Bench host: a leaf crate with the Windows `EngineHost` (P-61)

Status: PROPOSED, acted on, no user confirmation yet (P-61). Date: 2026-10-31. Source question: `ST-PARITY-SYNC4` "Questions waiting for the owner", item 2.

## Context

`pr-bench` has the engine backend (`EngineBackend<H: EngineHost>`, PR #466) but its binary used `UnwiredEngineHost`, because the WIC decoder, payload codec, `WindowsMemoryProbe` and `WindowsRecycleBin` live in `pr-platform-win`, which depends on `pr-app`, which depends on `pr-bench` (D-W8B1-1). A profile run therefore failed with `the benchmark engine is not wired into this build`.

## Options

- (a) A host crate (or a `pr-app` feature) that builds the CLI with a Windows host. A `pr-app` feature cannot work: `pr-app` may not depend on `pr-platform-win` (that is the D-W8B1-1 direction), so only a new leaf crate qualifies.
- (b) Move the binary out of `pr-bench`: clean graph, but the three process-test files (`cli_process`, `bench_compare_cli`, `w7_followup_killers`) and `tools/bench-compare.ps1` move with it.

## Decision

(a), as the report recommends. New crate `pr-bench-win` (nothing depends on it, so no cycle; no `unsafe`):

- `WindowsEngineHost` implements `EngineHost`: `WicDirectDecoder` over `PixelBufferImageCodec`, `WicPreviewPayloadCodec`, `WindowsMemoryProbe`, physical RAM from `WindowsPhysicalMemory`, the process `FileLog` (logging flag included), `COMPUTERNAME`. The real `WindowsRecycleBin` is built only when `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1` (as before); otherwise the benchmark's temp-copy deleter is used.
- Binary `pr-bench-win` runs the same `pr_bench::cli::run`. The `pr-bench` binary stays unchanged (unwired host): `--benchmark-list-profiles`, `compare`, `--bench-compare` and its process tests are unaffected.
- A second binary name is the cost of option (a): keeping the name `pr-bench` in both packages would collide in the target directory. Profile runs and `tools/bench-compare.ps1 -CandidateExe` use `pr-bench-win.exe`.

## Consequences

No measurement was made. A profile run now reaches the real engine and real decoder; W10-B2 (attended, own copies of photos, real bin off) is still required before any performance claim. If the owner prefers (b), the crate's `lib.rs` moves into the `pr-bench` package only after the cycle is cut.
