//! Port target: `PhotoReview.PerfAnalysis` (W1-16..21). Order: csv -> nav -> stats -> rules -> report <-> analyze.

pub mod analyze;
pub mod csv;
pub mod nav;
pub mod report;
pub mod rules;
pub mod stats;
