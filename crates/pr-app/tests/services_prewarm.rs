//! Tests of `pr_app::services::prewarm` with a fake `PrewarmPreviews` (contract `contracts/W8-04c.md`, rules R-PW-0..6, trap T-10).
//!
//! Port of `InitialImagePrewarmTests.cs` (the tests that need no real service). The fake answers every key with an error, so no
//! `PreviewHandle` is needed: a `PreviewHandle` has no public constructor. The tests that need a decode to start (the viewer
//! decode with the real service) are in `services_prewarm_service.rs`.

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use pr_app::services::prewarm::{PrewarmPreviews, start};
use pr_core::settings::app_settings::AppSettings;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::service::handle::PreviewHandle;

/// One call of `current_cache_key`, with the thread it ran on (T-10).
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyCall {
    path: String,
    target_box: Option<DecodeBox>,
    thread: ThreadId,
}

/// Fake `PrewarmPreviews`: records the key calls, answers with a key error (so no decode starts), and can panic on purpose.
#[derive(Default)]
struct FakePreviews {
    key_calls: Mutex<Vec<KeyCall>>,
    panic_on_key: bool,
}

impl FakePreviews {
    fn panicking() -> Self {
        Self {
            panic_on_key: true,
            ..Self::default()
        }
    }

    fn key_calls(&self) -> Vec<KeyCall> {
        self.key_calls.lock().expect("fake lock").clone()
    }
}

impl PrewarmPreviews for FakePreviews {
    fn current_cache_key(
        &self,
        full_path: &str,
        target_box: Option<DecodeBox>,
    ) -> Result<ImageCacheKey, CacheKeyError> {
        self.key_calls.lock().expect("fake lock").push(KeyCall {
            path: full_path.to_owned(),
            target_box,
            thread: std::thread::current().id(),
        });
        if self.panic_on_key {
            panic!("fake key panic");
        }
        Err(CacheKeyError::SourceNoLongerExists {
            full_name: full_path.to_owned(),
        })
    }

    fn start_viewer_preview(&self, _full_path: &str, _key: &ImageCacheKey) -> PreviewHandle {
        // Unreachable by construction: `current_cache_key` never returns a key, so the decode never starts in these tests.
        unreachable!("a fake key error must end the prewarm before any decode starts")
    }
}

/// Coerces the fake to the trait object `start` takes.
fn as_previews(fake: &Arc<FakePreviews>) -> Option<Arc<dyn PrewarmPreviews>> {
    let previews: Arc<dyn PrewarmPreviews> = fake.clone();
    Some(previews)
}

// C#: InitialImagePrewarmTests.Start_WithoutService_StartsNothing
#[test]
fn start_without_service_starts_nothing() {
    let started = start(None, &AppSettings::default(), Some("C:/photos/a.jpg"), None);
    assert!(started.is_none());
}

// C#: InitialImagePrewarmTests.Start_BlankPath_StartsNothing (Theory: null, ``, `   `)
#[test]
fn start_blank_path_starts_nothing() {
    let fake = Arc::new(FakePreviews::default());
    for path in [None, Some(""), Some("   ")] {
        let started = start(as_previews(&fake), &AppSettings::default(), path, None);
        assert!(started.is_none(), "path {path:?}");
    }
    assert!(fake.key_calls().is_empty(), "nothing is keyed for a blank path");
}

// C#: InitialImagePrewarmTests.Start_UnsupportedOrRawFile_StartsNothing (Theory: `notes.txt`, `shot.cr2`, `shot.NEF`)
#[test]
fn start_unsupported_or_raw_file_starts_nothing() {
    let fake = Arc::new(FakePreviews::default());
    for name in ["notes.txt", "shot.cr2", "shot.NEF"] {
        let started = start(as_previews(&fake), &AppSettings::default(), Some(name), None);
        assert!(started.is_none(), "file {name}");
    }
    assert!(
        fake.key_calls().is_empty(),
        "no key for an unsupported or RAW file (R-PW-3)"
    );
}

// Contract test (R-PW-5 a): without an explicit box the key is asked with `None` (the service's own viewport box).
// The decode start that follows a good key is covered by `services_prewarm_service.rs` (real service).
#[test]
fn start_without_box_asks_the_key_with_no_box() {
    let fake = Arc::new(FakePreviews::default());

    let task = start(as_previews(&fake), &AppSettings::default(), Some("launch.jpg"), None)
        .expect("a supported file is prewarmed");
    task.wait().expect("a key error ends the thread normally");

    let calls = fake.key_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_box, None);
    assert!(
        calls[0].path.ends_with("launch.jpg"),
        "stable tail of the full path, got {}",
        calls[0].path
    );
}

// Contract test (R-PW-5 a): an explicit box is passed to the key unchanged.
// C#: InitialImagePrewarmTests.Start_WithExplicitBox_DecodesAndCachesUnderThatBox (the key-building part, at fake level).
#[test]
fn start_with_explicit_box_passes_it_to_the_key() {
    let fake = Arc::new(FakePreviews::default());
    let predicted = DecodeBox::new(1280, 768);

    let task = start(
        as_previews(&fake),
        &AppSettings::default(),
        Some("launch.jpg"),
        Some(predicted),
    )
    .expect("a supported file is prewarmed");
    task.wait().expect("a key error ends the thread normally");

    let calls = fake.key_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].target_box, Some(predicted));
}

// Contract test (R-PW-5 b): a key error ends the thread silently; `wait` is `Ok` and no decode starts.
#[test]
fn start_key_error_ends_without_decoding() {
    let fake = Arc::new(FakePreviews::default());

    let task = start(as_previews(&fake), &AppSettings::default(), Some("launch.jpg"), None)
        .expect("a supported file is prewarmed");

    assert!(task.wait().is_ok(), "a key error is not a panic");
    assert_eq!(
        fake.key_calls().len(),
        1,
        "the key was asked once, and nothing else ran"
    );
}

// Contract test (T-10): the key is computed on the prewarm thread, never on the calling thread. No timing: the thread ids are compared.
#[test]
fn key_is_computed_off_the_calling_thread() {
    let fake = Arc::new(FakePreviews::default());

    let task = start(as_previews(&fake), &AppSettings::default(), Some("launch.jpg"), None)
        .expect("a supported file is prewarmed");
    task.wait().expect("a key error ends the thread normally");

    let calls = fake.key_calls();
    assert_eq!(calls.len(), 1);
    assert_ne!(
        calls[0].thread,
        std::thread::current().id(),
        "the key ran on the prewarm thread"
    );
}

// Contract test (R-PW-6): a path with a NUL character makes `GetFullPath` fail; nothing starts and the key is never asked.
#[test]
fn start_with_full_path_rejected_starts_nothing() {
    let fake = Arc::new(FakePreviews::default());

    let started = start(
        as_previews(&fake),
        &AppSettings::default(),
        Some("photo\0name.jpg"),
        None,
    );

    assert!(started.is_none());
    assert!(fake.key_calls().is_empty());
}

// Contract test (R-PW-5 c, R-PW-5 panic): a panicking prewarm is reported to the caller of `wait` only, as `Err`.
#[test]
fn wait_reports_a_panicking_prewarm_as_err() {
    let fake = Arc::new(FakePreviews::panicking());

    let task = start(as_previews(&fake), &AppSettings::default(), Some("launch.jpg"), None)
        .expect("a supported file is prewarmed");

    assert!(task.wait().is_err(), "the panic is reported to wait, not propagated");
}
