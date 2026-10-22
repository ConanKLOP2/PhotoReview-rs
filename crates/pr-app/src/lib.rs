//! Port target: `PhotoReview.App` (composition root, windows, view models). UI framework: docs/adr/R0001.

// The in-crate presenter tests include the shared fakes of `tests/support`, which name the crate as an outside user does.
#[cfg(test)]
extern crate self as pr_app;

// Wave 8 batch B1 modules (contract stubs, lead-owned wiring; contracts/README.md D-W8B1-*).
pub mod coordinators;
pub mod input;
pub mod localization;
pub mod menus;
pub mod misc;
pub mod overlay;
pub mod presenters;
pub mod services;
// Wave 8 batch B6 (W8-17a..17e start-up logic; index lead-owned, contracts/W8-17a.md).
pub mod startup;
pub mod viewing;
pub mod viewmodels;
pub mod viewport;
pub mod windowing;
