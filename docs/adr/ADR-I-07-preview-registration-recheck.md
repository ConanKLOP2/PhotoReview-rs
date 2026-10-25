# I-07: reuse a completed preview during in-flight registration

Date: 2026-10-26

Status: accepted as the root-cause repair required by the standing CI auto-fix workflow. This is an explicit concurrency difference from C# baseline `7eaee2a2`, not a claim that the baseline already contains this guard.

## Evidence

PR #239 failed `concurrent_requests_for_same_key_decode_once` in Windows CI run `40213093475`, job `121228379134`: SourceReads was 2 instead of 1. The aggregate build-test check failed because that shard failed. The failing test belongs to pr-imaging and is unchanged by W9-E5.

Both Rust `service/decode.rs::get_preview_attempt` and C# `PhotoReview.Imaging/Caching/PreviewImageService.cs::GetPreviewOnceAsync` (line452 at the pinned baseline) first check RAM and then fetch or create an in-flight entry. Between those operations, an earlier decode can publish its image and remove its entry. A caller that already saw the RAM miss then finds no entry and unnecessarily starts another decode. Atomic fetch-or-insert alone does not close this completed-load window. The shared C# window is a source-order inference, not a measured C# race reproduction.

## Decision

When in-flight lookup finds a vacant slot, check RAM again while holding the registration mutex. A resident image is returned by the same Arc and records a cache hit; no new cell, worker, source read or cache miss is created. If RAM is still empty, keep the original creator/joiner, epoch, cancellation and completion behavior.

The mutex is held only for lookup, the bounded in-memory cache access and insertion. It is not held across thread spawning, file I/O, decoding or waiting. The RAM cache releases its own lock before the call returns. Publication releases that lock before removing the in-flight entry; the change introduces no reverse simultaneous lock order.

This intentionally differs from the C# baseline for that interleaving: it avoids a redundant source read and its metrics. File/config/journal/cache formats and image data are unchanged. Register it as `D-preview-registration-cache-recheck` in PARITY. No C# checkout change is made.

## Alternatives considered

- Rerun or ignore the failing test: rejected; neither repairs the duplicate source read.
- Gate the decoder in the existing sixteen-thread test: rejected as the sole repair; it would keep the completed-load window out of that test.
- Recheck RAM under the registration mutex: selected; it closes the window with a bounded memory lookup and follows the project's read-once and review-speed priorities.

## Validation and limits

Keep the original sixteen-thread integration test unchanged. Add a deterministic stage regression: observe an initial RAM miss, publish an image as the earlier worker would, then enter registration with an empty in-flight table. Without the recheck it calls NullDecoder and fails; with it the exact cached Arc returns, cache hits is1, misses/source reads are0, and no registration remains. The registration stage is factored for this test without a timing hook or delay.

Run the imaging library/decode suites and final workspace gates. Selected mutation verification follows after merge. This does not promise one decode when the image was legitimately evicted, caching is disabled, the epoch changed, or the prior result was a non-cacheable degraded/error result. Native imaging, RAW corpus and performance measurements remain separate evidence.
