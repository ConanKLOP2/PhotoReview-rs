//! Port target: `PhotoReview.Core` (catalog, settings, session, file actions, journal, abstractions).
//! Must not depend on any UI crate (checked by the architecture test once `pr-app` exists).

pub mod app_paths;
pub mod identity;
