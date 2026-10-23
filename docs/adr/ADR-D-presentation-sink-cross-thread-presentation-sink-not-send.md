# ADR-D-presentation-sink-cross-thread: The presentation sink and hub are !Send; cross-thread marshalling and its counter are not ported

- **Status:** Proposed (draft written by DOC-ADR-DRAFTS, machine C, 2026-10-23; not accepted by the lead)
- **PARITY row:** `D-presentation-sink-cross-thread` in `docs/PARITY.md`, listed in `docs/PARITY-CLOSEOUT.md` ("Different")
- **Decision owner:** project lead

## Context
C# ADR 0005 has `Win32PresentationSink.InvokeUi` marshal an update from another thread, with a FIFO `_queued` guard and `ReviewMetrics.RecordCrossThreadPresent()` / `CrossThreadPresentCount`. The Rust sink and hub hold `Rc`s, so they are `!Send` and an off-thread call does not compile. D-W8B4V-10 and the row (`Different (compile-time)`) record this; contract W8-16i asks the lead whether a one-paragraph ADR is wanted.

## Proposed decision
Do not port the guard. The situation it handles cannot be written in Rust; `CrossThreadPresentCount` stays 0, the value ADR 0005 expects. The compile-time assertion `hub_and_sink_are_not_send` replaces the 4 untranslated `Win32PresentationSinkTests`.

## Consequences
A future change that makes the hub `Send` would silently remove the guarantee unless the assertion test is kept. The dispatcher and metrics constructor arguments of the C# sink are dropped.

## Alternatives not chosen
Port the FIFO marshalling queue and counter (needs a `Send` sink and a dispatcher; contradicts the single-UI-thread ownership of R0003).

## Sources
`docs/PARITY.md` row `D-presentation-sink-cross-thread`; `contracts/README.md` D-W8B4V-10; `contracts/W8-16i.md` item 7 and open question 2.

## Not verified
The C# ADR 0005 text was not re-read.
