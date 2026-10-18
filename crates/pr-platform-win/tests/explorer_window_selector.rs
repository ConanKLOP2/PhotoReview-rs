#[path = "../src/explorer_window_selector.rs"]
mod explorer_window_selector;

use std::error::Error;
use std::sync::{Arc, Mutex};

use explorer_window_selector::{
    ExplorerSelectionWindow, LocationReadError, SelectionReadError, select, try_canonicalize_location,
};
use pr_core::abstractions::Log;
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;

#[derive(Default)]
struct MemoryLog(Mutex<Vec<String>>);
impl Log for MemoryLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, text: &str) {
        self.0.lock().unwrap().push(format!("I:{text}"));
    }
    fn warn(&self, text: &str) {
        self.0.lock().unwrap().push(format!("W:{text}"));
    }
    fn error(&self, text: &str, _error: Option<&(dyn Error + 'static)>) {
        self.0.lock().unwrap().push(format!("E:{text}"));
    }
}

struct Window {
    location: Result<Option<String>, LocationReadError>,
    read: Read,
    released: Arc<Mutex<usize>>,
    cancel_on_location: Option<CancelToken>,
}
enum Read {
    Snapshot(ExplorerViewSnapshot),
    Error,
    Cancel,
}
impl ExplorerSelectionWindow for Window {
    fn location_url(&self) -> Result<Option<String>, LocationReadError> {
        if let Some(token) = &self.cancel_on_location {
            token.cancel();
        }
        match &self.location {
            Ok(location) => Ok(location.clone()),
            Err(LocationReadError::Unavailable) => Err(LocationReadError::Unavailable),
            Err(LocationReadError::Other(_)) => {
                Err(LocationReadError::Other(Box::new(std::io::Error::other("location"))))
            }
        }
    }
    fn read(&self, _folder: &str, _token: &CancelToken) -> Result<ExplorerViewSnapshot, SelectionReadError> {
        match &self.read {
            Read::Snapshot(snapshot) => Ok(snapshot.clone()),
            Read::Error => Err(SelectionReadError {
                type_name: "COMException".into(),
                hresult: 0x80004005_u32 as i32,
                canceled: false,
            }),
            Read::Cancel => Err(SelectionReadError {
                type_name: "OperationCanceledException".into(),
                hresult: 0x8013153B_u32 as i32,
                canceled: true,
            }),
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() += 1;
    }
}

fn snapshot(status: ExplorerOrderStatus, reason: Option<&str>) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: r"C:\photos".into(),
        ordered_paths: vec![],
        sort_columns: vec![],
        group_state: ExplorerGroupState::Unknown,
        status,
        reason: reason.map(str::to_owned),
        captured_utc: DateTimeUtc::MIN,
    }
}
fn window(location: Result<Option<&str>, bool>, read: Read, released: Arc<Mutex<usize>>) -> Window {
    Window {
        location: match location {
            Ok(value) => Ok(value.map(str::to_owned)),
            Err(false) => Err(LocationReadError::Unavailable),
            Err(true) => Err(LocationReadError::Other(Box::new(std::io::Error::other("location")))),
        },
        read,
        released,
        cancel_on_location: None,
    }
}
fn boxed_windows<I>(windows: I) -> impl Iterator<Item = Result<Window, Box<dyn Error + Send + Sync>>>
where
    I: IntoIterator<Item = Result<Window, Box<dyn Error + Send + Sync>>>,
{
    windows.into_iter()
}
fn releases(counter: &Arc<Mutex<usize>>) -> usize {
    *counter.lock().unwrap()
}

#[test]
fn no_windows_reports_no_matching_window_with_count() {
    // C#: ExplorerWindowSelectorTests.NoWindowsReportsNoMatchingWindowWithCount
    let log = MemoryLog::default();
    let result = select(
        r"C:\photos",
        Vec::<Result<Window, Box<dyn Error + Send + Sync>>>::new(),
        &log,
        &CancelToken::none(),
    );
    assert_eq!(result.status, ExplorerOrderStatus::NoMatchingWindow);
    assert_eq!(result.reason.as_deref(), Some("no-matching-window|0"));
}

#[test]
fn windows_on_other_folders_or_non_file_locations_are_skipped_and_all_released() {
    // C#: ExplorerWindowSelectorTests.WindowsOnOtherFoldersOrNonFileLocationsAreSkippedAndAllReleased
    let counter = Arc::new(Mutex::new(0));
    let windows: Vec<Result<Window, Box<dyn Error + Send + Sync>>> = [
        Some("file:///C:/other"),
        Some("https://example.com"),
        Some("file:///C:/photos"),
    ]
    .into_iter()
    .map(|loc| {
        Ok(window(
            Ok(loc),
            Read::Snapshot(snapshot(ExplorerOrderStatus::Available, None)),
            counter.clone(),
        ))
    })
    .collect();
    let mut windows = windows.into_iter();
    let mut requested = 0;
    let lazy = std::iter::from_fn(|| {
        requested += 1;
        assert!(requested <= 3, "selector requested a window after the available result");
        windows.next()
    });
    let result = select(
        r"C:\photos",
        boxed_windows(lazy),
        &MemoryLog::default(),
        &CancelToken::none(),
    );
    assert_eq!(result.status, ExplorerOrderStatus::Available);
    assert_eq!(releases(&counter), 3);
}

