# raw_exif corpus tests honour PHOTOREVIEW_RAW_CORPUS_STRICT (2026-11-11)

Test-only change in `crates/pr-imaging-raw/tests`. No production code touched; no mutation run (nothing under `src/` changed).

- Finding: `raw_exif.rs` had 4 corpus skip sites (`corpus_samples_exif_summary_matches_survey`, the two CR3 rows via `corpus_cr3_exif_summary`, `corpus_orf_files_expose_exif`) that always skipped when the corpus was absent, ignoring `PHOTOREVIEW_RAW_CORPUS_STRICT=1`.
- `tests/common/corpus_skip.rs` had only `skipped()`; the strict switch was a per-file copy (cr3_reader, readers_cr2, raw_reader_arw, raf_reader). The shared module now also has `strict_value` (1/true, trimmed, case-insensitive: same rule as cr3_reader), a pure `gate(env, corpus_present) -> Run|Skip|Fail`, and `skip_or_strict_fail(why)` (reads the env, panics when strict, else calls `skipped`). Other files were not refactored.
- `raw_exif.rs` sites now call `skip_or_strict_fail`. `raw_corpus_presence.rs` counts both call forms so its skip-site report stays accurate.
- New default-run test `corpus_gate_decision_is_strict_aware` covers the pure `gate`/`strict_value` over env values (None, "", 0, false, yes, 2, t, 1, true, TRUE, padded) x corpus present/absent; it does not touch the process environment.
- Evidence: hand mutation `(false, true) => Fail` changed to `Skip` makes the new test fail (reverted). With no corpus, `PHOTOREVIEW_RAW_CORPUS_STRICT=1 cargo test --test raw_exif corpus_samples` fails with the "strict is set but the RAW corpus is missing" message; without it the test skips as before.
