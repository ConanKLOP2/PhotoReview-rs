use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pr_app::coordinators::sibling_navigator::{
    SettingGetter, SiblingFolderNavigator, SiblingNavError, SiblingNavOutcome, SiblingSearch,
};
use pr_app::coordinators::traits::ISiblingNavigatorSink;
use pr_app::viewmodels::format::context::{DateText, FormatContext};
use pr_core::abstractions::{AppendStream, FileStat, FileSystem, ReadSeek};
use pr_core::catalog::generation::GenerationClock;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;
use pr_test_support::TempRoot;

type FileHook = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

struct HookedFileSystem {
    inner: PhysicalFileSystem,
    on_enumerate_files: FileHook,
}

impl HookedFileSystem {
    fn new(hook: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static) -> Self {
        Self {
            inner: PhysicalFileSystem,
            on_enumerate_files: Arc::new(hook),
        }
    }
}

impl FileSystem for HookedFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.inner.get_file_stat(path)
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.inner.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.inner.write_all_text_atomic(path, text, durable)
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        (self.on_enumerate_files)(directory)?;
        self.inner.enumerate_files(directory, pattern)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }
}

#[derive(Default)]
struct RecordingSink {
    statuses: Vec<String>,
    opened: Vec<(String, Option<String>)>,
}

impl ISiblingNavigatorSink for RecordingSink {
    fn set_status_text(&mut self, status: &str) {
        self.statuses.push(status.to_owned());
    }
    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) {
        self.opened.push((folder.to_owned(), initial_path.map(str::to_owned)));
    }
}

#[derive(Debug)]
struct UnusedDates;
impl DateText for UnusedDates {
    fn general(&self, _: DateTimeUtc) -> Option<String> {
        None
    }
    fn invariant_general(&self, _: DateTimeUtc) -> String {
        String::new()
    }
}

struct TestText {
    localizer: Localizer,
    numbers: NumberCulture,
    dates: UnusedDates,
}
impl TestText {
    fn new() -> Self {
        let json = r#"{"_meta":{"code":"en","name":"English","nativeName":"English","plural":"OneOther"},"status.siblingFolder.last":"Last sibling folder","status.siblingFolder.first":"First sibling folder"}"#;
        let catalog = LanguageCatalog::try_parse(json, "test", &mut Vec::new()).unwrap();
        Self {
            localizer: Localizer::create(&catalog, &[], &[]),
            numbers: NumberCulture::invariant(),
            dates: UnusedDates,
        }
    }
    fn context(&self) -> FormatContext<'_> {
        FormatContext {
            texts: &self.localizer,
            numbers: &self.numbers,
            group_separator: ",",
            dates: &self.dates,
        }
    }
}

struct Driver {
    root: TempRoot,
    clock: Arc<GenerationClock>,
    fs: Arc<dyn FileSystem>,
    sink: RecordingSink,
    text: TestText,
}
impl Driver {
    fn new(label: &str) -> Self {
        Self {
            root: TempRoot::new(label),
            clock: Arc::new(GenerationClock::default()),
            fs: Arc::new(PhysicalFileSystem),
            sink: RecordingSink::default(),
            text: TestText::new(),
        }
    }
    fn folder(&self, name: &str, file: Option<&str>) -> String {
        let path = self.root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        if let Some(file) = file {
            std::fs::write(path.join(file), [1]).unwrap();
        }
        path.to_string_lossy().into_owned()
    }
    fn navigator(&self) -> SiblingFolderNavigator {
        SiblingFolderNavigator::new(self.clock.clone(), self.fs.clone(), None, None)
    }
    fn navigate(
        &mut self,
        direction: i32,
        session: Option<&str>,
        catalog: Option<&str>,
    ) -> (SiblingNavOutcome, Option<String>) {
        let navigator = self.navigator();
        let search = navigator.begin_navigate(direction, session, catalog).unwrap().unwrap();
        let target = navigator.find_next_image_folder(&search).unwrap();
        let outcome = navigator.complete_navigate(&search, target.as_deref(), &mut self.sink, &self.text.context());
        (outcome, target)
    }
}

