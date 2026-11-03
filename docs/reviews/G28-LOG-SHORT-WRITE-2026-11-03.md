# G28 evidence: log short-write then failed retry (2026-11-03)

Source: `invariants-storage.md` L1, BACKLOG G28, W2-16 (`crates/pr-core/src/diagnostics/file_log.rs`). Diagnostics log only, not the journal.

## What PR #571 closed
- `write_remaining` (file_log.rs) tracks `offset`, so within ONE `drain_to_disk` call a retry after a short write never repeats bytes.
  Tests: `short_writes_are_continued_until_every_byte_is_written_exactly_once`, `failure_after_a_short_write_keeps_offset_so_retry_does_not_repeat_bytes`.
- The "do not over-promise" part: `docs/PARITY.md` P-08 already states (st-parity-sync4, PR #476) that a failed batch is retried whole and a line can repeat after two partial writes, equal to C# `FileLog.cs`.

## Closed by this change (tests only, same file's `write_remaining_tests`)
- A=6 bytes + half of B accepted, then two failures: offset stays 10, a later recovery appends only the rest of B, A appears once.
- Flush failing twice after all bytes were written: offset == len, retries only flush (write called once), no rewrite.
- `WriteZero` keeps offset 0; `Interrupted` is retried without loss/repeat.

## Still open: needs a behaviour change (not done)
`offset` and `buffer` are locals of one batch iteration in `drain_to_disk` (file_log.rs ~line 399-415). If the first write AND the in-drain retry both fail after bytes reached the file, entries stay un-flushed; the next drain rebuilds the whole buffer and appends from 0: A repeats after the half B (glued line). The sink cannot be injected into `drain_to_disk` (it opens the real path), so this cannot be tested end to end without a production seam, and a test asserting it would pin the defect. Not fixed: C# has the same behaviour, so a fix is a deliberate difference.

## Proposal P-140
Persist per-entry progress across drains: record the bytes already written for the head batch (or mark entries flushed by completed-line byte count and write a leading line terminator if the file ends mid-line), plus an injectable writer/opener seam in `drain_to_disk`. Needs an ADR + `Different` row vs C# `FileLog.cs:276-290`. Low priority (diagnostics log, only persistent I/O faults). Alternative: keep best-effort and only keep the PARITY wording (already present).

## Verdict
G28: test seam part complete; the end-to-end double-failure case is a documented best-effort limit pending P-140.
