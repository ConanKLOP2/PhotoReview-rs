# FIX-BENCH-HOST 2026-10-31: Windows host for the benchmark CLI

Status: report. Lane `fix-bench-host`. Follows [ST-BENCH-BACKEND](ST-BENCH-BACKEND-2026-10-31.md) (PR #466) and question 2 of [ST-PARITY-SYNC4](ST-PARITY-SYNC4-2026-10-31.md).

## Choice and why

Option (a) of the report, which it recommends: a host crate. A `pr-app` feature is impossible (`pr-app` must not depend on `pr-platform-win`, D-W8B1-1), so the host is a new leaf crate `pr-bench-win` (nothing depends on it, no cycle, no `unsafe`). Option (b) would move three process-test files and `tools/bench-compare.ps1`. Decision recorded as P-60 (`PROPOSED, acted on, no user confirmation yet`) and [ADR](../adr/ADR-D-2026-10-31-bench-host.md). Cost: a second binary name, because two packages cannot both ship `pr-bench` without an output collision; the old `pr-bench` binary is unchanged (unwired host).

## What was done

- `crates/pr-bench-win/src/lib.rs`: `WindowsEngineHost` implements `EngineHost`: `WicDirectDecoder` over `PixelBufferImageCodec`, `WicPreviewPayloadCodec`, `WindowsMemoryProbe`, RAM from `WindowsPhysicalMemory`, process `FileLog` adapter and logging flag, `COMPUTERNAME`. The real `WindowsRecycleBin` (+ restore backend) is built only when `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1`; otherwise the temp-copy deleter. `with_parts` is the test seam.
- `crates/pr-bench-win/src/main.rs`: binary `pr-bench-win` = `pr_bench::cli::run` with that host.
- `tools/bench-compare.ps1` comment and `pr-bench` `main.rs` comment point to `pr-bench-win.exe`.

## Checks (what was run)

- `cargo check -p pr-bench -p pr-app -p pr-platform-win -p pr-bench-win`: finished, no dependency cycle.
- `cargo test -p pr-bench-win --test host_wiring -j 2`: 7 passed (recycle bin default deleter never builds the real bin; opt-in calls the factory with a fake; executor parts carry decoder, probe, RAM > 0; machine name; logging flag round trip; unwired host still exported; process test: `--benchmark-list-profiles` prints 30 lines of 8 fields).
- `cargo fmt -p pr-bench-win -p pr-bench --check` ok; `cargo clippy -p pr-bench-win --all-targets --locked -- -D warnings` clean; `python -I tools/dev/doc-links.py` 0 errors.
- NOT run: any profile, any benchmark, anything touching the real Recycle Bin; no native `#[ignore]` test was added (decoding needs only files, left to W10-B2).

## PARITY rows (not edited here)

`X-benchmark-compare`: replace "no real Windows host in the binary" by "`pr-bench-win` binary carries `WindowsEngineHost` (P-60); `pr-bench` stays unwired"; stays Partial until the attended W10-B2 run.
