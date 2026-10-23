//! Port of `ImagePresenter.RetargetPath` at C# 7eaee2a2, lines 225-295.
#![allow(unused_variables, dead_code)] // Stub-only: remove after Wave 9 implementation.
use super::state::{ImagePresenter, PresenterChanges};
use std::collections::HashMap;

impl ImagePresenter {
    /// C# `IsPresentInFlight`: a present has started and not yet finished (a superseded one counts until it notices).
    /// `retarget_path` asks the caller to present again when this is true.
    pub fn is_present_in_flight(&self) -> bool {
        todo!()
    }
    /// C# `RetargetPath(renames)`: re-key the presented and shown-file paths, photo info, status line name and the zoom-detail
    /// target (via `ZoomDetailLoader::retarget_path`, effects applied) without presenting, decoding or touching zoom/pan.
    /// Returns `(kept, changes)`: `kept` false when Compare was visible (cleared) or a present was in flight, so the caller
    /// must present the current entry again. An empty map only re-arms the zoom detail.
    pub fn retarget_path(&mut self, renames: &HashMap<String, String>) -> (bool, PresenterChanges) {
        todo!()
    }
}
