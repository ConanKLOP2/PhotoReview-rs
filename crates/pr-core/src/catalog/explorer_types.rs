//! Port of `Catalog/ExplorerOrderTypes.cs` (moved into Wave 0: the [`ExplorerOrderProvider`] trait needs it).
//!
//! [`ExplorerOrderProvider`]: crate::abstractions::ExplorerOrderProvider

use crate::time::DateTimeUtc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerOrderStatus {
    Available,
    NoMatchingWindow,
    NativeViewUnavailable,
    InvalidSnapshot,
    TimedOut,
    Canceled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerSortDirection {
    Unknown,
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerGroupState {
    None,
    Active,
    Unknown,
}

/// A COM GUID in .NET `Guid.ToByteArray()` byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Guid(pub [u8; 16]);

/// One sort column of the Explorer view (`PROPERTYKEY` + direction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerSortColumn {
    pub property_set: Guid,
    pub property_id: u32,
    pub direction: ExplorerSortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerQueryProgress {
    pub items_read: i32,
    pub item_count: i32,
    pub com_calls: i32,
}

/// What an Explorer query returned. Paths are `String`s like every catalog/journal record (contracts/README.md D1).
/// `reason` uses the `ExplorerReason` wire form (`code|detail|...`, contracts/W1-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerViewSnapshot {
    pub folder: String,
    pub ordered_paths: Vec<String>,
    pub sort_columns: Vec<ExplorerSortColumn>,
    pub group_state: ExplorerGroupState,
    pub status: ExplorerOrderStatus,
    pub reason: Option<String>,
    pub captured_utc: DateTimeUtc,
}
