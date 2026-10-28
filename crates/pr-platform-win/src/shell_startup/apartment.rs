//! The COM apartment of the process's main thread: the Rust twin of the `[STAThread]` attribute on `Program.Main` (`PhotoReview.Shell.Win32/Program.cs` line 14, baseline `d92d6aae`).
//! Unit W8-20a. Contract: `contracts/W8-20a.md`.
//!
//! Why it exists: the UI thread registers the drop target (`RegisterDragDrop`, `shell_input::drop_target`) and shows the Shell file/folder dialogs, which need an STA. A .NET `Main` marked
//! `[STAThread]` gets one from the CLR; a Rust `main` gets none, and the first library call that wants COM would put the thread in the MTA (`wic::interop::ensure_com_initialized`), after
//! which `OleInitialize` fails with `RPC_E_CHANGED_MODE` and the drop target never registers. So `pr_shell::entry::main_entry` enters this apartment FIRST.

use std::fmt;
use std::marker::PhantomData;

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

/// Why the apartment could not be entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApartmentError {
    /// `RPC_E_CHANGED_MODE` (0x80010106): the thread is already in the multithreaded apartment. Cannot happen at the start of `main`; a test thread that used WIC first can see it.
    ChangedMode,
    /// Any other failing `HRESULT` of `CoInitializeEx`.
    Failed(i32),
}

impl fmt::Display for ApartmentError {
    /// `ChangedMode`: "the thread is already in the multithreaded COM apartment"; `Failed(hr)`: "CoInitializeEx failed (HRESULT 0x{hr:08X})".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApartmentError::ChangedMode => f.write_str("the thread is already in the multithreaded COM apartment"),
            ApartmentError::Failed(hr) => write!(f, "CoInitializeEx failed (HRESULT 0x{:08X})", *hr as u32),
        }
    }
}

impl std::error::Error for ApartmentError {}

/// RAII guard of one successful `CoInitializeEx(COINIT_APARTMENTTHREADED)` on THIS thread; `Drop` is the balancing `CoUninitialize`. `!Send` and `!Sync` (the apartment belongs to the thread
/// that entered it).
#[derive(Debug)]
pub struct StaApartment {
    not_send: PhantomData<*const ()>,
}

impl StaApartment {
    /// `CoInitializeEx(None, COINIT_APARTMENTTHREADED)`: `S_OK` and `S_FALSE` (already in an STA, the count is incremented) both give a guard; `RPC_E_CHANGED_MODE` is
    /// `Err(ApartmentError::ChangedMode)`; any other failure is `Err(Failed(hr))`. The only `unsafe` is the call itself (`// SAFETY:` says: no pointer is passed, the call only changes this
    /// thread's apartment state, and the guard balances it on the same thread).
    pub fn enter() -> Result<StaApartment, ApartmentError> {
        // SAFETY: no pointer is passed (reserved = NULL); the call only changes this thread's apartment state, and the guard balances it with `CoUninitialize` on the same thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.0 >= 0 {
            Ok(StaApartment { not_send: PhantomData })
        } else if hr == RPC_E_CHANGED_MODE {
            Err(ApartmentError::ChangedMode)
        } else {
            Err(ApartmentError::Failed(hr.0))
        }
    }
}

impl Drop for StaApartment {
    /// `CoUninitialize()` once, on the thread that entered (the guard is `!Send`).
    fn drop(&mut self) {
        // SAFETY: balances the successful (S_OK/S_FALSE) `CoInitializeEx` of `enter` on this thread; the guard is `!Send`, so this runs on the thread that entered.
        unsafe { CoUninitialize() };
    }
}
