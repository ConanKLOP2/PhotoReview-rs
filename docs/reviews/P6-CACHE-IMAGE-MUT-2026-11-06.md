Status: evidence report (2026-11-06), task P6, branch `test/mut6-cache-image` from origin/master 3d2d75a2. One new test file and this file; no production change, no edit of PARITY, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress or SYNC-LEDGER.

# P6: `pr-platform-win/src/wic/cache_image.rs` (W4-11e), mutation under OWNER RULE B (strict, P-391)

Rule B: a mutant is closed only if caught, unviable (a real rustc error), or proven equivalent over the whole domain or every input class. A timeout is never caught; infrastructure failures count as NOT RUN.

## Method

* `cargo mutants --list --file crates/pr-platform-win/src/wic/cache_image.rs`: 130 mutants (same 130 as P3). All 130 were run again in one process (the 22 decided in P3 were not excluded; they were simply re-run, 12 minutes in total because of `--cargo-arg --test=`).
* `bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/wic/cache_image.rs --cargo-arg --test=<binary> ... --budget 40000 --jobs 1 --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1 --cargo-test-arg --skip --cargo-test-arg encode_jpeg_and_decode_are_pinned_by_golden_hash`, `MUT_TIMEOUT=600`, `CARGO_BUILD_JOBS=2`, TEMP/TMP `C:\Users\PC\Projects\target-mut6-tmp`. Test binaries: `wic_cache_image`, `w4_mutation_killers2`, `w4_mutation_killers3`, `wic_repair1`, `wic_cache_adapters`, `preview_cache_wic` (`--cargo-arg` limits the build as well; one mutant cost about 5 s build). Native (WIC) tests run with `--include-ignored`; no attended desktop is needed.
* Preflight of the unmutated command: 75 tests executed, exit 0. Excluded because it already fails on the unmutated tree of this machine (PNG payload 349 vs 346 bytes, WIC build): `wic_cache_image::encode_jpeg_and_decode_are_pinned_by_golden_hash`.
* Second run (`-F` on the four survivors, with `mut6_cache_image` added to the binaries): 9 mutants matched the filters (the 4 survivors plus 5 neighbours on the same line:col), 5 caught, the same 4 still MISSED (expected: they are equivalent).
* Checked: all 12 unviable logs hold a real rustc error in `crates/*/src` (`Guid`/`PixelBuffer`/`WicCacheImageInfo: Default` not satisfied, and `<` parsed as generic arguments at 247:19); no log has an infrastructure marker (the "1455"/"Dirty" hits are timestamps). Every caught mutant has a failing test in its log.

## Counts (tested = caught + missed + unviable + timeout)

| file | listed | tested | caught | missed | unviable | timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|---:|
| `wic/cache_image.rs` | 130 | 130 | 114 | 4 | 12 | 0 | 0 |

The 4 missed are all argued equivalent below (2 by arithmetic over every residue class, 2 by a differential run over the whole input domain of the option). Caught + equivalent = 118 of 130; unviable 12.

## Survivors

| mutant | fate | basis |
|---|---|---|
| 293:14 `>` -> `>=` in `plan_decode` | equivalent | `bytes = w * h * 4` (u64, no overflow: `checked_mul` of the maximum sides is `Some`) is a multiple of 4; `MAX_DECODED_BYTES = 2^31 - 1` is 3 mod 4. They are never equal, so `>` and `>=` agree for every input. Test `plan_decode_size_limit_operands_can_never_be_equal` executes the 16 residue classes of (w, h) mod 4, pins both premises, and the real function at 2^29 pixels (refused) and 2^29 - 1 (accepted). |
| 466:28 `>` -> `>=` in `validate_pixels` | equivalent | `byte_count = width * 4 * height` (also in the test hook; a wrap modulo 2^64 keeps the residue mod 4) is 0 mod 4; `u32::MAX` is 3 mod 4. Test `pixel_buffer_byte_count_is_never_u32_max`: residue classes, the corner geometries on the real constructors, and the refusal at 2^32 bytes. |
| 679:13 delete `dwType` | equivalent (differential) | The WIC JPEG encoder reads the option by name and takes the type from the VARIANT. Test `jpeg_image_quality_bag_ignores_dwtype_and_vt_for_every_quality` (native: needs WIC) re-implements `encode` with the four field variants (full, no dwType, no vt, neither): bytes identical to each other AND to production `encode_jpeg` for EVERY quality 1..=100, two sizes (16x8 multi-pixel, 33x17) and four pixel-content classes (flat, patterned, ramp, hash noise); every `IPropertyBag2::Write` succeeds. |
| 680:13 delete `vt` | equivalent (differential) | same test. |

Caveat for the verifier: the two PROPBAG2 equivalences are a differential run of a replica of the mutated code against the real WIC encoder. Quality (the only input of the bag) is covered completely; pixel content is not a parameter of the bag but is not enumerated (impossible), so a strict reader may keep 679:13 and 680:13 as run-supported, open. The two arithmetic ones do not depend on a sample.

## Caught by existing tests (no new test needed): 114 mutants; unviable: 12

127:5, 147:5, 247:19 (`<`, compile error in one build; its `==`/`>=` variants are caught), 268:5, 320:5, 487:5, 514:5, 545:9, 554:9, 558:9, 564:9, 589:9.

## New test

`crates/pr-platform-win/tests/mut6_cache_image.rs`: 3 tests (one `#[ignore = "native: needs WIC"]`, runs with `--include-ignored`, no attended desktop). The test file has four `unsafe` blocks, each with a `// SAFETY:` comment.

## Checks

`rustfmt --edition 2024 --check` on the file; `cargo clippy -p pr-platform-win --tests -- -D warnings`; `cargo test -p pr-platform-win --test mut6_cache_image -- --include-ignored` (3 passed); `PY_BIN=... bash tools/dev/pre-pr.sh` (all ok except `cargo fmt -p`, which fails with os error 206 as known; rustfmt on the file passes).

## NOT RUN

None for this file (130 of 130 tested).