// C#: SiblingFolderNavigatorTests.NavigateSiblingFolderAsync_DirectionOtherThanPlusMinusOne_Throws
#[test]
fn navigate_direction_other_than_plus_minus_one_throws() {
    let d = Driver::new("sib-direction");
    d.folder("a", None);
    let b = d.folder("b", None);
    d.folder("c", None);
    for direction in [0, 2, -2] {
        assert!(
            matches!(d.navigator().begin_navigate(direction, Some(&b), None), Err(SiblingNavError::InvalidDirection(n)) if n == direction)
        );
    }
    assert!(d.sink.opened.is_empty());
}

// C#: SiblingFolderNavigatorTests.NavigateSiblingFolderAsync_Next_OpensTheNextImageFolder
#[test]
fn navigate_next_opens_the_next_image_folder() {
    let mut d = Driver::new("sib-next");
    let a = d.folder("a", Some("a.jpg"));
    d.folder("b", None);
    let c = d.folder("c", Some("a.jpg"));
    assert_eq!(d.navigate(1, Some(&a), None).0, SiblingNavOutcome::Opened);
    assert_eq!(d.sink.opened, vec![(c, None)]);
}

// C#: SiblingFolderNavigatorTests.NavigateSiblingFolderAsync_FolderGenerationChangesDuringSearch_DoesNotOpenAnything
#[test]
fn navigate_folder_generation_changes_during_search_does_not_open_anything() {
    let mut d = Driver::new("sib-stale");
    let a = d.folder("a", Some("a.jpg"));
    let b = d.folder("b", Some("a.jpg"));
    let clock = d.clock.clone();
    d.fs = Arc::new(HookedFileSystem::new(move |path| {
        if path == Path::new(&b) {
            clock.next_folder();
        }
        Ok(())
    }));
    assert_eq!(d.navigate(1, Some(&a), None).0, SiblingNavOutcome::Stale);
    assert!(d.sink.opened.is_empty());
    assert!(d.sink.statuses.is_empty());
}

// C#: SiblingFolderNavigatorTests.NavigateSiblingFolderAsync_UnreadableSibling_IsSkipped
#[test]
fn navigate_unreadable_sibling_is_skipped() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
        let mut d = Driver::new("sib-unreadable");
        let a = d.folder("a", Some("a.jpg"));
        let b = d.folder("b", Some("a.jpg"));
        let c = d.folder("c", Some("a.jpg"));
        d.fs = Arc::new(HookedFileSystem::new(move |path| {
            if path == Path::new(&b) {
                Err(io::Error::new(kind, "unreadable"))
            } else {
                Ok(())
            }
        }));
        assert_eq!(d.navigate(1, Some(&a), None).0, SiblingNavOutcome::Opened);
        assert_eq!(d.sink.opened, vec![(c, None)]);
    }
}

// C#: SiblingFolderNavigatorTests.NavigateSiblingFolderAsync_RootFolderWithoutParent_IsANoOp
#[test]
fn navigate_root_folder_without_parent_is_a_no_op() {
    let mut d = Driver::new("sib-root");
    let root = d.root.path().ancestors().last().unwrap().to_string_lossy().into_owned();
    assert_eq!(d.navigate(1, Some(&root), None).0, SiblingNavOutcome::Boundary);
    assert!(d.sink.opened.is_empty());
}

// C#: SiblingFolderNavigatorMutationGapTests.NavigateSiblingFolderAsync_NoSession_StartsFromTheFolderOfTheCurrentCatalogEntry
#[test]
fn navigate_no_session_starts_from_the_folder_of_the_current_catalog_entry() {
    let mut d = Driver::new("sib-catalog");
    let b = d.folder("b", Some("a.jpg"));
    let c = d.folder("c", Some("a.jpg"));
    let catalog = Path::new(&b).join("a.jpg").to_string_lossy().into_owned();
    assert_eq!(d.navigate(1, None, Some(&catalog)).0, SiblingNavOutcome::Opened);
    assert_eq!(d.sink.opened, vec![(c, None)]);
}

// C#: SiblingFolderNavigatorMutationGapTests.NavigateSiblingFolderAsync_NoSessionAndEmptyCatalog_DoesNothing
#[test]
fn navigate_no_session_and_empty_catalog_does_nothing() {
    let d = Driver::new("sib-empty-catalog");
    assert_eq!(d.navigator().begin_navigate(1, None, None), Ok(None));
}

