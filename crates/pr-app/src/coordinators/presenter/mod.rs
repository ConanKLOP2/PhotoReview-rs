//! `ImagePresenter` (C# `Coordinators/ImagePresenter.cs`, 949 lines), split over the units W8-07h to W8-07k, one file per slice:
//!
//! | file | unit | C# lines |
//! |---|---|---|
//! | `state` | W8-07h | 1 to 232, 800 to 832, 932: the struct, constructor, properties, `UpdateCurrentImage`, `UpdateStatus`, the pump |
//! | `zoom` | W8-07h | `ShowZoomDetailImage`, `SetViewerZoom`, the zoom-detail glue |
//! | `present_a` | W8-07i | 222 to 511: `PresentAsync`, steps 1 to 5 of `PresentCoreAsync` |
//! | `present_b` | W8-07j | 500 to 712: steps 6 and 7, the `catch` clauses, the compare branch |
//! | `missing` | W8-07k | 714 to 770: `RemoveOrDegrade`, `RemoveMissingCatalogItemAsync` |
//! | `compare_pair` | W8-07k | 772 to 791, 865 to 884: the pair cache, the compare preview load |
//! | `photo_info` | W8-07k | 833 to 863, 886 to 949: stat, original dimensions, `PhotoInfo` |
//!
//! Lead-owned wiring: `coordinators/mod.rs` gets `pub mod presenter;` and `pub mod zoom_detail;`.

pub mod compare_pair;
pub mod missing;
pub mod photo_info;
pub mod present_a;
pub mod present_b;
pub mod rename;
pub mod state;
pub mod zoom;

pub use photo_info::PhotoInfo;
pub use state::{
    ImageChange, ImagePresenter, ImagePresenterDeps, PresentError, PresentRequest, PresenterChanges, PresenterEnv,
};
