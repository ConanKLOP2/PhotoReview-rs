use std::sync::Arc;
use std::sync::Mutex;

use pr_app::presenters::rename_prompt::{NameTaken, RenameCheck, RenamePromptPresenter};
use pr_core::abstractions::{AppendStream, FileStat, FileSystem, ReadSeek};
use pr_core::file_actions::rename::RenameNamePolicy;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}
mod presenters_support;
use support::main_env::MainEnv;

const SOURCE: &str = r"C:\Photos\IMG_0042.JPG";
static LOCALIZER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct ExistsCounter {
    calls: std::sync::atomic::AtomicUsize,
    exists: std::sync::atomic::AtomicBool,
}
impl FileSystem for ExistsCounter {
    fn file_exists(&self, _: &std::path::Path) -> bool {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.exists.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn directory_exists(&self, _: &std::path::Path) -> bool {
        false
    }
    fn get_file_stat(&self, _: &std::path::Path) -> Option<FileStat> {
        None
    }
    fn move_file(&self, _: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn copy_file(&self, _: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn delete(&self, _: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn open_read_shared(&self, _: &std::path::Path, _: usize) -> std::io::Result<Box<dyn ReadSeek>> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn open_append(&self, _: &std::path::Path, _: bool) -> std::io::Result<Box<dyn AppendStream>> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn write_all_text_atomic(&self, _: &std::path::Path, _: &str, _: bool) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn read_all_text(&self, _: &std::path::Path) -> std::io::Result<String> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn enumerate_files(&self, _: &std::path::Path, _: &str) -> std::io::Result<Vec<std::path::PathBuf>> {
        Ok(Vec::new())
    }
    fn enumerate_directories(&self, _: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        Ok(Vec::new())
    }
    fn create_directory(&self, _: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }
}

fn presenter(taken: Option<NameTaken>) -> RenamePromptPresenter {
    let check: RenameCheck = Arc::new(|typed| RenameNamePolicy::check(Some(typed), SOURCE, false));
    RenamePromptPresenter::new(SOURCE.to_owned(), check, taken).expect("nonblank source")
}
fn english_locale() {
    Localizer::set_current(built_in_english_localizer());
}

#[test]
fn initial_is_the_stem_without_the_extension_and_the_extension_keeps_its_case() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: RenamePromptPresenterTests.Initial_IsTheStemWithoutTheExtension_AndTheExtensionKeepsItsCase
    let value = presenter(None);
    assert_eq!(value.initial_text(), "IMG_0042");
    assert_eq!(value.extension(), ".JPG");
    let request = value.to_request();
    assert_eq!(request.initial_name, "IMG_0042");
    assert_eq!(request.extension, ".JPG");
}

#[test]
fn a_dotted_stem_and_an_extensionless_file_are_split_like_explorer() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: RenamePromptPresenterTests.ADottedStem_AndAnExtensionlessFile_AreSplitLikeExplorer
    let dotted = RenamePromptPresenter::new(
        r"C:\p\a.v2.final.png".into(),
        Arc::new(|_| RenameNamePolicy::check(Some("x"), "C:\\p\\a.v2.final.png", false)),
        None,
    )
    .expect("source");
    let extensionless = RenamePromptPresenter::new(
        "C:\\p\\README".into(),
        Arc::new(|_| RenameNamePolicy::check(Some("x"), "C:\\p\\README", false)),
        None,
    )
    .expect("source");
    assert_eq!(dotted.initial_text(), "a.v2.final");
    assert_eq!(dotted.extension(), ".png");
    assert_eq!(extensionless.initial_text(), "README");
    assert_eq!(extensionless.extension(), "");
}

#[test]
fn every_policy_problem_maps_to_its_own_localized_text() {
    // C#: RenamePromptPresenterTests.EveryPolicyProblem_MapsToItsOwnLocalizedText
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    let loc = Localizer::current();
    let long_path = format!(r"C:\{}\IMG_0042.JPG", "d".repeat(230));
    for (input, key, source) in [
        (String::new(), "dialog.rename.error.empty", SOURCE.to_owned()),
        (
            "a?b".to_owned(),
            "dialog.rename.error.invalidCharacter",
            SOURCE.to_owned(),
        ),
        ("a.".to_owned(), "dialog.rename.error.trailingDot", SOURCE.to_owned()),
        ("CON".to_owned(), "dialog.rename.error.reservedName", SOURCE.to_owned()),
        ("x".repeat(256), "dialog.rename.error.tooLong", SOURCE.to_owned()),
        ("x".repeat(40), "dialog.rename.error.pathTooLong", long_path),
    ] {
        let expected = loc.get(key);
        let check_path = source.clone();
        let item = RenamePromptPresenter::new(
            source,
            Arc::new(move |typed| RenameNamePolicy::check(Some(typed), &check_path, false)),
            None,
        )
        .expect("source");
        assert_eq!(item.try_accept(Some(&input), false), Some(expected));
    }
}

#[test]
fn too_long_and_path_too_long_are_refused() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: RenamePromptPresenterTests.TooLongAndPathTooLong_AreRefused
    let long_path = format!(r"C:\{}\IMG_0042.JPG", "d".repeat(230));
    let deep = RenamePromptPresenter::new(
        long_path.clone(),
        Arc::new(move |typed| RenameNamePolicy::check(Some(typed), &long_path, false)),
        None,
    )
    .expect("path");
    assert!(presenter(None).try_accept(Some(&"x".repeat(300)), false).is_some());
    assert!(deep.try_accept(Some(&"x".repeat(40)), false).is_some());
    assert_eq!(deep.try_accept(Some("IMG_0042"), false), None);
}

#[test]
fn not_refused_the_same_name_a_change_of_case_only_and_ordinary_names() {
    // C#: RenamePromptPresenterTests.NotRefused_TheSameName_AChangeOfCaseOnly_AndOrdinaryNames
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&calls);
    let value = presenter(Some(Arc::new(move |name, _| {
        seen.lock().unwrap().push(name.to_owned());
        true
    })));
    assert_eq!(value.try_accept(Some("IMG_0042"), false), None);
    assert_eq!(value.try_accept(Some("img_0042"), false), None);
    assert!(calls.lock().unwrap().is_empty());
    let error = value.try_accept(Some("other"), false);
    let expected = Localizer::current().format(
        "dialog.rename.error.exists",
        &[pr_l10n::template::LocArg::new("name", "other.JPG")],
    );
    assert_eq!(error, Some(expected));
    assert_eq!(*calls.lock().unwrap(), ["other.JPG"]);
}