#[test]
fn location_spellings_of_the_same_folder_match() {
    // C#: ExplorerWindowSelectorTests.LocationSpellingsOfTheSameFolderMatch
    assert_eq!(
        try_canonicalize_location("file:///C:/photos/"),
        Some(r"C:\photos".into())
    );
}
#[test]
fn unicode_and_percent_encoded_folder_names_match() {
    // C#: ExplorerWindowSelectorTests.UnicodeAndPercentEncodedFolderNamesMatch
    assert_eq!(
        try_canonicalize_location("file:///C:/photos/%E6%97%A5%E6%9C%AC"),
        Some("C:\\photos\\日本".into())
    );
}
#[test]
fn unc_location_matches_unc_folder() {
    // C#: ExplorerWindowSelectorTests.UncLocationMatchesUncFolder
    assert_eq!(
        try_canonicalize_location("file://server/share/photos"),
        Some(r"\\server\share\photos".into())
    );
}
#[test]
fn window_closing_during_location_read_is_skipped() {
    // C#: ExplorerWindowSelectorTests.WindowClosingDuringLocationReadIsSkipped
    let counter = Arc::new(Mutex::new(0));
    let w = window(Err(false), Read::Error, counter.clone());
    let result = select(r"C:\photos", [Ok(w)], &MemoryLog::default(), &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::NoMatchingWindow);
    assert_eq!(releases(&counter), 1);
}
#[test]
fn window_without_the_browser_interface_is_skipped_without_a_log_entry() {
    // C#: ExplorerWindowSelectorTests.WindowWithoutTheBrowserInterfaceIsSkippedWithoutALogEntry
    let log = MemoryLog::default();
    let counter = Arc::new(Mutex::new(0));
    let _ = select(
        r"C:\photos",
        [Ok(window(Err(false), Read::Error, counter.clone()))],
        &log,
        &CancelToken::none(),
    );
    assert!(log.0.lock().unwrap().iter().all(|entry| !entry.starts_with("E:")));
    assert_eq!(releases(&counter), 1);
}
#[test]
fn unexpected_location_error_is_logged_and_skipped() {
    // C#: ExplorerWindowSelectorTests.UnexpectedLocationErrorIsLoggedAndSkipped
    let log = MemoryLog::default();
    let counter = Arc::new(Mutex::new(0));
    let _ = select(
        r"C:\photos",
        [Ok(window(Err(true), Read::Error, counter.clone()))],
        &log,
        &CancelToken::none(),
    );
    assert!(
        log.0
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry.starts_with("E:Explorer window inspection failed"))
    );
    assert_eq!(releases(&counter), 1);
}
#[test]
fn multiple_matching_windows_fall_through_failures() {
    // C#: ExplorerWindowSelectorTests.MultipleMatchingWindowsFallThroughFailures
    let counter = Arc::new(Mutex::new(0));
    let windows: Vec<Result<Window, Box<dyn Error + Send + Sync>>> = [
        Read::Snapshot(snapshot(ExplorerOrderStatus::Failed, Some("first"))),
        Read::Snapshot(snapshot(ExplorerOrderStatus::Available, None)),
    ]
    .into_iter()
    .map(|read| Ok(window(Ok(Some("file:///C:/photos")), read, counter.clone())))
    .collect();
    assert_eq!(
        select(
            r"C:\photos",
            boxed_windows(windows),
            &MemoryLog::default(),
            &CancelToken::none()
        )
        .status,
        ExplorerOrderStatus::Available
    );
    assert_eq!(releases(&counter), 2);
}
#[test]
fn all_matching_windows_failing_reports_first_failure() {
    // C#: ExplorerWindowSelectorTests.AllMatchingWindowsFailingReportsFirstFailure
    let counter = Arc::new(Mutex::new(0));
    let windows: Vec<Result<Window, Box<dyn Error + Send + Sync>>> = ["first", "second"]
        .into_iter()
        .map(|reason| {
            Ok(window(
                Ok(Some("file:///C:/photos")),
                Read::Snapshot(snapshot(ExplorerOrderStatus::Failed, Some(reason))),
                counter.clone(),
            ))
        })
        .collect();
    assert_eq!(
        select(
            r"C:\photos",
            boxed_windows(windows),
            &MemoryLog::default(),
            &CancelToken::none()
        )
        .reason
        .as_deref(),
        Some("first")
    );
    assert_eq!(releases(&counter), 2);
}
#[test]
fn thrown_read_is_reported_with_type_and_hresult() {
    // C#: ExplorerWindowSelectorTests.ThrownReadIsReportedWithTypeAndHresult
    let counter = Arc::new(Mutex::new(0));
    let result = select(
        r"C:\photos",
        [Ok(window(Ok(Some("file:///C:/photos")), Read::Error, counter.clone()))],
        &MemoryLog::default(),
        &CancelToken::none(),
    );
    assert_eq!(
        result.reason.as_deref(),
        Some("native-view-failed|COMException|0x80004005")
    );
    assert_eq!(releases(&counter), 1);
}
#[test]
fn cancellation_during_read_stops_the_walk_and_releases() {
    // C#: ExplorerWindowSelectorTests.CancellationDuringReadStopsTheWalkAndReleases
    let counter = Arc::new(Mutex::new(0));
    let result = select(
        r"C:\photos",
        [Ok(window(Ok(Some("file:///C:/photos")), Read::Cancel, counter.clone()))],
        &MemoryLog::default(),
        &CancelToken::none(),
    );
    assert_eq!(result.status, ExplorerOrderStatus::Canceled);
    assert_eq!(releases(&counter), 1);
}
#[test]
fn cancelled_token_stops_before_looking_at_any_location() {
    // C#: ExplorerWindowSelectorTests.CancelledTokenStopsBeforeLookingAtAnyLocation
    let counter = Arc::new(Mutex::new(0));
    let token = CancelToken::new();
    token.cancel();
    let result = select(
        r"C:\photos",
        [Ok(window(Ok(Some("file:///C:/photos")), Read::Error, counter.clone()))],
        &MemoryLog::default(),
        &token,
    );
    assert_eq!(result.status, ExplorerOrderStatus::Canceled);
    assert_eq!(releases(&counter), 1);
}
#[test]
fn cancellation_requested_between_windows_stops_the_walk() {
    // C#: ExplorerWindowSelectorTests.CancellationRequestedBetweenWindowsStopsTheWalk
    let counter = Arc::new(Mutex::new(0));
    let token = CancelToken::new();
    let mut first = window(Ok(Some("file:///C:/other")), Read::Error, counter.clone());
    first.cancel_on_location = Some(token.clone());
    let second = window(Ok(Some("file:///C:/photos")), Read::Error, counter.clone());
    let result = select(r"C:\photos", [Ok(first), Ok(second)], &MemoryLog::default(), &token);
    assert_eq!(result.status, ExplorerOrderStatus::Canceled);
    assert_eq!(releases(&counter), 2);
}
#[test]
fn try_canonicalize_location_decodes_file_urls() {
    // C#: ExplorerWindowSelectorTests.TryCanonicalizeLocationDecodesFileUrls
    assert_eq!(try_canonicalize_location("file:///C:/a%20b"), Some(r"C:\a b".into()));
}
#[test]
fn try_canonicalize_location_rejects_non_file_locations() {
    // C#: ExplorerWindowSelectorTests.TryCanonicalizeLocationRejectsNonFileLocations
    for url in [
        "",
        "relative",
        "https://example.com",
        "shell:Downloads",
        "file://user@host/share",
    ] {
        assert_eq!(try_canonicalize_location(url), None, "{url}");
    }
}
#[test]
fn enumerator_move_next_failure_after_window_on_folder_keeps_first_failure_and_releases_window() {
    // C#: ExplorerWindowSelectorTests.EnumeratorMoveNextFailure_AfterWindowOnFolder_KeepsFirstFailureAndReleasesWindow
    let counter = Arc::new(Mutex::new(0));
    let error: Box<dyn Error + Send + Sync> = Box::new(std::io::Error::other("iterator"));
    let iter = vec![
        Ok(window(
            Ok(Some("file:///C:/photos")),
            Read::Snapshot(snapshot(ExplorerOrderStatus::Failed, Some("first"))),
            counter.clone(),
        )),
        Err(error),
    ];
    let result = select(
        r"C:\photos",
        boxed_windows(iter),
        &MemoryLog::default(),
        &CancelToken::none(),
    );
    assert_eq!(result.reason.as_deref(), Some("first"));
    assert_eq!(releases(&counter), 1);
}
#[test]
fn enumerator_move_next_failure_no_match_reports_no_matching_window() {
    // C#: ExplorerWindowSelectorTests.EnumeratorMoveNextFailure_NoMatch_ReportsNoMatchingWindow
    let error: Box<dyn Error + Send + Sync> = Box::new(std::io::Error::other("iterator"));
    let iter: Vec<Result<Window, Box<dyn Error + Send + Sync>>> = vec![Err(error)];
    let result = select(r"C:\photos", iter, &MemoryLog::default(), &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::NoMatchingWindow);
    assert_eq!(result.reason.as_deref(), Some("no-matching-window|0"));
}
