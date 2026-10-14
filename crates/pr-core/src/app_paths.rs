//! Port of `PhotoReview.Core/AppPaths.cs` (C# baseline e441d67a) with the root renamed to
//! [`APP_DIR_NAME`](crate::identity::APP_DIR_NAME) and Rust-specific environment variables, so a test or a
//! debug run of one app can never redirect the other app's data.

use std::path::{Path, PathBuf};

use crate::identity::APP_DIR_NAME;

/// C#: `PHOTOREVIEW_DATA_ROOT`.
pub const DATA_ROOT_ENV: &str = "PHOTOREVIEW_RS_DATA_ROOT";

/// C#: `PHOTOREVIEW_ISOLATE_CONFIG`. "1"/"true" together with [`DATA_ROOT_ENV`]: `config.json` and the user files
/// also move under the data root, so a test that saves settings never overwrites the user's real config.
pub const ISOLATE_CONFIG_ENV: &str = "PHOTOREVIEW_RS_ISOLATE_CONFIG";

/// Every file and folder the app reads or writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub journal_file: PathBuf,
    pub sessions_dir: PathBuf,
    pub log_file: PathBuf,
    pub preview_cache_dir: PathBuf,
    pub thumbnail_cache_dir: PathBuf,
    pub window_placement_file: PathBuf,
}

impl AppPaths {
    /// Same layout as the C# app:
    /// `<app>\config.json`, `<data>\operations.jsonl`, `<data>\Sessions`, `<override or app>\logs\app.log`,
    /// `<user>\cache`, `<user>\thumbnails`, `<user>\window-placement.json`,
    /// where `<app>` = `<local_app_data>\PhotoReviewRs` and `<data>` = override or `<app>\Data`.
    pub fn new(local_app_data: &Path, data_root_override: Option<&str>, isolate_config: bool) -> Self {
        let app_root = local_app_data.join(APP_DIR_NAME);
        // Resolved once: a relative override would otherwise follow the (mutable) current directory at every later
        // use. An unusable value must not crash startup or logger init: it is ignored.
        let override_root = resolve_override(data_root_override);
        let isolated_root = override_root.as_ref().filter(|_| isolate_config);

        let data_root = override_root.clone().unwrap_or_else(|| app_root.join("Data"));
        let config_root = isolated_root.unwrap_or(&app_root);
        let log_root = override_root.as_ref().unwrap_or(&app_root);
        let user_root = isolated_root.unwrap_or(&app_root);

        Self {
            config_file: config_root.join("config.json"),
            journal_file: data_root.join("operations.jsonl"),
            sessions_dir: data_root.join("Sessions"),
            log_file: log_root.join("logs").join("app.log"),
            preview_cache_dir: user_root.join("cache"),
            thumbnail_cache_dir: user_root.join("thumbnails"),
            window_placement_file: user_root.join("window-placement.json"),
        }
    }

    /// The only place that reads [`DATA_ROOT_ENV`] / [`ISOLATE_CONFIG_ENV`].
    pub fn from_environment() -> Self {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let data_root = std::env::var(DATA_ROOT_ENV).ok();
        let isolate = std::env::var(ISOLATE_CONFIG_ENV).ok();
        Self::new(
            &resolve_local_app_data(local),
            data_root.as_deref(),
            is_truthy(isolate.as_deref()),
        )
    }
}

/// A profile without `LOCALAPPDATA` (service accounts, stripped profiles) falls back to the temp folder instead of
/// failing startup.
fn resolve_local_app_data(local_app_data: Option<PathBuf>) -> PathBuf {
    match local_app_data {
        Some(p) if !p.to_string_lossy().trim().is_empty() => p,
        _ => std::env::temp_dir(),
    }
}

fn resolve_override(value: Option<&str>) -> Option<PathBuf> {
    let trimmed = value?.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    std::path::absolute(trimmed).ok()
}

