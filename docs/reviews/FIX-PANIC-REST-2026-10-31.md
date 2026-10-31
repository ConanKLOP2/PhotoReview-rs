# FIX-PANIC-REST (2026-10-31): the remaining worker jobs turn a panic into their own failure result

Status: fixed on this branch (reliability lane, continues [FIX-WORKER-PANICS-2026-10-31.md](FIX-WORKER-PANICS-2026-10-31.md), PR #473). Source: [ST-APP-RACES-2026-10-31.md](ST-APP-RACES-2026-10-31.md) M3.
C# reference: a faulted `Task` still runs its continuation (`ImagePresenter`, `ZoomDetailLoader.LoadAsync` `catch`/`finally`, `SiblingFolderNavigator` awaits), so the phase / slot is always released.

## Pattern (same as #473)
One helper, `coordinators::zoom_detail::catch_job(work, on_panic)`: `catch_unwind` around the job body; on a panic `on_panic(text)` builds the failure value the job
already sends. The non-panic path is `work()` unchanged. No event enum gained a variant; the existing consumer arm releases the state.

## Per job
| Job | Completion event | Failure value on panic | Released state |
|---|---|---|---|
| `present_a` preview wait | `PresenterEvent::Preview` | `Err(PreviewError::Panicked)` | `Phase::AwaitPreview` (generic present failure) |
| `present_a` thumbnail wait | `PresenterEvent::Thumbnail` | `Err(ThumbnailError::Panicked)` | thumbnail race |
| `present_b` compare previews (x2) | `ComparePreview` | `Err(PreviewError::Panicked)` | `Phase::AwaitCompare` |
| `present_b` compare sizes | `CompareSizes` | `None, None` (size suffix is cosmetic, like a failed stat) | `AwaitCompare` / `Sizes` |
| `present_b` compare hash (x2) | `CompareHash` | `Err(FileHashError::Io("hash computation panicked: .."))` (same text as #473 M2) | `AwaitCompare` / `Hashes` (shows "hash unknown") |
| `present_b` original dimensions | `Dimensions` | `Err(PreviewError::Panicked)` | `Phase::AwaitDimensions` (generic failure) |
| `zoom_detail` decode | `OriginalDecoded` | `Err(PreviewError::Panicked)` | the `load` slot ("decoding") |
| overlay sibling search (`state.rs`) | `MainEvent::OverlaySiblings` | `SearchOutcome::Failed` (via `SiblingNavError::Panicked`) | `pending_worker_jobs` |
| sibling folder search (`navigation.rs`) | `MainEvent::SiblingFound` | `Err(SiblingNavError::Panicked)` (logged) | `pending_worker_jobs` |

`SiblingNavError` gained `Panicked(String)` (Display added; the enum is no longer `Copy`; no other crate uses it). The stat job already has its own drop guard; the folder-ownership job already caught.

## Tests (red with `catch_job` replaced by a pass-through, green with the fix)
- `presenter_present_b`: `present_dimension_job_panic_is_a_failure_not_a_stuck_present`, `present_compare_hash_job_panic_is_a_failure_not_a_stuck_present` (red: 2 failed).
- `viewmodels_main_state`: `overlay_sibling_search_job_panic_still_releases_the_pending_count`, `sibling_folder_search_job_panic_still_releases_the_pending_count` (red: 2 failed). Seam: `EnvOptions::sibling_raw_getter`.
- `zoom_detail`: `catch_job_turns_a_panic_into_the_failure_result_and_keeps_the_normal_result`.
Green: `presenter_present_b` 60, `viewmodels_main_state` 51, `zoom_detail` 45, `coordinators_sibling_navigator` 25, `viewmodels_main_navigation` 66; fmt and one `clippy -p pr-app --all-targets -- -D warnings` clean.

## Honest limits
- Preview wait, thumbnail wait, compare preview, compare size and the zoom decode have NO test that panics inside the job: `PreviewImageService`, the thumbnail cache and the preload worker already turn decoder panics into
  `Panicked` results, and there is no injectable seam that panics in `PreviewHandle::wait` / `compare_file_size`. Those catches are defensive and covered only by the `catch_job` unit test.
- Not changed: the generic `ThreadRunner` (a catch there can only log; it cannot invent an event) - the per-job approach above makes it unnecessary.
- Deferred: none of the listed jobs.
- PARITY: no row changes proposed (reliability fix, no new C# behaviour).
