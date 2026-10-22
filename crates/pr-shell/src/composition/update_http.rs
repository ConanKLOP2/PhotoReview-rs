#![allow(unused_variables, dead_code)] // Contract stub; porter removes with the last todo.
//! W10-H3: production update checker factory; `src/PhotoReview.App.Shared/Presenters/SettingsPresenter.cs` lines 567-587.

use pr_core::updates::checker::UpdateCheck;
use std::sync::Arc;

/// Construct the inert production checker for manual Settings update checks.
pub fn update_checker() -> Arc<dyn UpdateCheck> {
    todo!()
}
