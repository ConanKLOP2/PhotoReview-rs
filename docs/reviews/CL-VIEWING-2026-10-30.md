# CL-VIEWING 2026-10-30: viewing pure logic mutation (frame_stats, pacer, frame_log, compare, crossfade, composer)

Status: partial. `X-viewing-pure` reaches Done (every mutant of its five files has now been run). `X-viewing-compare-layer`, `X-viewing-crossfade` and `X-viewing-composer` stay Partial (see "Open").

Killers file: `crates/pr-platform-win/tests/w8_viewing_killers.rs` (17 tests, no sleeps, no window; fake clock `FakeClock`, recording draw context, fake caches). All pass. Machine: Windows, `cargo-mutants 27.1.0`, `--baseline skip --timeout 90 --build-timeout 300 -j 1`, own `target-mut` and `--output mut-out-*` per group.

## Mutation results

| Group (files, filter) | Test binaries | Tested / listed | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| `X-viewing-pure` remainder: `frame_stats.rs` (getters, percentiles, max), `pacer.rs` (`is_degraded`, `deferred_total`), `frame_log.rs` (`open`, `from_environment`, `close`, `Drop`) | `shell_viewing_frame_stats`, `shell_viewing_pacer`, `shell_viewing_frame_log` | 29 / 29 | 25 | 1 (equivalent) | 3 | 0 |
| `compare.rs`, whole file | `shell_viewing_compare_crossfade` + `w8_viewing_killers` | 139 / 139 | 130 | 3 (equivalent) | 6 | 0 |
| `crossfade.rs`, everything except `start`/`start_count` (those 20 were done by CX-HOST) | `shell_viewing_compare_crossfade` + `w8_host_mutation_killers` | 39 / 39 | 30 | 8 | 1 | 0 |
| the 8 `crossfade.rs` survivors, killers binary alone | `w8_viewing_killers` | 8 / 8 | 7 | 1 (equivalent) | 0 | 0 |
| `composer.rs`, whole file, STOPPED at the time box | `shell_viewing_composer` + `w8_viewing_killers` | 31 / 89 | 29 | 1 (equivalent) | 1 | 0 |

`X-viewing-pure` totals after this run: `scaling_policy` + `image_layer` 33/33 (CORE-LANE), `frame_stats` 55/55, `pacer` 23/23, `frame_log` 10/10. Earlier verifier result (W8-PURE-MUTATION) 59 tested of the 88 platform mutants plus these 29 = all 88 run; survivors are 4 equivalent (earlier) + 1 equivalent (here).

## Survivors

Equivalent (no test can see them):

- `frame_log.rs:108` `Drop for FrameLog -> ()`: `Drop` only calls `close()`, which sets `file = None`; without it the `File` field is dropped by the compiler right after. Same observable state.
- `compare.rs:127:53` and `:128:55` `* -> +` in `layout::content`: `2.0 * BORDER_THICKNESS` and `2.0 + BORDER_THICKNESS` are both `4.0` because `BORDER_THICKNESS == 2.0`.
- `compare.rs:95:23` `|| -> &&` in `layout::net_min`: the NaN arm is unreachable. Its only caller `layout::image` returns early unless `content.width > 0 && content.height > 0` (false for NaN) and divides by positive integer pixel sizes, so neither operand can be NaN.
- `crossfade.rs:58:35` `&& -> ||` in `Store::opacity`: needs a running tween with `outgoing == None`. Every public path that clears `outgoing` (`cancel`, `on_frame` at opacity 0, `dispose`, `draw` releasing a layer whose animator is idle) also stops the tween, and the only other path (`draw` seeing a disposed buffer) cannot be reached because an `Arc<PixelBuffer>` held by the layer cannot be disposed (documented C# twin gap).
- `composer.rs:158:37` `|| -> &&` in `arm_pending_deadline` job: `disposed && pending.is_none()` differs from `disposed || pending.is_none()` only when the composer is live and `pending` is empty with the token still current; then `try_promote_pending()` returns false (no `pending`) and `invalidate` is not called, the same as the early return.

Killed by `w8_viewing_killers.rs` (each test names mutant file:line and operator in a comment):

- compare: layout exact values and clamps (`panel`, `border`, `content`, `image`), empty-image cases, zero-width and zero-height hit tests, full draw call sequences (rectangles, colours, order, clip, opacity), border thickness for tiny cells (`inner == 0` boundary), empty panel / empty cell guards, one-image counts.
- crossfade: `set_opacity` write-through, `Debug`, `draw` release rules (idle with opacity 0 releases, animating with opacity 0 does not), `draw` cancels only when the cache reports a disposed buffer.

No production code was changed; no mutant exposed a real bug.

## Open

- Composer: 58 of 89 mutants NOT run (time box; the machine was shared with other lanes). A partial run is not a full verdict. `X-viewing-composer` stays Partial with its other gaps (native twins `LargeImage_SplitIntoTiles_*`, `DeviceLostDuringACrossfade_*`, `Orientation_*`, WARP only).
- Compare / crossfade: the C# scene/pixel golden comparison with WPF and hardware GPU runs are human-only gaps (attended machine); these two rows stay Partial. `crossfade.rs` has 2 C# tests without a twin.
- `compare.rs` caught counts come from the combined run (existing tests + killers); only crossfade survivors were re-proved with the killers binary alone.
- Native tests (`shell_viewing_composer_native`, `shell_viewing_crossfade_native`) were not re-run in this lane; `stage-native.ps1` was not needed for the pure binaries.
- Commands: `cargo mutants -p pr-platform-win --baseline skip --timeout 90 --build-timeout 300 -j 1 --file "**/shell_viewing/<file>.rs" [-F ...|-E "CrossfadeLayer::start"] --output <worktree>/mut-out-<group> --cargo-arg=--test=<binary>`.
