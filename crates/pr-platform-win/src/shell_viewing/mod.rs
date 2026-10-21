//! `pr_platform_win::shell_viewing`: viewing adapters that need no `pr-app` type (`pr-app` depends on this crate, so anything
//! that needs `pr_app` types lives in `pr_app::viewing`; lead decision D-W8B1-1). Lead-owned index.

pub mod compare;
pub mod composer;
pub mod crossfade;
pub mod frame_log;
pub mod frame_stats;
pub mod keep_warm;
pub mod pacer;
pub mod prefetcher;
pub mod presentation_sink;
pub mod scrollbar_native;
pub mod scrollbars;
pub mod viewport_controller;
