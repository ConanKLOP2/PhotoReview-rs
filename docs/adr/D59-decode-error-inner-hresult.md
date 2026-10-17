# D59: Preserve an inner COM HRESULT on NotSupported decode errors

- **Status:** Accepted
- **Date:** 2026-10-17
- **Decision owner:** User
- **Context:** C# `RawDecoder.IsRecoverablePreviewFailure` and `IsCorruptDataFailure` inspect a directly wrapped `COMException` inside `NotSupportedException`. Rust previously kept only the ICC transform message in `DecodeError::NotSupported`, so resource exhaustion could be treated as recoverable and a smaller preview could be returned.
- **Decision:** Add `inner_hresult: Option<i32>` to `DecodeError::NotSupported`. WIC ICC transform failures retain the original COM HRESULT when the cause is COM; `InvalidCast` and `InvalidArgument` ICC failures leave it absent. The metadata survives localized-text wrapping, source-byte wrapping, and preview-service error reconstruction.
- **Consequences:** `RawDecoder` classifies nested resource exhaustion as non-recoverable and nested WIC bad-image codes as corrupt data, matching the C# oracle. Adding a public field to `DecodeError::NotSupported` and changing the public `icc_transform_failed` constructor arity are source-breaking Rust API changes. All in-repository callers were updated; downstream callers must update their constructors.
- **Verification:** WIC decoder tests check HRESULT retention and non-COM behavior; `raw-decoder-fallback` golden tests exercise all three nested-HRESULT cases. On the integrated tree, `cargo fmt --all --check`, workspace clippy with `-D warnings`, and `cargo test --workspace --locked` passed (4,803 passed, 0 failed, 413 ignored); focused result details are in `task_on_progress.md`.