// C#: SiblingFolderNavigatorMutationGapTests.NavigateSiblingFolderAsync_SessionFolderDiffersFromTheCatalogEntry_TheSessionFolderWins
#[test]
fn navigate_session_folder_differs_from_the_catalog_entry_the_session_folder_wins() {
    let mut d = Driver::new("sib-session-wins");
    d.folder("a", Some("a.jpg"));
    let b = d.folder("b", Some("a.jpg"));
    let c = d.folder("c", Some("a.jpg"));
    let e = d.folder("e", Some("a.jpg"));
    let catalog = Path::new(&e).join("a.jpg").to_string_lossy().into_owned();
    assert_eq!(d.navigate(1, Some(&b), Some(&catalog)).0, SiblingNavOutcome::Opened);
    assert_eq!(d.sink.opened, vec![(c, None)]);
}

// C#: SiblingFolderNavigatorMutationGapTests.FindSiblingImageFolders_CurrentFolderIsTheFirstSibling_FindsTheNextOneAndNoPrevious
#[test]
fn find_sibling_image_folders_current_folder_is_the_first_sibling_finds_the_next_one_and_no_previous() {
    let d = Driver::new("sib-first");
    let a = d.folder("a", Some("a.jpg"));
    let b = d.folder("b", Some("a.jpg"));
    let result = d
        .navigator()
        .find_sibling_image_folders(&a, &CancelToken::none())
        .unwrap();
    assert_eq!(result.previous, None);
    assert_eq!(result.next, Some(b));
}

// C#: SiblingFolderNavigatorMutationGapTests.FindSiblingImageFolders_RawSettingOn_CountsAFolderWithOnlyRawFiles
#[test]
fn find_sibling_image_folders_raw_setting_on_counts_a_folder_with_only_raw_files() {
    let d = Driver::new("sib-raw");
    let a = d.folder("a", Some("a.jpg"));
    let b = d.folder("b", Some("shot.cr2"));
    for (enabled, expected) in [(true, Some(b)), (false, None)] {
        let getter: SettingGetter = Arc::new(move || enabled);
        let nav = SiblingFolderNavigator::new(d.clock.clone(), d.fs.clone(), Some(getter), None);
        assert_eq!(
            nav.find_sibling_image_folders(&a, &CancelToken::none()).unwrap().next,
            expected
        );
    }
}

// C#: ViewerQuickFeaturesTests.FindSiblingImageFolders_SkipsFoldersWithoutImages_BothDirections
#[test]
fn find_sibling_image_folders_skips_folders_without_images_both_directions() {
    let d = Driver::new("sib-both");
    let a = d.folder("a", Some("1.jpg"));
    d.folder("b", Some("1.txt"));
    let c = d.folder("c", Some("1.png"));
    d.folder("d", None);
    let e = d.folder("e", Some("1.jpeg"));
    let result = d
        .navigator()
        .find_sibling_image_folders(&c, &CancelToken::none())
        .unwrap();
    assert_eq!(result.previous, Some(a));
    assert_eq!(result.next, Some(e));
}

// C#: ViewerQuickFeaturesTests.FindSiblingImageFolders_TrailingSeparatorOnCurrentFolder_StillFindsSiblings
#[test]
fn find_sibling_image_folders_trailing_separator_on_current_folder_still_finds_siblings() {
    let d = Driver::new("sib-trailing");
    let a = d.folder("a", Some("1.jpg"));
    let b = d.folder("b", Some("1.png"));
    let c = d.folder("c", Some("1.jpeg"));
    let result = d
        .navigator()
        .find_sibling_image_folders(&format!("{b}{}", std::path::MAIN_SEPARATOR), &CancelToken::none())
        .unwrap();
    assert_eq!(result.previous, Some(a));
    assert_eq!(result.next, Some(c));
}

