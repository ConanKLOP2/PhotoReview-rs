# FIX-LOGGING-OFF - `startup_with_logging_off_writes_no_log_file` (2026-10-31)

Status: finding closed, not a production bug (INV-10 holds). Test diagnostics improved only.

## Finding (from ST-EARLY-STJ)

`cargo test -p pr-shell --test startup_native -- --include-ignored --test-threads=1` failed with `data/logs/app.log` present after the real exe ran with logging off.

## Reproduction and cause

Run 1 (target dir fresh, native DLLs NOT staged), test improved to print the file content. The exe wrote exactly two lines, both `[ERROR]`:

- `TurboJPEG decoder backend is not available and will not be registered: Could not load turbojpeg.dll ...`
- `LibRaw decoder backend is not available and will not be registered: Could not load libraw.dll ...`

They come from `pr-shell/src/composition/decoders.rs` `create_decoder_providers_with`, which calls `CompositionLog::error_forced` (`AppLog::error_forced`) when a decoder probe fails. That is the C# behaviour (`AppLog.ErrorForced`: a forced error is written even while logging is off and creates the file; INV-10 only forbids ordinary/info/warn output). With `libraw.dll` and `turbojpeg.dll` present the probes pass and nothing is logged.

Run 2 after `tools/dev/stage-native.ps1` (PASS: both DLLs staged and SHA-verified against C# 7eaee2a2), no rebuild: `startup_native` 3 passed, 0 failed (including `startup_with_logging_off_writes_no_log_file`, 8.2 s).

So the failure was the environment (unstaged native DLLs), not a defect: no code change in pr-core/pr-app/pr-shell, no DIVERGENCE.

## Change

`crates/pr-shell/tests/startup_native.rs`: the assertion now prints the content of the unexpected `app.log`, and a comment documents the precondition (stage the DLLs; a failed probe legitimately yields a forced error). Behaviour of passing runs is unchanged.

## Not done

No clippy/workspace build (not needed: test-only message change; fmt clean). The nightly workflow has no staging step, so on windows-latest this test stays red until staging exists there (same gap noted for `pr-libraw raw_share_delete`); the failure message now says why.

## PARITY rows

None to change; X-startup-early-decode note about the red test can be dropped (it passes when staged).
