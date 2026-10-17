# D60: Fail closed on malformed RawOrientationPatcher test inputs

- **Status:** Accepted
- **Date:** 2026-10-17
- **Decision owner:** Project
- **Context:** The test-only Rust port of C# `RawOrientationPatcher` supports a corpus-backed portrait-orientation test. For valid parser-derived offsets and TIFF/JPEG data, the routing and byte updates match the pinned C# baseline `d92d6aae`. On malformed inputs, however, C# can throw for a truncated APP1 Exif signature span, negative or oversized preview offsets, and checked TIFF-offset conversion; the Rust helper uses checked arithmetic and bounded slices and returns `false`.
- **Decision:** Keep the Rust helper fail-closed and record this as a test-helper-only behavior difference. Do not reproduce accidental byte edits from negative offsets or C# indexing/overflow exceptions. Add synthetic tests that pin the malformed-input behavior.
- **Consequences:** Valid corpus fixtures follow the same patching routes. Invalid synthetic fixtures are not byte-for-byte behavior-compatible with the C# helper, but they cannot panic or write outside the caller-provided slice. No production decoder behavior or public API changes.
- **Verification:** `raw_orientation_patcher` synthetic tests cover TIFF-at-zero, RAF embedded EXIF, RW2 fallback, CR3 CMT1, malformed CMT1, and fail-closed handling for a truncated APP1 and invalid preview offsets.
