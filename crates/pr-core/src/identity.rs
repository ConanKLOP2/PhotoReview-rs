//! Names that keep this app apart from the C# PhotoReview running on the same machine (docs/adr/R0002).
//!
//! The two apps must never share a journal, session, cache or single-instance lock: both run startup
//! reconcile (INV-6), so a shared journal would let one app retry or undo the other's file actions.

/// Folder under `%LOCALAPPDATA%`. The C# app uses [`CSHARP_APP_DIR_NAME`].
pub const APP_DIR_NAME: &str = "PhotoReviewRs";

/// The C# app's folder under `%LOCALAPPDATA%`. Read only, for the one-way first-run `config.json` import.
pub const CSHARP_APP_DIR_NAME: &str = "PhotoReview";

/// Prefix of the single-instance mutex and forward pipe names (C#: `InstanceKeys.DefaultPrefix` = `PhotoReview`).
/// Names are `Local\<prefix>_<hex>`, so `PhotoReviewRs_` can never equal `PhotoReview_`.
pub const INSTANCE_PREFIX: &str = "PhotoReviewRs";

/// Release executable name.
pub const EXE_NAME: &str = "PhotoReviewRs.exe";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_dir_name_differs_from_csharp_app() {
        assert!(!APP_DIR_NAME.eq_ignore_ascii_case(CSHARP_APP_DIR_NAME));
    }

    #[test]
    fn instance_prefix_differs_from_csharp_prefix() {
        let csharp = format!("Local\\{CSHARP_APP_DIR_NAME}_").to_ascii_uppercase();
        let ours = format!("Local\\{INSTANCE_PREFIX}_").to_ascii_uppercase();
        assert!(!ours.starts_with(&csharp));
    }
}
