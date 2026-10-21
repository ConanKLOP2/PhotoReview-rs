//! Port of `PhotoReview.Shell.Win32/Overlay/OverlayHost.cs` (101 lines, baseline `d92d6aae`). Unit W8-15f.
//!
//! The container of the overlay's root elements in z order (later = on top): layout, drawing, hit testing (hidden and zero-opacity elements are
//! skipped, so a click falls through to the image) and id lookup for the animator. The elements are `pr_app::overlay` `ElementNode`s (W8-15a); the host owns
//! them as `Box<dyn ElementNode>` (contract W8-15a, "15f holds `Box<dyn ElementNode>`").
//!
//! Rust shape (contract `contracts/W8-15f.md`): the C# `Resolve(id) -> IOpacityTarget` that the `Animator` called becomes [`OverlayHost::opacity_store`], an
//! `OpacityStore` over the ROOTS (the only elements the overlay animates: `ToolbarPanel` and `InfoOverlayHost`). `Find` still walks the whole tree.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use std::fmt;

use pr_app::input::primitives::PointD;
use pr_app::overlay::animator::OpacityStore;
use pr_app::overlay::contracts::OverlayLayoutContext;
use pr_app::overlay::element::ElementNode;
use pr_app::services::render::DrawContext;

/// `Add` failures (C# `ArgumentException` for a duplicate id, `ObjectDisposedException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayHostError {
    /// An element with the id of the new root already exists in some tree (C# message "An overlay element with id '...' already exists.").
    DuplicateId(String),
    /// The host was disposed.
    Disposed,
}

impl fmt::Display for OverlayHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for OverlayHostError {}

/// C# `OverlayHost : IDisposable`.
#[derive(Debug, Default)]
pub struct OverlayHost {
    /// C# `_roots`: z order, last = topmost.
    roots: Vec<Box<dyn ElementNode>>,
    /// C# `_disposed`.
    disposed: bool,
}

impl OverlayHost {
    /// C# constructor: no roots.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Roots`, z order (last = top).
    pub fn roots(&self) -> &[Box<dyn ElementNode>] {
        todo!()
    }

    /// C# `Add(root)`: `Disposed` after `dispose`; `DuplicateId` when `find(root id)` finds something in the existing trees (only the NEW root's own id is
    /// checked, not its descendants, as C#); then appended on top.
    pub fn add(&mut self, root: Box<dyn ElementNode>) -> Result<(), OverlayHostError> {
        todo!()
    }

    /// C# `Remove(root)` by id (Rust has no reference identity): the root with that id, or `None`. Allowed after `dispose` (C# did not throw).
    pub fn remove(&mut self, root_id: &str) -> Option<Box<dyn ElementNode>> {
        todo!()
    }

    /// C# `Find(id)`: the first element with that id in any tree, roots in z order, each tree depth first (`ElementNode::find`).
    pub fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// Typed read access to a ROOT by id (downcast through `ElementNode::as_any`); `None` when the id is missing or the type differs.
    pub fn root<T: ElementNode + 'static>(&self, id: &str) -> Option<&T> {
        todo!()
    }

    /// Typed write access to a ROOT by id (`ElementNode::as_any_mut`).
    pub fn root_mut<T: ElementNode + 'static>(&mut self, id: &str) -> Option<&mut T> {
        todo!()
    }

    /// C# `Resolve(id)`, for the animator: an [`OpacityStore`] over the roots. Opacity of a non-root id reads as missing (see the module doc).
    pub fn opacity_store(&mut self) -> HostOpacityStore<'_> {
        todo!()
    }

    /// C# `Arrange(context)`: every root in z order (`OverlayElement::arrange`: measure in the whole client, then place).
    pub fn arrange(&mut self, context: &OverlayLayoutContext<'_>) {
        todo!()
    }

    /// C# `Render(dc)`: every root in z order (the toolbar, added last, is drawn last).
    pub fn render(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    /// C# `HitTest(point)`: roots from the topmost, the first `find_at` hit (deepest element); `None` = the click goes to the image.
    pub fn hit_test(&self, point: PointD) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// C# `Dispose()`: `release_resources()` on every root (text layout leases go back), then clears the roots. Idempotent.
    pub fn dispose(&mut self) {
        todo!()
    }

    /// True after `dispose`.
    pub fn is_disposed(&self) -> bool {
        todo!()
    }
}

/// `OpacityStore` over the roots of an [`OverlayHost`] (the Rust twin of the C# `Func<string, IOpacityTarget?>` resolver the animator was given).
#[derive(Debug)]
pub struct HostOpacityStore<'a> {
    roots: &'a mut [Box<dyn ElementNode>],
}

impl OpacityStore for HostOpacityStore<'_> {
    fn opacity(&self, element_id: &str) -> Option<f32> {
        todo!()
    }

    fn set_opacity(&mut self, element_id: &str, opacity: f32) {
        todo!()
    }
}
