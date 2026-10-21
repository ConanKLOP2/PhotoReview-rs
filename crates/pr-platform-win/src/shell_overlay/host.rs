//! Port of `PhotoReview.Shell.Win32/Overlay/OverlayHost.cs` (101 lines, baseline `d92d6aae`). Unit W8-15f.
//!
//! The container of the overlay's root elements in z order (later = on top): layout, drawing, hit testing (hidden and zero-opacity elements are
//! skipped, so a click falls through to the image) and id lookup for the animator. The elements are `pr_app::overlay` `ElementNode`s (W8-15a); the host owns
//! them as `Box<dyn ElementNode>` (contract W8-15a, "15f holds `Box<dyn ElementNode>`").
//!
//! Rust shape (contract `contracts/W8-15f.md`): the C# `Resolve(id) -> IOpacityTarget` that the `Animator` called becomes [`OverlayHost::opacity_store`], an
//! `OpacityStore` over the ROOTS (the only elements the overlay animates: `ToolbarPanel` and `InfoOverlayHost`). `Find` still walks the whole tree.

use std::fmt;

use pr_app::input::primitives::PointD;
use pr_app::overlay::animator::OpacityStore;
use pr_app::overlay::contracts::OverlayLayoutContext;
use pr_app::overlay::element::ElementNode;
use pr_app::overlay::element::OverlayElement;
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
        match self {
            Self::DuplicateId(id) => write!(f, "An overlay element with id '{id}' already exists."),
            Self::Disposed => f.write_str("OverlayHost is disposed."),
        }
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
        Self::default()
    }

    /// C# `Roots`, z order (last = top).
    pub fn roots(&self) -> &[Box<dyn ElementNode>] {
        &self.roots
    }

    /// C# `Add(root)`: `Disposed` after `dispose`; `DuplicateId` when `find(root id)` finds something in the existing trees (only the NEW root's own id is
    /// checked, not its descendants, as C#); then appended on top.
    pub fn add(&mut self, root: Box<dyn ElementNode>) -> Result<(), OverlayHostError> {
        if self.disposed {
            return Err(OverlayHostError::Disposed);
        }
        let id = root.base().id();
        if self.find(id).is_some() {
            return Err(OverlayHostError::DuplicateId(id.to_owned()));
        }
        self.roots.push(root);
        Ok(())
    }

    /// C# `Remove(root)` by id (Rust has no reference identity): the root with that id, or `None`. Allowed after `dispose` (C# did not throw).
    pub fn remove(&mut self, root_id: &str) -> Option<Box<dyn ElementNode>> {
        let index = self.roots.iter().position(|root| root.base().id() == root_id)?;
        Some(self.roots.remove(index))
    }

    /// C# `Find(id)`: the first element with that id in any tree, roots in z order, each tree depth first (`ElementNode::find`).
    pub fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        self.roots.iter().find_map(|root| root.find(id))
    }

    /// Typed read access to a ROOT by id (downcast through `ElementNode::as_any`); `None` when the id is missing or the type differs.
    pub fn root<T: ElementNode + 'static>(&self, id: &str) -> Option<&T> {
        self.roots
            .iter()
            .find(|root| root.base().id() == id)?
            .as_any()
            .downcast_ref::<T>()
    }

    /// Typed write access to a ROOT by id (`ElementNode::as_any_mut`).
    pub fn root_mut<T: ElementNode + 'static>(&mut self, id: &str) -> Option<&mut T> {
        self.roots
            .iter_mut()
            .find(|root| root.base().id() == id)?
            .as_any_mut()
            .downcast_mut::<T>()
    }

    /// C# `Resolve(id)`, for the animator: an [`OpacityStore`] over the roots. Opacity of a non-root id reads as missing (see the module doc).
    pub fn opacity_store(&mut self) -> HostOpacityStore<'_> {
        HostOpacityStore { roots: &mut self.roots }
    }

    /// C# `Arrange(context)`: every root in z order (`OverlayElement::arrange`: measure in the whole client, then place).
    pub fn arrange(&mut self, context: &OverlayLayoutContext<'_>) {
        for root in &mut self.roots {
            root.arrange(context);
        }
    }

    /// C# `Render(dc)`: every root in z order (the toolbar, added last, is drawn last).
    pub fn render(&self, dc: &mut dyn DrawContext) {
        for root in &self.roots {
            root.render(dc);
        }
    }

    /// C# `HitTest(point)`: roots from the topmost, the first `find_at` hit (deepest element); `None` = the click goes to the image.
    pub fn hit_test(&self, point: PointD) -> Option<&dyn ElementNode> {
        self.roots.iter().rev().find_map(|root| root.find_at(point))
    }

    /// C# `Dispose()`: `release_resources()` on every root (text layout leases go back), then clears the roots. Idempotent.
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        for root in &mut self.roots {
            root.release_resources();
        }
        self.roots.clear();
    }

    /// True after `dispose`.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }
}

/// `OpacityStore` over the roots of an [`OverlayHost`] (the Rust twin of the C# `Func<string, IOpacityTarget?>` resolver the animator was given).
#[derive(Debug)]
pub struct HostOpacityStore<'a> {
    roots: &'a mut [Box<dyn ElementNode>],
}

impl OpacityStore for HostOpacityStore<'_> {
    fn opacity(&self, element_id: &str) -> Option<f32> {
        self.roots
            .iter()
            .find(|root| root.base().id() == element_id)
            .map(|root| root.base().opacity())
    }

    fn set_opacity(&mut self, element_id: &str, opacity: f32) {
        if let Some(root) = self.roots.iter_mut().find(|root| root.base().id() == element_id) {
            root.base_mut().set_opacity(opacity);
        }
    }
}
