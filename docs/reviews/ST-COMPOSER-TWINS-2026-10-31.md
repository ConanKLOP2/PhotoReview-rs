# ST-COMPOSER-TWINS (2026-10-31)

Status: done, no code change needed. Lane `st-composer-twins`, task 2 of section 6 in [ST-GAPMAP2](ST-GAPMAP2-2026-10-31.md).

## Result

The brief asked to port the three native composer twins `LargeImage_SplitIntoTiles_*`, `DeviceLostDuringACrossfade_*` and `Orientation_*`.
They are already on master since PR #404 (lane cl-misc, 2026-10-30), in `crates/pr-platform-win/tests/shell_viewing_composer_twins_native.rs`.
The ST-GAPMAP2 brief proposed a new file `crates/pr-shell/tests/w8_composer_twins.rs`; that would duplicate them, so none was written.

| C# test (`PresentationRenderingTests.cs` @ 7eaee2a2) | Rust test | Fidelity check (read side by side) |
|---|---|---|
| `Orientation_RawBufferDrawsLikeApplyOrientation(2,3,6,8)` | `orientation_raw_buffer_draws_like_apply_orientation` | same 40x40 surface, 20x12 smooth source, `apply_orientation` reference, `try_draw` with `NearestNeighbor`, row-exact compare in the destination crop |
| `LargeImage_SplitIntoTiles_HasNoSeamAtFit` | `large_image_split_into_tiles_has_no_seam_at_fit` | 17000x1200 one-colour image on a 1000x80 surface, all 1000 pixels at y=20 checked; Rust additionally asserts 2 tiles and 1 cache entry |
| `DeviceLostDuringACrossfade_ReuploadsBothImagesAndTheFadeContinues` | `device_lost_during_a_crossfade_reuploads_both_images_and_the_fade_continues` | same sequence: 2 cached, `recreate_device`, cache 0, fake clock 20000/20100, 2 re-uploaded, red 50 +-3, blue 150 +-3 |

All three are `#[ignore = "native: needs desktop"]`, offscreen WARP, fake frame clock, no sleeps, no window or desktop state.

## Run

`cargo test -p pr-platform-win --test shell_viewing_composer_twins_native -j 2 -- --include-ignored --test-threads=1` in this worktree: 3 passed, 0 failed (3.80 s; the build took 3m41s). Not run: any other binary, fmt or clippy (no file changed).

## CI

The three tests need `--include-ignored`. `.github/workflows/native-nightly.yml` lists `pr-platform-win` binaries `w2_fs_flush_killers, w4_mutation_killers2, w6_platform_native, physical_memory` only, so `shell_viewing_composer_twins_native` (and `shell_viewing_composer_native`) are not run by CI. They pass on this desktop with WARP. Adding both binaries to that list is a one-line change, left for the orchestrator (it needs a nightly run to confirm WARP exists on the runner).

## PARITY rows

No change needed: `X-viewing-composer` already says the three twins are ported and lists the remaining gaps (2 C# tests without a twin, `..DisposedByItsOwner` and `..DrawnBufferWasDisposed`; WARP only). Suggest adding to that row: "nightly list does not include the twin binaries" until they are added. Do not raise to Done.