#[test]
fn a_refused_name_is_never_looked_up() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: RenamePromptPresenterTests.ARefusedNameIsNeverLookedUp
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let value = presenter(Some(Arc::new(move |_, _| {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    })));
    assert!(value.try_accept(Some("a?b"), false).is_some());
    assert!(value.try_accept(None, false).is_some());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn without_a_taken_question_nothing_is_taken() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: RenamePromptPresenterTests.WithoutATakenQuestion_NothingIsTaken
    assert_eq!(presenter(None).try_accept(Some("anything goes"), false), None);
}

#[test]
fn text_is_localized_in_vietnamese_too() {
    // C#: RenamePromptPresenterTests.TextIsLocalized_InVietnameseToo
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    let original = Localizer::current();
    Localizer::set_current(presenters_support::vietnamese());
    let error = presenter(None).try_accept(Some(""), false).expect("localized error");
    assert!(error.to_lowercase().contains("trống"));
    Localizer::set_current(original);
}

#[test]
fn the_capture_group_check_can_be_injected_for_the_paired_jpeg_and_raw() {
    // C#: RenamePromptPresenterTests.TheCaptureGroupCheck_CanBeInjected_ForThePairedJpegAndRaw
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    use pr_core::file_actions::rename::{RenameNameCheck, RenameNameProblem};
    let value = RenamePromptPresenter::new(
        SOURCE.to_owned(),
        Arc::new(|_| RenameNameCheck {
            problem: RenameNameProblem::SidecarNotMappable,
            new_file_name: String::new(),
            invalid_character: None,
            is_unchanged: false,
            differs_only_by_case: false,
        }),
        None,
    )
    .expect("source");
    assert_eq!(
        value.try_accept(Some("fresh"), false),
        Some(Localizer::current().get("dialog.rename.error.sidecarNotMappable"))
    );
}

