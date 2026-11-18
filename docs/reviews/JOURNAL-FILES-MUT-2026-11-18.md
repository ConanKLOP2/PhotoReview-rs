Status: evidence report (2026-11-18). Clean whole-file rerun of four file-action journal files against the whole `pr-core` suite on master `8e613291`, closing the "not run" columns of `W3-01-JOURNAL-MUTATION.md` (`journal.rs` 85, `journal_line.rs` 15, `journal_rules.rs` 2, `recovery_retry.rs` 30). Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=900`; two runs in parallel, one worktree and one target dir each. No source or test change.

# Journal files: whole-file mutation rerun

| File | Identities | Caught | Unviable | Missed | Timeout |
|---|---|---|---|---|---|
| `journal.rs` | 181 | 142 | 26 | 1 | 12 |
| `journal_line.rs` | 139 | 118 | 17 | 4 | 0 |
| `journal_rules.rs` | 22 | 21 | 1 | 0 | 0 |
| `recovery_retry.rs` | 158 | 142 | 16 | 0 | 0 |

(`journal_line.rs` and `recovery_retry.rs` counts come from the per-file grep of one 319-identity run: 281 caught, 34 unviable, 4 missed, 0 timeout.)

This agrees with the earlier decision of `journal.rs` in [CL-JRNL4](CL-JRNL4-2026-10-31.md) (one equivalent, nothing missed apart from it, hangs counted as detected); the identity count differs by two because the file changed since.

## Timeouts (12, all `journal.rs`)

All are hangs, detected by non-termination, not by an assertion: `58:50`, `316:32`, `316:38`, `331:9`, `333:24` (gate), `700:29`, `700:55`, `702:29` (append retry loop), `935:60`, `949:11`, `951:73` ×2 (reverse reader and `parse_committed_moves`).

## Survivors, all equivalent by argument (5)

| Mutant | Argument |
|---|---|
| `journal.rs:925:48` `first_newline + 1` -> `* 1` in `read_committed_moves_reverse` | `line_start` and `new_boundary` move one byte earlier, onto the `'\n'`; the parser skips the empty piece, a final piece without `'\n'` is parsed as before and `'\r'` is still stripped, so the entries returned are the same |
| `journal_line.rs:596:9`, `619:9`, `647:9` `Visitor::expecting` -> `Ok(Default::default())` | the text only appears in serde error messages; `try_parse` and `try_parse_utf8` return `Option`, so the message cannot be observed through the public API |
| `journal_line.rs:623:9` `GroupMembersVisitor::visit_unit` -> `Ok(Default::default())` | `Default` of `Option<Vec<_>>` is `None`, the same value |

## Follow-up

The earlier `journal_compaction_files` end-to-end rerun remains separate and is not covered here.
