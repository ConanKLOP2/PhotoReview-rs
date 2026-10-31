# FIX-SHELL-LOW (2026-10-31)

> Status: lane `fix-shell-low`. Source: `docs/reviews/ST-SHELL-REVIEW-2026-10-31.md`, low findings L1..L8. Each item was checked against the code and the C# baseline `7eaee2a2`.

## Verdict per item

| Item | Verdict | What changed or why not |
|---|---|---|
| L1 hook installed after warm-up | Fixed | `ShellStartup::run` now calls `install_process_hooks` before the two warm-up threads (C# installs the hook even later, in `RunStartupGuardedAsync`, so this is stricter than C#). The unused `log` parameter stays (public signature, used by tests) with a comment. No new test: the order is only observable in the native `run` path. |
| L2 lone surrogate in a file name | Deferred (design) | Keeping an `OsString` up to `ShellLaunch` changes `ShellLaunch::parse`, `existing_paths` forwarding and the pipe protocol. Not a small fix; a file name with a lone surrogate is not creatable from Explorer. The existing test that pins `U+FFFD` stays. Added the missing tests for `\\?\C:\Ảnh\x.jpg`, `\\server\share\Ảnh\x.jpg` and a UNC folder at `ShellLaunch::parse` level (`pr-app/tests/startup_launch_long_paths.rs`). |
| L3 scattered `PHOTOREVIEW_RS_*` rules | Partly fixed, rest intentional | `PHOTOREVIEW_RS_DATA_ROOT` is read with `var_os`: a non-Unicode value now redirects the data (C# `GetEnvironmentVariable` returns the string with the surrogate and uses it) instead of silently falling back to the real `%LOCALAPPDATA%` (`pr-core/src/app_paths.rs`, tests `resolve_override_os_*`). `WARP == "1"` and `NOWARM != "1"` are exact C# parity (`ShellStartup.cs:57,63`), so `WARP=true` having no effect is intended; `ISOLATE_CONFIG` accepting `true` is the C# `IsTruthy`. `WindowsLiveOperationRegistry` uses the default prefix in C# too (`SharedServiceRegistration.cs:113`, no prefix argument), so it is parity. One shared helper was not introduced: the rules differ on purpose per C# variable. |
| L4 panic on registry `Err` | Fixed for the decoder factory; `start_viewer_preview` kept | `LazyDecoderFactory::resolve` no longer panics: `factory_or_unavailable` logs the error and returns an `UnavailableDecoderFactory` (nothing registered, decode fails with `DecodeError::NotSupported`). `start_viewer_preview` keeps its panic: its `key` comes from `current_cache_key`, which already answered `Err` for the same memoized `OnceLock` result, and `PreviewHandle::ready` is `pub(crate)` in `pr-imaging`, so a typed failure needs a new `pr-imaging` API (deferred). |
| L5 Settings/Recovery creation errors | Settings fixed, Recovery intentional | A failed `SettingsDialog::new` is now `AppLog::error_forced` (C# let the exception reach the dispatcher handler, which logs forced). No test: forcing `SettingsDialog::new` to fail needs a seam that does not exist. Recovery already shows the user a warning through `try_read_recovery_entries` (`show_error` callback) and logs; unchanged. |
| L6 `borrow()` in four overlay commands | Fixed | `show_skipped_files/recovery/diagnostics/benchmark` use `try_borrow` through `with_shared_view_model`; a busy view model drops the request with a warning. Native test `overlay_vm_read_only_dialogs_are_ignored_while_the_view_model_is_borrowed_mutably` (`#[ignore]`, needs a desktop). |
| L7 ShowWindow before the view model | Recorded, no change | Intentional order (comment `window.rs:5-7`); the review found no leak and `forced_exit=1` wins at shutdown. |
| L8 `localization()` relative path | Fixed | `shipped_languages_dir` falls back to the directory of `argv[0]` with a warning when `current_exe()` fails; with neither it keeps the relative `Languages` as before. Tests in `pr-shell/tests/fix_shell_low.rs`. |

## Tests

See the PR description for what was run. Not run: workspace clippy/tests, mutation testing, native tests (limits of this lane).

## PARITY

No PARITY row changes proposed. L2 stays a known `Different` candidate (lone surrogate in arguments) if the owner wants it recorded.