// C#: ViewerQuickFeaturesTests.FindSiblingImageFolders_WebpHeicOnlyFolder_CountsOnlyWhileTheSwitchIsOn
#[test]
fn find_sibling_image_folders_webp_heic_only_folder_counts_only_while_the_switch_is_on() {
    let d = Driver::new("sib-heic");
    d.folder("a", Some("1.jpg"));
    let b = d.folder("b", Some("1.png"));
    let c = d.folder("c", Some("IMG_0001.HEIC"));
    let next = d.folder("d", Some("2.jpg"));
    for (enabled, expected) in [(true, c), (false, next)] {
        let getter: SettingGetter = Arc::new(move || enabled);
        let nav = SiblingFolderNavigator::new(d.clock.clone(), d.fs.clone(), None, Some(getter));
        assert_eq!(
            nav.find_sibling_image_folders(&b, &CancelToken::none()).unwrap().next,
            Some(expected)
        );
    }
}

// C#: ViewerQuickFeaturesTests.FindSiblingImageFolders_CancelledToken_Throws
#[test]
fn find_sibling_image_folders_cancelled_token_throws() {
    let d = Driver::new("sib-cancel");
    let a = d.folder("a", Some("1.jpg"));
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        d.navigator().find_sibling_image_folders(&a, &token),
        Err(SiblingNavError::Cancelled)
    );
}

// C#: SiblingFolderNavigator.NavigateSiblingFolderAsync (null-coalescing session folder)
#[test]
fn begin_navigate_with_empty_session_folder_does_not_fall_back_to_the_catalog() {
    let d = Driver::new("sib-empty-session");
    let b = d.folder("b", Some("1.jpg"));
    let catalog = Path::new(&b).join("1.jpg").to_string_lossy().into_owned();
    assert_eq!(d.navigator().begin_navigate(1, Some(""), Some(&catalog)), Ok(None));
}

// C#: SiblingFolderNavigator.NavigateSiblingFolderAsync (folder generation captured before Task.Run)
#[test]
fn begin_navigate_reads_the_folder_generation_before_the_search() {
    let d = Driver::new("sib-generation");
    let a = d.folder("a", Some("1.jpg"));
    let nav = d.navigator();
    let search = nav.begin_navigate(1, Some(&a), None).unwrap().unwrap();
    assert_eq!(search.folder_generation, d.clock.current_folder());
    d.clock.next_folder();
    assert_ne!(search.folder_generation, d.clock.current_folder());
}

// C#: SiblingFolderNavigator.NavigateSiblingFolderAsync (StatusFormatter.SiblingFolderBoundary)
#[test]
fn complete_navigate_without_target_sets_the_boundary_status() {
    let d = Driver::new("sib-boundary");
    let nav = d.navigator();
    for (direction, expected) in [(1, "Last sibling folder"), (-1, "First sibling folder")] {
        let search = SiblingSearch {
            current_folder: String::new(),
            direction,
            folder_generation: d.clock.current_folder(),
        };
        let mut sink = RecordingSink::default();
        assert_eq!(
            nav.complete_navigate(&search, None, &mut sink, &d.text.context()),
            SiblingNavOutcome::Boundary
        );
        assert_eq!(sink.statuses, vec![expected]);
        assert!(sink.opened.is_empty());
    }
}

// C#: SiblingFolderNavigator.NavigateSiblingFolderAsync (OpenFolderAsync uses default initialPath)
#[test]
fn complete_navigate_with_target_opens_it_with_no_initial_path() {
    let d = Driver::new("sib-open");
    let nav = d.navigator();
    let search = SiblingSearch {
        current_folder: String::new(),
        direction: 1,
        folder_generation: d.clock.current_folder(),
    };
    let mut sink = RecordingSink::default();
    assert_eq!(
        nav.complete_navigate(&search, Some("target"), &mut sink, &d.text.context()),
        SiblingNavOutcome::Opened
    );
    assert_eq!(sink.opened, vec![("target".to_owned(), None)]);
    assert!(sink.statuses.is_empty());
}

