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
#[allow(dead_code)] // W6-03c consumes mutex-name generation and the default prefix.
pub(crate) mod instance_keys;

#[cfg(windows)]
#[allow(dead_code)] // W6-03c/d consume the pipe identity and ACL construction API.
pub(crate) mod forward_pipe;

#[cfg(windows)]
pub mod forward_server;

#[cfg(windows)]
pub mod forward_client;

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

<<<<<<< HEAD
/// Wave 8 batch B4 overlay Win32 parts (W8-15f to 15j stubs): host, message handler, tooltips, `ShellOverlay` (layout, hit test, input, paint), main window glue.
#[cfg(windows)]
pub mod shell_overlay;
=======
#[cfg(windows)]
pub mod shell_input;

#[cfg(windows)]
pub mod shell_menus;
>>>>>>> 07fd480d92a534da4b0d3efb6ad721bcb5db4a6b
