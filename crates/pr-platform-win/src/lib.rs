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

/// Phase 0 UI spike (R0001): Win32 window + Direct2D/DirectWrite host and a WIC JPEG writer for synthetic fixtures.
#[cfg(all(windows, feature = "ui-spike"))]
pub mod spike_ui;

/// Test-only Win32 process memory metrics used by native decoder parity tests.
#[cfg(all(windows, feature = "test-support"))]
pub mod test_support;

/// Wave 8 viewing adapters that need no `pr-app` type (W8-16a/16e stubs; `pr-app` depends on this crate).
pub mod shell_viewing;
