# Wave 10: manual update HTTP lane (contracts only)

Baseline `d92d6aae`; `HttpTransport` and presenter logic are already merged. This plan reserves three implementation units; its stubs do not perform network I/O. `F` means fake/in-process socket tests, `N` means Windows desktop or network environment verification. No test contacts the public internet.

| WU | C# files (lines) | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W10-H1 | `Core/Updates/UpdateChecker.cs` 126 (transport policy subset) | `pr_platform_win::http::policy` | 🟡 | Sonnet | F | code: W2-18 `pr_core::updates::checker` | H1 |
| W10-H2 | `Core/Updates/{UpdateChecker 126, IUpdateChecker 10, UpdateCheckResult 26}` = 162 (network adapter subset) | `pr_platform_win::http::transport` | 🟡 | Sonnet | F/N | plan: H1; code: W2-18 `HttpTransport`, W1-10 `updates::version` | H2 |
| W10-H3 | `App.Shared/Presenters/SettingsPresenter.cs` 561-635 (75); `Shell.Win32/Dialogs/SettingsUi/Pages/GeneralSettingsPage.cs` 183-197, 245-276 (47); `SettingsUi/SettingsDialog.cs` 119-130, 440-459 (32); `Dialogs/Native/Win32SecondaryWindowPort.cs` 103-114 (12) = 166 selected lines | `pr_shell::composition::update_http`, `pr_shell::composition::graph`, `pr_shell::startup::secondary_dialogs`; existing `pr_app::presenters::settings`, `pr_platform_win::dialogs::settings` | 🟡 | Sonnet | F/N | plan: H2; code: W8-09b graph, W8-18o/19g/19k/19m Settings, W2-18 checker | H3 |

Expected test counts and unit gates (run with the worktree's existing `CARGO_TARGET_DIR`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_INCREMENTAL=0`):

| Unit | Expected Rust tests | Required gate after implementation |
|---|---|---|
| W10-H1 | 4 F in `tests/http_policy.rs` | `cargo check --workspace --all-targets --locked`; `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`; `cargo fmt -p pr-platform-win --check`; `cargo test -p pr-platform-win --test http_policy --locked` |
| W10-H2 | 11 F + 1 ignored N in `tests/http_transport_loopback.rs` | Same workspace check and platform clippy/fmt; `cargo test -p pr-platform-win --test http_transport_loopback --locked` (N requires a separate `--include-ignored` desktop run) |
| W10-H3 | 8 F + 1 ignored N in `tests/update_http_composition.rs` | Same workspace check; `cargo clippy -p pr-shell --all-targets --locked -- -D warnings`; `cargo fmt -p pr-shell --check`; `cargo test -p pr-shell --test update_http_composition --locked` (N requires a separate desktop run) |

Dependency graph: `W2-18 + W1-10 → H1 → H2 → H3`; H3 also depends on merged Settings dialog and shell composition interfaces. Each row is a porting surface, not a claim that its C# lines are disjoint.

Risks: `new HttpClient()` on .NET 10 uses defaults that WinHTTP does not inherit automatically, especially environment proxy precedence, current-user PAC/WPAD, redirect policy, and a single ten-second deadline. A synchronous WinHTTP call cannot be canceled promptly merely by setting phase timeouts; H2 must use an asynchronously cancelable request handle. Header and decompression defaults need a local .NET 10 probe before claiming exact parity. Existing Settings and shell code still contains `todo!()` in this checkout, so compiling stubs prove interface shape only.

Stay **Partial** until a desktop/network run: system proxy/PAC and TLS trust behavior; cancellation during DNS, proxy discovery, TLS, redirects and body streaming; ShellExecute open-page behavior; settings dialog close during an active check. The F tests use loopback sockets and fake services only.
