//! Port target: `PhotoReview.Core` (catalog, settings, session, file actions, journal, abstractions).
//! Must not depend on any UI crate (checked by the architecture test once `pr-app` exists).
//!
//! Module declarations are owned by the lead (docs/MULTI-AGENT-PLAN.md §2.1): a porter only fills files its contract
//! lists.

pub mod abstractions;
pub mod app_paths;
pub mod catalog;
pub mod identity;
pub mod sync;
pub mod text;
pub mod time;
