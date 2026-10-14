//! Port target: `PhotoReview.Core` (catalog, settings, session, file actions, journal, abstractions).
//! Must not depend on any UI crate (checked by the architecture test once `pr-app` exists).
//!
//! Module declarations are owned by the lead (docs/MULTI-AGENT-PLAN.md §2.1): a porter only fills files its contract
//! lists.

pub mod abstractions;
pub mod app_paths;
pub mod caching;
pub mod catalog;
pub mod diagnostics;
pub mod display;
pub mod file_actions;
pub mod identity;
pub mod instance;
pub mod io;
pub mod model;
pub mod session;
pub mod settings;
pub mod sync;
pub mod text;
pub mod time;
pub mod updates;
