//! `pr_platform_win::win32`: the Win32 foundation that replaces `PhotoReview.Shell.Interop` (`Win32/*.cs`, `Shell/*.cs`; baseline `d92d6aae`).
//! Lead-owned index (D-W8B1-4). The `windows` crate generates the P/Invoke and COM declarations, so these modules hold only the custom
//! constants, the layout aliases and the thin wrappers the hosts call (batch B3, W8-11a to W8-11e; the `gfx` group W8-11f to 11h is a
//! separate module). Handles cross this module as `isize` (`structs::Hwnd` and friends), never as `windows::Win32::Foundation::HWND`.

pub mod activation_context;
pub mod api;
pub mod comctl;
pub mod control_constants;
pub mod dialog_chrome;
pub mod dialog_native;
pub mod key_capture;
pub mod messages;
pub mod settings_native;
pub mod shell_native;
pub mod startup;
pub mod structs;
