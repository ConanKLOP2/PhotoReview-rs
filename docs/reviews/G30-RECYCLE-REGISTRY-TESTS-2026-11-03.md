# G30 recycle-policy M1: second registry read, test coverage check (2026-11-03)

Result: BACKLOG G30 is already done. No test and no production change was added.

## C# behaviour (spec baseline)
`RecycleBinCapacity.cs:219-233` (`ReadDword`): a value from `RegistryKey.GetValue` is accepted only as `int` (returned unsigned); null stays null; any other type throws `InvalidDataException`. C# has a single read, so there is no second query to validate.

## Rust code
PR #532 (`f4fedc8`, 2026-11-01) added `read_dword_with_query` in `crates/pr-platform-win/src/recycle_bin_settings.rs`. It checks kind == REG_DWORD (4) and byte count == 4 after BOTH queries. A value that vanishes between the queries is `Err(NotFound)`. `read_registry_value` calls it through a closure over `RegQueryValueExW`. The seam takes a closure, so a fake registry needs no production change.

## Tests (all in `crates/pr-platform-win/tests/recycle_bin_settings_capacity.rs`)
Each M1 test scenario from `recycle-policy.md` is covered:
- probe DWORD/4 then BINARY/4, DWORD/1, DWORD/0, DWORD/8 gives `Other`: `second_query_with_changed_kind_or_length_is_other`
- wrong first query skips the data query: `first_query_with_wrong_kind_or_length_is_other_and_data_query_is_skipped`
- DWORD/4 control gives `Dword(u32::MAX)`, absent gives `None`: `matching_queries_return_the_dword_including_u32_max`
- vanish, ACCESS_DENIED, InvalidData, ERROR_MORE_DATA (234) fail closed: `value_vanishing_or_failing_between_the_queries_fails_closed`
- through the settings source and `RecycleBinCapacityGuard::evaluate`, the changed second query gives `InvalidData` and `Verdict::Unknown`: `changed_second_query_blocks_the_capacity_guard_through_the_settings_source`
- an Unknown verdict stops the Shell: `recycle_bin_facade.rs` `send_to_recycle_bin_refuses_*_without_shell` (existing).

Run 2026-11-03: fmt check, clippy --tests -D warnings and the 31 tests of the binary pass (see the report).

## Proposal rows
None. No test needed a behaviour change.

## Proposed wording for the owner (files not edited here)
- BACKLOG row G30: status "done, PR #532, tests verified 2026-11-03".
- PARITY P-03: nothing to add; the st-parity-sync9 note already records the fix.
