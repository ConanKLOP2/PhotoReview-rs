// Each integration test imports one half of this shared fixture, leaving the other half unused.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use pr_app::services::render::RenderError;
use pr_imaging::pixels::PixelBuffer;
use pr_platform_win::host::contracts::WindowMessageHandler;
use pr_platform_win::shell_viewing::keep_warm::KeepWarmWindow;
use pr_platform_win::shell_viewing::prefetcher::{LookupRequest, PrefetchCache, PrefetchHost, RetryRequest};
use std::sync::Arc;

#[derive(Default)]
pub struct FakePrefetchCache {
    pub contained: RefCell<HashSet<i64>>,
    pub prefetched: RefCell<Vec<i64>>,
    pub fail: Cell<bool>,
}
impl PrefetchCache for FakePrefetchCache {
    fn contains(&self, id: i64) -> bool {
        self.contained.borrow().contains(&id)
    }
    fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        if self.fail.get() {
            return Err(RenderError::Disposed { object: "fake" });
        }
        self.prefetched.borrow_mut().push(pixels.id());
        self.contained.borrow_mut().insert(pixels.id());
        Ok(())
    }
}

#[derive(Default)]
pub struct FakePrefetchHost {
    pub lookups: RefCell<Vec<LookupRequest>>,
    pub retries: RefCell<Vec<(Duration, RetryRequest)>>,
    pub no_paths: Cell<bool>,
}
impl PrefetchHost for FakePrefetchHost {
    fn path_at(&self, index: i32) -> Option<String> {
        (!self.no_paths.get()).then(|| format!("p{index}"))
    }
    fn start_lookup(&self, request: LookupRequest) {
        self.lookups.borrow_mut().push(request);
    }
    fn schedule_retry(&self, delay: Duration, request: RetryRequest) {
        self.retries.borrow_mut().push((delay, request));
    }
}

#[derive(Default)]
pub struct FakeKeepWarmWindow {
    pub active: Cell<bool>,
    pub handlers: RefCell<Vec<Rc<dyn WindowMessageHandler>>>,
}
impl KeepWarmWindow for FakeKeepWarmWindow {
    fn is_active(&self) -> bool {
        self.active.get()
    }
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.handlers.borrow_mut().insert(0, handler);
    }
}
