Status: evidence report (2026-11-19). Re-run of the 41 survivors of the `pr-platform-win` P1 files (`instance_keys.rs`, `live_operation_registry.rs`, `natural_comparer.rs`) after adding killers. Measured on one machine (Windows 11, interactive session 1), `--jobs 1`, `MUT_TIMEOUT=600`, `--tests p1k_instance_keys_killers,p1k_live_registry_killers`, name filter on the touched functions, `--skip for_tests`.

# `pr-platform-win` P1: mutation verdict after killers

## Result

61 identities tested (the 41 old survivors plus the new mutants that the seams and the extracted functions created, minus the ten mutants of the `*_for_tests` wrappers, which were skipped because every killer asserts through them): 45 caught, 2 unviable, 2 timeouts (infinite loop of `+=` -> `*=`, detected), 12 missed. All 12 missed ones are among the old 41 survivors, so 29 of the 41 are now killed or detected and 12 stay open, all argued below.

## Product change (test seams, no behaviour change, P-409)

- `instance_keys.rs`: `#[doc(hidden)] read_user_sid_for_tests`, `read_session_id_for_tests`.
- `live_operation_registry.rs`: the `is_live` error classification is a method `open_failure_is_live` (same three outcomes); `#[doc(hidden)]` `open_failure_is_live_for_tests`, `event_name_for_tests`, `current_user_sid_for_tests`.
- `natural_comparer.rs`: the "missing shlwapi -> managed fallback, any other load error panics" decision is the `#[doc(hidden)] pub fn fallback_for_load_error`.

New tests: `crates/pr-platform-win/tests/p1k_instance_keys_killers.rs` (3) and `p1k_live_registry_killers.rs` (9). They use real named events with a unique prefix, real token reads and `GetProcessHandleCount`; no desktop.

## Killed

| Mutants | Killer |
|---|---|
| `live_operation_registry` `is_live` -> true/false, `EventMarker::drop` -> (), `name_for` -> "" / "xyzzy" | `marker_round_trip_is_live_until_dropped`, `event_name_is_local_prefixed_sid_bound_and_what_begin_creates` (raw `OpenEventW` on the independently computed `Local\` name) |
| `current_user_sid` (both modules) -> "" / "xyzzy" / `Ok("")`, `<` -> `>`, `!=` -> `==`, `+=` -> `-=` | `user_sid_reader_returns_a_real_sid`, `current_user_sid_is_the_process_sid` (starts with `S-1-5-`, equals the other reader) |
| `Token::drop` / `OwnedHandle::drop` -> () | `*_closes_its_token_handle` (400 reads, process handle count may not grow by 100) |
| `Debug::fmt` -> Ok(Default) | `debug_format_names_the_type_and_prefix` |
| `report` -> (), match guards at `is_live`, `win32_error_code` (-> 0/1, `&` -> `\|`/`^`) in both modules | `open_failure_classification`, `begin_failure_is_logged_and_not_live` (40 000 character prefix makes `CreateEventW` fail), `load_error_fallback_only_for_missing_module` |
| `native_compare` -> None | `native_comparer_uses_shell_order_not_managed_fallback` (`a-1`/`a1`, `a_1`/`a1`, `IMG_1.JPG`/`img_1.jpg` differ between shell and managed order) |
| `+=` -> `*=` in both SID readers | timeout (length never grows, endless loop): detected |

## Open (12), all argued

| Mutant | Verdict |
|---|---|
| `instance_keys.rs` `delete match arm Component::CurDir` / `ParentDir` in `absolute_lexical_path` | equivalent. `std::path::absolute` resolves `.`/`..` for ordinary paths, and for `\?\` paths `components()` yields them but `PathBuf::push` of a verbatim path pops on `..` and ignores `.` itself, so the fall-through arm produces the same string. Measured: `\?\C:\Photos\sub\..` and the arm-disabled build both give `\?\C:\PHOTOS`. |
| `instance_keys.rs:131` and `live_operation_registry.rs:169`, `<` -> `==` / `<=` in the size-query guard | equivalent. The first `GetTokenInformation` call returns `required = sizeof(TOKEN_USER) + SID length`, which is always strictly greater than `sizeof(TOKEN_USER)`; the branch is reachable only if the OS misreports a size, which cannot be provoked. |
| `instance_keys.rs:153:71` `\|\|` -> `&&` after `ConvertSidToStringSidW` | equivalent. When the call succeeds `is_err()` is false and the pointer is non-null, so both operators continue; the failure branch needs an OS failure with a non-null output. |
| `instance_keys.rs:172` (two) and `:190` | equivalent on Windows: they sit in the `#[cfg(not(windows))]` twins, which are not compiled here. |
| `instance_keys.rs:29` `current_session_id` -> 1, `:177` `read_session_id` -> 1 | not killable on this machine: the interactive user is in session 1, so the mutant returns the true value. `session_id_matches_independent_win32_read` compares with an independent `ProcessIdToSessionId` and kills both wherever the session is not 1 (service, session 0, second RDP user). Logged in P-409; the `-> 0` twins are caught. |

## Notes

- Existing quirk, not changed: for `\?\C:\...` input the first lexical step pushes the verbatim prefix and `PathBuf` keeps it, so the key keeps the `\?\` spelling; it still differs from the plain `C:\...` key (C# `Path.GetFullPath` also leaves verbatim paths alone).
- A first full run hit the 1200 s default budget after 7 identities; this report is from the second run with `--budget 30000`.
