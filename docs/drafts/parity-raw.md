# PARITY row proposals: Wave 10 RAW/native residual

Proposals only; do not edit `docs/PARITY.md` in this task. Confirm current row keys and wording during integration.

| Proposed area | Proposed status | Evidence required to mark Done |
|---|---|---|
| LibRaw native runtime (`native.rs`, `decoder_native.rs`, `availability.rs`) | Partial | Windows x64 run with hash-verified LibRaw DLL; report exact test binaries/cases, strict result and timeout outcome. |
| TurboJPEG native runtime (`native.rs`, `decoder_native.rs`, `native_golden.rs`) | Partial | Windows x64 run with hash-verified TurboJPEG DLL; report native golden and selected tests. |
| RAW corpus readers and decoder corpus parity | Partial | Explicit consent, complete verified manifest corpus, strict run, and regenerated/compared named corpus goldens. |
| W5 mutation coverage | Partial | Reconcile exact mutant identities; close open items or document equivalent/unviable outcomes with rationale. |

Existing Wave 5 ledger says native DLL and corpus execution remain unverified (task_on_progress.md lines 181-188). Do not promote ignored-by-default, synthetic fixture, or default test success into native/corpus evidence.
