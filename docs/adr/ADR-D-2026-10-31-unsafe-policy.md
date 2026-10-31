# ADR: `unsafe` in `pr-core` and `pr-imaging` is grandfathered, new `unsafe` stays in the FFI crates (P-64)

Date: 2026-10-31

Status: PROPOSED, acted on, no user confirmation yet (proposal row `P-64` in `docs/PROPOSED-DECISIONS.md`). It takes the recommended option of question 4 of `docs/reviews/ST-PARITY-SYNC4-2026-10-31.md` ("Questions waiting for the owner").

## Context

`AGENTS.md` (Coding conventions) lets `unsafe` live only in the FFI crates: `pr-platform-win`, `pr-turbojpeg`, `pr-libraw` (plus the test-only Win32 ProcessStatus helper). `tools/dev/unsafe-audit.py` enforces it: rule A (a `SAFETY:` comment within 3 lines) and rule B (`unsafe` outside the allowed crates), with the exceptions listed in `tools/dev/unsafe-audit.baseline`.

Two crates outside that list already contain `unsafe` and are carried by baseline rule B (checked 2026-10-31 against `tools/dev/unsafe-audit.baseline`):

| Crate | Production `src` lines | Test lines | Sites |
|---|---|---|---|
| `pr-core` | 9 | 0 | `io/journal_compaction_files.rs` (3), `io/physical.rs` (4), `text/dotnet_path.rs` (2): `extern "system"` declarations and their calls (file move, volume and path APIs) |
| `pr-imaging` | 6 | 12 | `cache/atomic_file.rs` (2), `pixel_memory.rs` (2), `pixels.rs` (2): an `extern` declaration, memory accounting and one `from_raw_parts`; the tests are in `tests/pixel_memory.rs` (a counting allocator) |

Most of these also carry rule A entries (no `SAFETY:` comment within 3 lines), which the baseline accepts today.

## Options

1. Add `pr-core` and `pr-imaging` to the allowed list with the rule A (`SAFETY:`) check. Honest: the code exists and is reviewed; it records the real policy. Costs a change of `AGENTS.md` and of the audit default.
2. Keep both grandfathered under rule B (no change): the policy text keeps saying "FFI crates only", which is not what the repository does.
3. Move the sites into `pr-platform-win` (large refactor; `pr-core` must not depend on Windows-only crates, so each site would need a new seam).

## Decision (recommended option 1, in its smallest form)

- The 9 production lines of `pr-core` and the 6 production lines of `pr-imaging` listed above are a **closed, grandfathered set**. They are accepted as they are.
- **No new `unsafe` in `pr-core` or `pr-imaging`.** A new site needs an ADR or a user decision first and lives in `pr-platform-win` when it is a Windows call. The baseline is not widened for new lines.
- Existing sites should gain a `// SAFETY:` comment when they are touched (rule A), and the set only shrinks.
- `AGENTS.md` (unsafe line) and `tools/dev/docs/unsafe-audit.md` state this in one sentence each.

## Not done here

`tools/dev/unsafe-audit.py` (its `ALLOWED` default) and `tools/dev/unsafe-audit.baseline` are NOT edited by this decision. The baseline therefore still lists the sites as rule B; the audit result is unchanged. Moving the two crates into the audit's allowed list, and then dropping their B entries from the baseline, is a follow-up code change that needs its own lane (the script and baseline are outside the docs lane). Option 3 stays possible later.

## Consequences and undo

- Documentation now matches the repository; reviewers have a rule for a new `unsafe` in the two crates.
- Undo: restore the previous wording of the `AGENTS.md` line and the audit doc sentence, and mark this ADR rejected.
