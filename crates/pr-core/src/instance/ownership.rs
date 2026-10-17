//! Port of `PhotoReview.Core/Instance/FolderOwnership.cs` (C# baseline d92d6aae, 29 lines): what a window does with a
//! folder-open request once the instance ownership has been checked (Q-R18). The implementation
//! (`InstanceScope`: mutex + forward pipe per folder) is Wave 6 (`pr-platform-win`); this unit is the contract.
//!
//! Unit W2-19. Complete (data and trait only). Contract: `contracts/W2-19.md`.

use crate::sync::{CancelToken, Canceled};

/// Failure while checking ownership before a folder open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderOwnershipError {
    /// The pending ownership operation was canceled.
    Canceled,
    /// The requested folder is empty or whitespace-only (C# `ThrowIfNullOrWhiteSpace`).
    InvalidFolder,
}

impl From<Canceled> for FolderOwnershipError {
    fn from(_: Canceled) -> Self {
        Self::Canceled
    }
}

impl std::fmt::Display for FolderOwnershipError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canceled => formatter.write_str("operation canceled"),
            Self::InvalidFolder => formatter.write_str("folder cannot be empty or whitespace"),
        }
    }
}

impl std::error::Error for FolderOwnershipError {}

/// Q-R18: what a window does with a folder-open request once the instance ownership has been checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderOpenDecision {
    /// This window may load the folder (it holds, or has just taken, the folder's instance lock).
    Proceed,
    /// Another instance owns the folder and accepted the request (or very likely did, Q-R12); this window keeps its
    /// folder.
    ForwardedToOtherInstance,
    /// Another instance owns the folder but did not take the request; this window keeps its folder.
    OwnedByOtherInstance,
}

/// Keeps the single-instance lock and the forward pipe in step with the folder a window shows (PerFolder mode; in
/// SingleWindow mode every folder maps to the one app-wide lock, so every open proceeds).
///
/// Contract: [`after_open`](Self::after_open) is called exactly once for every [`before_open`](Self::before_open) that
/// returned [`FolderOpenDecision::Proceed`], whether the load succeeded, failed or was superseded.
/// C# `IFolderOwnership`.
pub trait FolderOwnership: Send + Sync {
    fn before_open(
        &self,
        folder: &str,
        initial_path: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<FolderOpenDecision, FolderOwnershipError>;

    /// The window now shows `folder`: locks of folders it no longer shows (and no open is pending for) are released.
    fn on_folder_shown(&self, folder: &str);

    /// The open of `folder` finished; `shown_folder` is what the window shows now (`None`: nothing yet).
    fn after_open(&self, folder: &str, shown_folder: Option<&str>);
}
