# ADR-INV: journal durability default, Recycle error UI, native-test CI job

- **Status:** Accepted by the project lead on 2026-10-23 ("use the proposals"), recorded by machine C (cloud).
- **Sources:** `docs/reviews/invariants.md` M2, L3; `docs/reviews/test-quality.md` M1; `docs/reviews/BACKLOG.md` G24 (Q1, Q2, Q7).
- **Decision owner:** project lead

## Q1: `JournalDurability` default stays `Fast`
Context: with `Fast` the Prepared line is only flushed, not synced (`pr-core/src/file_actions/journal.rs` `append_lines`); a power loss can lose it while the NTFS rename survived, leaving a move with no journal trace. C# behaves the same (ADR 0007, IO03).
Decision: keep `Fast` as the default (parity, review speed first). `PowerLossSafe` stays a Settings option. No `Different` row. G19 adds tests that pin both modes.

## Q2: Recycle keeps the C# shell flags; add a waiting notice
Context: `FOF_NOERRORUI` is deliberately absent (C# `ShellFileRecycler.cs:20`); a shell error dialog without an owner can hold the INV-4 gate until dismissed.
Decision: keep the flags (parity). Add a status notice "waiting for a system dialog" when a Recycle runs longer than 10 s (implementation: a new Codex group, follows G12). No `Different` row because behaviour is unchanged.

## Q7: native tests get a separate nightly CI job
Context: CI runs `cargo test` without `--ignored`; 1157 tests never run (`docs/reviews/test-quality.md` M1).
Decision: add a `native` job on `windows-latest`, scheduled nightly, running `cargo test --workspace --locked -- --include-ignored` with the pinned DLLs staged as in P-50. It does not gate PRs yet; revisit after two weeks of results. The workflow file is a draft in `docs/drafts/` first (`.github/workflows` changes need the lead).