// C#: SiblingFolderNavigator.FindImageFolder (the two setting getters are called once per search)
#[test]
fn find_image_folder_reads_the_getters_once_per_search() {
    let d = Driver::new("sib-getters");
    let a = d.folder("a", Some("1.jpg"));
    d.folder("b", None);
    d.folder("c", None);
    let raw_calls = Arc::new(AtomicUsize::new(0));
    let webp_calls = Arc::new(AtomicUsize::new(0));
    let raw = raw_calls.clone();
    let webp = webp_calls.clone();
    let nav = SiblingFolderNavigator::new(
        d.clock.clone(),
        d.fs.clone(),
        Some(Arc::new(move || {
            raw.fetch_add(1, Ordering::SeqCst);
            false
        })),
        Some(Arc::new(move || {
            webp.fetch_add(1, Ordering::SeqCst);
            false
        })),
    );
    let search = nav.begin_navigate(1, Some(&a), None).unwrap().unwrap();
    assert_eq!(nav.find_next_image_folder(&search), Ok(None));
    assert_eq!(raw_calls.load(Ordering::SeqCst), 1);
    assert_eq!(webp_calls.load(Ordering::SeqCst), 1);
}

// C#: SiblingFolderNavigator.FindSiblingImageFolders (current folder absent from sorted siblings)
#[test]
fn find_sibling_image_folders_current_folder_not_in_list_gives_default() {
    let d = Driver::new("sib-missing");
    d.folder("a", Some("1.jpg"));
    // The parent does not exist, so the sorted list is empty and the folder is "not in list".
    let missing = d.root.join("no-parent").join("missing").to_string_lossy().into_owned();
    let result = d
        .navigator()
        .find_sibling_image_folders(&missing, &CancelToken::none())
        .unwrap();
    assert_eq!(result.previous, None);
    assert_eq!(result.next, None);
}

// C#: SiblingFolderNavigator.FindSiblingImageFolders (ThrowIfNullOrWhiteSpace)
#[test]
fn find_sibling_image_folders_blank_argument_is_an_error() {
    let d = Driver::new("sib-blank");
    for blank in ["", " \t "] {
        assert_eq!(
            d.navigator().find_sibling_image_folders(blank, &CancelToken::none()),
            Err(SiblingNavError::BlankFolder)
        );
    }
}

// C#: SiblingFolderNavigator.FindSiblingImageFolders (GetSorted precedes cancellation check)
#[test]
fn find_sibling_image_folders_listing_precedes_the_cancel_check() {
    let mut d = Driver::new("sib-list-before-cancel");
    let a = d.folder("a", Some("1.jpg"));
    d.folder("b", Some("1.jpg"));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    d.fs = Arc::new(HookedFileSystem::new(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        d.navigator().find_sibling_image_folders(&a, &token),
        Err(SiblingNavError::Cancelled)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// New test: T3, index lookup is ordinal ignoring case (Windows paths are case-insensitive)
#[cfg(windows)]
#[test]
fn find_sibling_image_folders_current_folder_differing_only_in_case_is_found() {
    let d = Driver::new("sib-case");
    let a = d.folder("a", Some("1.jpg"));
    let b = d.folder("b", Some("1.jpg"));
    let result = d
        .navigator()
        .find_sibling_image_folders(&a.to_uppercase(), &CancelToken::none())
        .unwrap();
    assert_eq!(result.previous, None);
    assert!(pr_core::text::eq_ignore_case(result.next.as_deref().unwrap(), &b));
}

// New test: T6, the token is checked before every sibling probed, not only after the listing
#[test]
fn find_sibling_image_folders_cancel_during_the_probe_stops_before_the_next_sibling() {
    let mut d = Driver::new("sib-cancel-mid");
    let a = d.folder("a", Some("1.jpg"));
    let b = d.folder("b", None);
    d.folder("c", Some("1.jpg"));
    let token = CancelToken::new();
    let hook_token = token.clone();
    d.fs = Arc::new(HookedFileSystem::new(move |path| {
        if path == Path::new(&b) {
            hook_token.cancel();
        }
        Ok(())
    }));
    assert_eq!(
        d.navigator().find_sibling_image_folders(&a, &token),
        Err(SiblingNavError::Cancelled)
    );
}

// New test: T5, a direction other than +-1 handed straight to the worker step cannot spin
#[test]
fn find_next_image_folder_rejects_direction_zero() {
    let d = Driver::new("sib-worker-direction");
    let a = d.folder("a", None);
    d.folder("b", None);
    let search = SiblingSearch {
        current_folder: a,
        direction: 0,
        folder_generation: d.clock.current_folder(),
    };
    assert_eq!(
        d.navigator().find_next_image_folder(&search),
        Err(SiblingNavError::InvalidDirection(0))
    );
}
