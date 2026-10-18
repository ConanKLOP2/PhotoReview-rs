//! Port of `PhotoReview.Benchmarking/SlowLinkSourceReader.cs` (C# baseline d92d6aae, 35 lines), W7-02b.
//!
//! Q-R29 option C-2 measurement harness, NOT a production default: a decorator over a [`SourceReader`] that meters
//! every byte read through a [`SharedBandwidthLimiter`] shared by all readers of one simulated link. `priority` is
//! passed through to the inner reader and otherwise ignored (the decorator is priority-blind, like the C# one).
//!
//! Depends on W7-02a (`crate::slow_link_fs::{SharedBandwidthLimiter, ThrottledReadStream}`), which must be merged
//! (or at least stubbed at that path) before this file compiles.
#![allow(dead_code, unused_variables)] // stub: the porter removes this line

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};

use crate::slow_link_fs::{SharedBandwidthLimiter, ThrottledReadStream};

/// C# `SlowLinkSourceReader`. The C# `ArgumentNullException` for a null `inner` has no Rust twin (the type forbids it).
pub struct SlowLinkSourceReader {
    inner: Arc<dyn SourceReader>,
    bandwidth: Option<Arc<SharedBandwidthLimiter>>,
}

impl SlowLinkSourceReader {
    /// `bandwidth == None` means a pure pass-through (no wrapper stream at all).
    pub fn new(inner: Arc<dyn SourceReader>, bandwidth: Option<Arc<SharedBandwidthLimiter>>) -> Self {
        Self { inner, bandwidth }
    }
}

impl fmt::Debug for SlowLinkSourceReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlowLinkSourceReader")
            .field("throttled", &self.bandwidth.is_some())
            .finish_non_exhaustive()
    }
}

impl SourceReader for SlowLinkSourceReader {
    /// Opens through `inner` with the SAME `path`, `priority` and `buffer_size`; an inner error is returned unchanged
    /// (no limiter use). With a limiter the stream is wrapped in [`ThrottledReadStream`], otherwise returned as is.
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        let _ = ThrottledReadStream::new;
        todo!("W7-02b: inner.open_source(path, priority, buffer_size)?, then wrap when bandwidth is Some")
    }
}
