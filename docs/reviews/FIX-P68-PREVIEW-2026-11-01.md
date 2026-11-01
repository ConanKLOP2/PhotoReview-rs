# FIX-P68-PREVIEW (lane fix-p68-preview, 2026-11-01)

Owner confirmed P-68 (`docs/PROPOSED-DECISIONS.md`, [FIX-G9-G7RES](FIX-G9-G7RES-2026-11-01.md)): remove the `panic!("{error}")` in
`LazyPrewarmPreviews::start_viewer_preview` (`crates/pr-shell/src/composition/adapters.rs`).

## Choice: option A (public failed-handle constructor)

- Added `PreviewHandle::failed(PreviewError)` (`pr-imaging/src/service/handle.rs`), `pub`, a thin wrapper over the existing `pub(crate) ready(Err(..))`.
  Reason: one additive method, no change to the `PrewarmPreviews` trait, its three fakes in `pr-app/tests` or any caller; the prewarm already handles a failed
  handle like any failed preview. Option B (`Option` return) would touch the trait, all fakes and the prewarm loop.
- API impact: one new public method on `PreviewHandle`, no existing signature changed, no `Cargo.toml` change, no new `PreviewError` variant (adding one would break exhaustive matches).
- The adapter now logs `Prewarm: the preview service could not be created, so the viewer preview is skipped` and answers
  `PreviewHandle::failed(PreviewError::Spawn(io::Error::other(<registry error text>)))`. `Spawn` ("a worker could not be started") is the closest existing variant; no panic is left on the user path.

## Seam and tests

- Seam: `composition_fixture::shared_services_with_failing_preview_service()` (blank `AppPaths::preview_cache_dir` makes `PreviewImageService::new` fail with
  `DiskDirectory`); the private fixture builder got an optional `AppPaths` editor, existing callers pass `None`.
- New `crates/pr-shell/tests/fix_p68_preview.rs` (3 tests): the seam fails `preview_service()`; `start_viewer_preview` returns a ready, failed handle and logs;
  `current_cache_key` still answers an error and logs.
- Red first: before the fix `start_viewer_preview_answers_a_failed_handle_...` panicked at `adapters.rs:189`. After the fix: 3 passed
  (`cargo test -p pr-shell --test fix_p68_preview -j 2`). `cargo fmt --check` for pr-shell and pr-imaging clean; `doc-links.py` 0 errors.
- Not run (limit): clippy, workspace tests, other pr-shell/pr-app test binaries; CI covers them.

## PARITY rows that should change (not edited here)

- The startup/composition row mentioning `LazyPrewarmPreviews` / G7: note that the `adapters.rs` `panic!` is gone (replaced by a logged failed handle), covered by `fix_p68_preview.rs`. No status upgrade needed.
- `docs/PROPOSED-DECISIONS.md` P-68: mark confirmed and done (option A).
