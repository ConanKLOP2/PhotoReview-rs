//! Port targets: `PhotoReview.Platform.Windows` (Explorer order: INV-7/INV-9; Recycle Bin: INV-6;
//! W6-05: `WindowsMemoryProbe`, `WindowsNaturalComparer`, `WindowsDarkTitleBar`,
//! `WindowsLiveOperationRegistry`).

#[cfg(windows)]
pub(crate) mod explorer_com_interop;

#[cfg(windows)]
#[allow(dead_code)] // W6-01d consumes the native query helpers once lead-owned wiring lands.
pub(crate) mod explorer_native_view;

#[cfg(windows)]
#[allow(dead_code)] // W6-01d consumes the selector policy in the native query path.
pub(crate) mod explorer_window_selector;

#[cfg(windows)]
pub(crate) mod instance_keys;

#[cfg(windows)]
pub(crate) mod forward_pipe;

#[cfg(windows)]
pub mod forward_server;

#[cfg(windows)]
pub mod forward_client;

#[cfg(windows)]
pub mod http;

#[cfg(windows)]
pub mod instance_scope;

#[cfg(windows)]
pub mod recycle_bin;

#[cfg(windows)]
#[allow(dead_code)] // Failure-kind policy is consumed by the Recycle Bin shell adapter wiring.
mod recycle_candidate;

#[cfg(windows)]
mod recycle_bin_shell;

#[cfg(windows)]
pub mod restore_verb;

#[cfg(windows)]
pub mod recycle_restore;

#[cfg(windows)]
pub mod recycle_bin_settings;

#[cfg(windows)]
pub mod recycle_bin_capacity;

#[cfg(windows)]
pub mod recycle_eligibility;

#[cfg(windows)]
#[allow(dead_code)] // The crate-private provider awaits registration by the application composition root.
pub(crate) mod explorer_order_service;

#[cfg(windows)]
pub mod wic;

#[cfg(windows)]
pub mod dark_title_bar;

#[cfg(windows)]
pub mod live_operation_registry;

#[cfg(windows)]
pub mod memory_probe;

#[cfg(windows)]
pub mod file_mode;

/// W8-09b slice S2 (`contracts/W8-09b.md`, D-W8B6-151): production adapters the shared service graph needs and no merged unit implements.
#[cfg(windows)]
pub mod culture;
#[cfg(windows)]
pub mod explorer_order_provider;
#[cfg(windows)]
pub mod physical_memory;
#[cfg(windows)]
pub mod shell_integration;

#[cfg(windows)]
pub mod natural_comparer;

#[cfg(windows)]
pub(crate) mod vblank_driver;

#[cfg(windows)]
pub mod display_clock;

/// Wave 8 graphics interop helpers over `windows-rs` (D3D11, DXGI, Direct2D, DirectWrite; W8-11f..11h stubs).
#[cfg(windows)]
pub mod gfx;

#[cfg(windows)]
pub mod render;

/// Phase 0 UI spike (R0001): Win32 window + Direct2D/DirectWrite host and a WIC JPEG writer for synthetic fixtures.
#[cfg(all(windows, feature = "ui-spike"))]
pub mod spike_ui;

/// Test-only Win32 process memory metrics used by native decoder parity tests.
#[cfg(all(windows, feature = "test-support"))]
pub mod test_support;

/// Wave 8 viewing adapters that need no `pr-app` type (W8-16a/16e stubs; `pr-app` depends on this crate).
pub mod shell_viewing;

/// Wave 8 Win32 foundation (W8-11a to W8-11e stubs): constants, struct layouts and thin wrappers that replace `PhotoReview.Shell.Interop`.
#[cfg(windows)]
pub mod win32;

#[cfg(windows)]
pub mod host;

#[cfg(windows)]
pub mod shell_input;

#[cfg(windows)]
pub mod dialogs;

#[cfg(windows)]
pub mod shell_menus;

/// Wave 8 batch B4 overlay Win32 parts (W8-15f to 15j stubs): host, message handler, tooltips, `ShellOverlay` (layout, hit test, input, paint), main window glue.
#[cfg(windows)]
pub mod shell_overlay;

/// Wave 8 batch B6 native start-up parts (W8-17a DPI, W8-17b placement and cloak, W8-17c warm-ups): see `shell_startup/mod.rs`.
#[cfg(windows)]
pub mod shell_startup;

/// Wave 8 batch B6 test drivers (W8-20a/20b stubs): a reader of the resources of a built exe and a poster of input messages to a window of the SAME machine. Only
/// `pr-shell`'s integration tests use them (`features = ["test-support"]` in its dev-dependencies); the unsafe code stays in this crate (AGENTS.md).
#[cfg(all(windows, feature = "test-support"))]
pub mod exe_resources;
#[cfg(all(windows, feature = "test-support"))]
pub mod ui_driver;
