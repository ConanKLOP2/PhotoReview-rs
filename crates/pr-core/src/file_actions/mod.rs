//! Port target: `PhotoReview.Core/FileActions` (W1-09, W1-13, W1-14; services in Wave 3).

pub mod compaction;
pub mod destination_policy;
pub mod duplicates;
pub mod errors;
pub mod journal;
pub mod journal_line;
pub mod journal_rules;
pub mod partial_cleanup;
pub mod reconcile;
pub mod recovery_check;
pub mod recovery_retry;
pub mod service;
pub mod startup_recovery;
pub mod transaction;
pub mod types;
pub mod undo;
