//! Native dialog infrastructure (Wave 8 B5, W8-18d..18n): template, chrome, controls, the `NativeDialog` base class and the dark painter.

// W8-18d..18g (contract group G2): template, chrome, edit centering, list dialog, control wrappers.
pub mod chrome;
pub mod controls;
pub mod edit_centering;
pub mod list_dialog;
pub mod template;

// W8-18h..18k (contract group G3): the NativeDialog base class (core, modal, dpi, layout, focus, events, tooltip, draw, result).
pub mod dialog;

// W8-18l..18n (contract group G4): the hand-painted dark look of the controls (kinds and window procedures, push button and shared drawing, the other widgets and borders).
pub mod painter;
