//! Q1 mutation killers (docs/reviews/Q1-PARITY-CLOSE-2026-11-03.md) for `coordinators::traits` default bodies and the
//! `Debug` / `Display` text of `coordinators::traits` and `menus::builder`. New tests: the C# defaults are the oracle
//! (`IFileActionSink.OnFilesRenamed => false`, `IPresentationSink` has no zoom-change pull in C#, see the trait doc).

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::coordinators::traits::{
    FileActionServiceExecutor, IFileActionSink, IPresentationSink, PreloadControllerAdapter,
};
use pr_app::menus::builder::MenuBuildError;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::file_actions::service::FileActionService;
use pr_core::settings::context_menu::ContextMenuItemId;
use pr_imaging::decode_types::PlatformImage;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin};
use std::path::Path;
use std::sync::Arc;

/// A sink that keeps every optional member at its default.
struct DefaultFileActionSink;

impl IFileActionSink for DefaultFileActionSink {
    fn set_status_text(&mut self, _status: &str) {}
    fn show_late_action_status(&mut self, _status: &str) {}
    fn on_catalog_changed(&mut self, _removed_path: Option<&str>) {}
    fn present(&mut self, _index: i32) {}
    fn update_session_path(&mut self, _current_path: &str) {}
    fn notify_navigation_state_changed(&mut self) {}
}

#[test]
fn file_action_sink_on_files_renamed_default_is_false() {
    // C# default member `OnFilesRenamed(...) => false`: the shown image was NOT kept, the caller presents again.
    let mut sink = DefaultFileActionSink;
    let mut renames = PathKeyMap::new();
    assert!(!sink.on_files_renamed(&renames), "empty rename set");
    renames = [(r"C:\a\old.jpg".to_owned(), r"C:\a\new.jpg".to_owned())]
        .into_iter()
        .collect();
    assert!(!sink.on_files_renamed(&renames), "non-empty rename set");
}

/// A presentation sink that keeps `take_viewer_zoom_change` at its default.
struct DefaultPresentationSink;

impl IPresentationSink for DefaultPresentationSink {
    fn set_current_image(&mut self, _image: Option<PlatformImage>, _is_file_change: bool, _change: &ImageChange<'_>) {}
    fn set_status_text(&mut self, _status: &str) {}
    fn apply_initial_view_mode(&mut self) {}
    fn on_presented(&mut self, _path: &str) {}
    fn trace_presented(&mut self, _token: i64, _kind: &str, _assigned_timestamp: i64) {}
}

#[test]
fn presentation_sink_take_viewer_zoom_change_default_is_no_change() {
    // Default: "no change" is `None`; `Some(None)` would mean "the view model asked for Fit".
    let mut sink = DefaultPresentationSink;
    assert_eq!(sink.take_viewer_zoom_change(), None);
    assert_eq!(sink.take_viewer_zoom_change(), None, "still none on a second pull");
}

#[test]
fn debug_text_names_the_type_and_does_not_call_the_factory() {
    // `PreloadControllerAdapter::new` is lazy; Debug must not read the scheduler either (the factory panics if it does).
    let adapter = PreloadControllerAdapter::new(|| -> Arc<pr_imaging::preload::scheduler::PreloadScheduler> {
        panic!("Debug must not call the scheduler factory")
    });
    let text = format!("{adapter:?}");
    assert!(text.starts_with("PreloadControllerAdapter"), "{text}");

    let fs = Arc::new(FakeFileSystem::new());
    let clock = Arc::new(FakeClock::new());
    let paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
    let bin = Arc::new(FakeRecycleBin::new(fs.clone()));
    let service = Arc::new(FileActionService::new(journal, fs, clock, bin));
    let executor = FileActionServiceExecutor::new(service);
    let text = format!("{executor:?}");
    assert!(text.starts_with("FileActionServiceExecutor"), "{text}");
}

#[test]
fn menu_build_error_display_names_the_item_and_the_missing_row_kind() {
    // C# messages: "Context menu item {id} is not a top-level item." / "... is not a Zoom submenu item."
    assert_eq!(
        MenuBuildError::NotTopLevel(ContextMenuItemId::Fit).to_string(),
        "Context menu item Fit is not a top-level item."
    );
    assert_eq!(
        MenuBuildError::NotZoomChild(ContextMenuItemId::Undo).to_string(),
        "Context menu item Undo is not a Zoom submenu item."
    );
}