#[test]
fn prepared_request_queries_live_catalog_and_disk_only_on_commit() {
    // C#: MainViewModelRenameTests.PromptRenameCurrentAsync_ValidateFollowsThePolicy_AndTypingNeverTouchesTheDisk
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    use pr_app::presenters::rename_prompt::PreparedRenamePrompt;
    use std::sync::atomic::{AtomicBool, Ordering};
    let check: RenameCheck = Arc::new(|typed| RenameNamePolicy::check(Some(typed), SOURCE, false));
    let plan = PreparedRenamePrompt::new(SOURCE.to_owned(), check);
    let fs = Arc::new(ExistsCounter::default());
    let listed = Arc::new(AtomicBool::new(false));
    let listing_state = Arc::clone(&listed);
    let listing = Arc::new(move |candidate: &str| {
        assert_eq!(candidate, r"C:\Photos\late.JPG");
        listing_state.load(Ordering::SeqCst)
    });
    let request = plan
        .request(listing, fs.clone() as Arc<dyn FileSystem>)
        .expect("valid source");
    assert_eq!((request.validate)("late", false), None);
    assert_eq!(
        fs.calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "typing never probes the filesystem"
    );
    fs.exists.store(true, Ordering::SeqCst);
    let disk_error = (request.validate)("late", true);
    assert_eq!(
        disk_error,
        Some(Localizer::current().format(
            "dialog.rename.error.exists",
            &[pr_l10n::template::LocArg::new("name", "late.JPG")],
        ))
    );
    assert_eq!(
        fs.calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "commit probes once"
    );
    listed.store(true, Ordering::SeqCst);
    fs.exists.store(false, Ordering::SeqCst);
    fs.calls.store(0, Ordering::SeqCst);
    assert!((request.validate)("late", false).is_some());
    assert_eq!(
        fs.calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a live catalog hit short-circuits disk"
    );
}

#[test]
fn blank_source_is_rejected_without_panicking() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: RenamePromptPresenter constructor ArgumentException.ThrowIfNullOrWhiteSpace.
    let check: RenameCheck = Arc::new(|typed| RenameNamePolicy::check(Some(typed), SOURCE, false));
    assert!(RenamePromptPresenter::new("  ".to_owned(), check, None).is_err());
}

#[test]
fn prompt_rename_current_the_viewer_moved_on_while_the_box_was_open_renames_nothing() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    // C#: MainViewModelRenameTests.PromptRenameCurrentAsync_TheViewerMovedOnWhileTheBoxWasOpen_RenamesNothing
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_prompt_moved", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open_at(&folder, &files[0]);
    env.vm.prompt_rename_current();
    let plan = env.vm.take_rename_prompt().expect("queued plan");
    let source = plan.source().to_owned();
    env.vm.next();
    env.settle();
    env.vm.finish_rename_prompt(&source, Some("moved"));
    assert!(std::path::Path::new(&files[0]).exists());
    assert!(!std::path::Path::new(&folder).join("moved.jpg").exists());
}

#[test]
fn prompt_rename_current_with_no_catalog_does_not_queue_a_prompt() {
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    let mut env = MainEnv::new();
    env.vm.prompt_rename_current();
    assert!(env.vm.take_rename_prompt().is_none());
}

#[test]
fn the_taken_question_gets_the_complete_new_file_name_and_knows_whether_it_is_the_confirming_call() {
    // C#: RenamePromptPresenterTests.TheTakenQuestion_GetsTheCompleteNewFileName_AndKnowsWhetherItIsTheConfirmingCall
    let _localizer = LOCALIZER_LOCK.lock().unwrap();
    english_locale();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&calls);
    let value = presenter(Some(Arc::new(move |name, commit| {
        seen.lock().unwrap().push((name.to_owned(), commit));
        false
    })));
    assert_eq!(value.try_accept(Some("holiday"), false), None);
    assert_eq!(value.try_accept(Some("holiday"), true), None);
    assert_eq!(
        *calls.lock().unwrap(),
        [("holiday.JPG".to_owned(), false), ("holiday.JPG".to_owned(), true)]
    );
}
