//! W10-H3: production update checker factory; `src/PhotoReview.App.Shared/Presenters/SettingsPresenter.cs` lines 567-587.

use pr_core::updates::checker::{UpdateCheck, UpdateChecker};
use pr_platform_win::http::WinHttpTransport;
use std::sync::Arc;

/// Construct the inert production checker for manual Settings update checks.
pub fn update_checker() -> Arc<dyn UpdateCheck> {
    Arc::new(UpdateChecker::new(Arc::new(WinHttpTransport), None))
}