fn is_truthy(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.trim();
        v == "1" || v.eq_ignore_ascii_case("true")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> PathBuf {
        PathBuf::from(r"C:\Users\u\AppData\Local")
    }

    #[test]
    fn new_without_override_uses_rust_app_root() {
        let p = AppPaths::new(&local(), None, false);
        let app = local().join("PhotoReviewRs");
        assert_eq!(p.config_file, app.join("config.json"));
        assert_eq!(p.journal_file, app.join("Data").join("operations.jsonl"));
        assert_eq!(p.sessions_dir, app.join("Data").join("Sessions"));
        assert_eq!(p.log_file, app.join("logs").join("app.log"));
        assert_eq!(p.preview_cache_dir, app.join("cache"));
        assert_eq!(p.thumbnail_cache_dir, app.join("thumbnails"));
        assert_eq!(p.window_placement_file, app.join("window-placement.json"));
    }

    #[test]
    fn new_never_points_into_csharp_app_root() {
        let p = AppPaths::new(&local(), None, false);
        let csharp = local().join(crate::identity::CSHARP_APP_DIR_NAME);
        let all = [
            &p.config_file,
            &p.journal_file,
            &p.sessions_dir,
            &p.log_file,
            &p.preview_cache_dir,
            &p.thumbnail_cache_dir,
            &p.window_placement_file,
        ];
        for path in all {
            assert!(
                !path.starts_with(&csharp),
                "{} is under the C# app root",
                path.display()
            );
        }
    }

    #[test]
    fn new_with_override_moves_data_and_log_but_keeps_config_and_caches() {
        let p = AppPaths::new(&local(), Some(r"D:\data"), false);
        let app = local().join("PhotoReviewRs");
        assert_eq!(p.journal_file, PathBuf::from(r"D:\data\operations.jsonl"));
        assert_eq!(p.sessions_dir, PathBuf::from(r"D:\data\Sessions"));
        assert_eq!(p.log_file, PathBuf::from(r"D:\data\logs\app.log"));
        assert_eq!(p.config_file, app.join("config.json"));
        assert_eq!(p.preview_cache_dir, app.join("cache"));
    }

    #[test]
    fn new_with_override_and_isolate_moves_config_and_user_files() {
        let p = AppPaths::new(&local(), Some(r"D:\data"), true);
        assert_eq!(p.config_file, PathBuf::from(r"D:\data\config.json"));
        assert_eq!(p.preview_cache_dir, PathBuf::from(r"D:\data\cache"));
        assert_eq!(p.thumbnail_cache_dir, PathBuf::from(r"D:\data\thumbnails"));
        assert_eq!(p.window_placement_file, PathBuf::from(r"D:\data\window-placement.json"));
    }

    #[test]
    fn new_with_isolate_but_no_override_keeps_config_in_app_root() {
        let p = AppPaths::new(&local(), None, true);
        assert_eq!(p.config_file, local().join("PhotoReviewRs").join("config.json"));
    }

    #[test]
    fn new_with_blank_or_nul_override_ignores_it() {
        let default = AppPaths::new(&local(), None, false);
        assert_eq!(AppPaths::new(&local(), Some("   "), false), default);
        assert_eq!(AppPaths::new(&local(), Some("a\0b"), false), default);
    }

    #[test]
    fn new_with_relative_override_resolves_to_absolute_once() {
        let p = AppPaths::new(&local(), Some("rel-data"), false);
        assert!(p.journal_file.is_absolute());
        assert!(p.journal_file.ends_with(Path::new("rel-data").join("operations.jsonl")));
    }

    #[test]
    fn resolve_local_app_data_empty_falls_back_to_temp() {
        assert_eq!(resolve_local_app_data(None), std::env::temp_dir());
        assert_eq!(resolve_local_app_data(Some(PathBuf::from("  "))), std::env::temp_dir());
        assert_eq!(resolve_local_app_data(Some(local())), local());
    }

    #[test]
    fn is_truthy_accepts_only_one_and_true() {
        assert!(is_truthy(Some(" 1 ")));
        assert!(is_truthy(Some("TRUE")));
        assert!(!is_truthy(Some("yes")));
        assert!(!is_truthy(Some("0")));
        assert!(!is_truthy(None));
    }
}
