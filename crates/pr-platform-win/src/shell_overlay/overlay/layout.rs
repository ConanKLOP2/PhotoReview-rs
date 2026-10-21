//! `ShellOverlay` part 2, layout (PURE: sizes in, boxes out; no OS, no drawing). Port of `ShellOverlay.cs` lines 247 to 253 (`Arrange`), baseline `d92d6aae`.
//! Unit W8-15h (contract `contracts/W8-15h.md`). The element layout itself is `pr_app::overlay` (15a to 15e); this file only builds the layout context
//! and keeps the last client size and DPI that `paint` and the tooltips use.
//!
//! Layout works in DIP and ignores the DPI (C# `OverlayElement` doc): the DPI is stored for drawing and for the DIP-to-pixel conversion of the tooltips.

use pr_app::input::primitives::{RectD, SizeD};
use pr_app::overlay::contracts::OverlayLayoutContext;

use super::state::ShellOverlay;

/// C# `_lastDpi = dpiScale <= 0 ? 1 : dpiScale` (NaN is not `<= 0`, so it is kept, as C#).
pub fn effective_dpi(dpi_scale: f64) -> f64 {
    if dpi_scale <= 0.0 { 1.0 } else { dpi_scale }
}

impl ShellOverlay {
    /// C# `Arrange(clientDip, dpiScale)`: stores `last_client` and `last_dpi = effective_dpi(dpi)`, then arranges the host with
    /// `OverlayLayoutContext::with_renderer_id(client, last_dpi, text, text_renderer_id)`. Used by `paint::render` and directly by tests (boxes and hit tests
    /// without a draw surface). Not guarded by `disposed` (C# did not check it; the host is empty after dispose, so it is a no-op).
    pub fn arrange(&self, client_dip: SizeD, dpi_scale: f64) {
        let dpi = effective_dpi(dpi_scale);
        let mut guard = self.state.borrow_mut();
        let state = &mut *guard;
        state.last_client = client_dip;
        state.last_dpi = dpi;
        let context = OverlayLayoutContext::with_renderer_id(client_dip, dpi, &*self.text, self.text_renderer_id);
        state.host.arrange(&context);
    }

    /// The client size of the last `arrange` (C# `_lastClient`).
    pub fn last_client(&self) -> SizeD {
        self.state.borrow().last_client
    }

    /// The DPI scale of the last `arrange` (C# `_lastDpi`).
    pub fn last_dpi(&self) -> f64 {
        self.state.borrow().last_dpi
    }

    /// C# `Host.Find(id).Bounds` (used by the golden tests and the `Center(...)` helper of the C# test rig): the box of any element after the last arrange.
    pub fn element_bounds(&self, id: &str) -> Option<RectD> {
        self.state.borrow().host.find(id).map(|element| element.base().bounds())
    }
}
