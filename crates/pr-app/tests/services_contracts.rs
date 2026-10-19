//! Tests of `pr_app::services::contracts` (contract `contracts/W8-04c.md`, rules R-CT-1..4).
//!
//! Port of the C# seams `IClipboardService`, `IPresentationObserver`, `IZoomPromptService` and `UnavailableClipboardService`.
//! There is no C# test file for these seams; the tests below follow the contract's test plan.

use std::cell::Cell;

use pr_app::services::contracts::{
    ClipboardService, NullPresentationObserver, PresentationObserver, UnavailableClipboardService, ZoomPromptService,
};

// Contract test (R-CT-2): C# `UnavailableClipboardService.TrySetText(text) => false` for every text.
#[test]
fn unavailable_clipboard_reports_unavailable_for_every_text() {
    let clipboard = UnavailableClipboardService;
    for text in ["", "x", "Příliš žluťoučký kůň"] {
        assert!(!clipboard.try_set_text(text), "text {text:?} must report unavailable");
    }
}

// Contract test (R-CT-3): C# `NullPresentationObserver` does nothing for any path, including the empty path.
#[test]
fn null_presentation_observer_ignores_every_path() {
    let observer = NullPresentationObserver;
    for path in ["", r"C:\photos\a.jpg", r"\\server\share\b.png"] {
        observer.on_presented(path);
    }
}

/// Fake zoom prompt: records what it was asked and answers with a scripted value (UI-thread seam, so `Cell`, no `Send`; T-11).
struct FakeZoomPrompt {
    answer: Option<i32>,
    asked: Cell<Option<i32>>,
}

impl ZoomPromptService for FakeZoomPrompt {
    fn prompt_custom_zoom(&self, current_percent: i32) -> Option<i32> {
        self.asked.set(Some(current_percent));
        self.answer
    }
}

// Contract test (R-CT-4): a cancelled prompt gives `None`, through the trait object only.
// NOTE: this test has no stub body to hit; it checks the trait shape and passes by construction (documented in the report).
#[test]
fn zoom_prompt_trait_object_returns_none_on_cancel() {
    let prompt = FakeZoomPrompt {
        answer: None,
        asked: Cell::new(None),
    };
    let dynamic: &dyn ZoomPromptService = &prompt;

    assert_eq!(dynamic.prompt_custom_zoom(150), None);
    assert_eq!(prompt.asked.get(), Some(150));
}
