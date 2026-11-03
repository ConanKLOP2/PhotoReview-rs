//! INV-1 ("an OLD preview never overwrites the NEW one") at the sink level, third and last part of the port of
//! `PhotoReview.App.Tests/Coordinators/StalePreviewOverwriteTests.cs` (baseline 7eaee2a2; lane g20-stale), next to `g19_stale_preview.rs` and
//! `g19_stale_preview2.rs`: the five C# tests those two files left out (12 of 17 were ported):
//! `Burst_TwoFiles_EveryThumbnailAndPreviewOrder_...`, `Burst_ThreeFilesWithThumbnails_SeededSampleOfOrders_...`,
//! `JoinedPreloadDecodes_FinishingInReverseOrder_...`, `PostPresentStat_OfASupersededNavigation_FinishingLate_...` and
//! `HeaderReadFinishingLate_OfASupersededNavigation_...`.
//!
//! Every decode, thumbnail read, header read and refresh stat is a gate the test opens; every wait is bounded and gate- or probe-driven (no sleeps). The
//! sink log is checked after every step: every image and status that reaches it must belong to the navigation that is current at that moment.
//! Fakes only: the folder is a temp root with tiny files; nothing touches the Recycle Bin.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_core::abstractions::FileStat;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::LoadingMode;
use pr_core::sync::CancelToken;
use pr_imaging::cache::thumbnails::EmbeddedThumbnailReaderFn;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::service::PreviewImage;
use support::present_helpers::{ScriptedFs, images_shown, presented_paths, pump_one, start, statuses};
use support::presenter_fakes::{FixtureOptions, PresenterFixture, build_thumbnail_cache_with};
use support::preview_fakes::{Gate, ServiceOptions, TIMEOUT, build_service_full, lock, sized_image};

const NAMES: [&str; 3] = ["g20-a.jpg", "g20-b.jpg", "g20-c.jpg"];

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn real_stat(path: &str) -> FileStat {
    use pr_core::abstractions::FileSystem;
    PhysicalFileSystem
        .get_file_stat(Path::new(path))
        .unwrap_or_else(|| panic!("{path} has no stat"))
}

fn index_of(file: usize) -> i32 {
    i32::try_from(file).unwrap_or_else(|error| panic!("index {file}: {error}"))
}

// ---- the decoder ----------------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct DecodeLog {
    started: Vec<String>,
    finished: Vec<String>,
}

/// C# `StalePreviewHarness.GatedDecoder`: the decode of each file blocks on that file's gate (opened by the test); each file has its own preview image (so the
/// sink can tell whose image it got); the header read of a HELD file signals and blocks until released. "started" and "finished" are observable.
/// (header read started, header read released)
type HeldInfo = (Arc<Gate>, Arc<Gate>);

#[derive(Default)]
struct StaleDecoder {
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    images: Mutex<HashMap<String, PreviewImage>>,
    log: Mutex<DecodeLog>,
    changed: Condvar,
    timed_out: Mutex<Vec<String>>,
    held_info: Mutex<HashMap<String, HeldInfo>>,
    info_started: Mutex<HashSet<String>>,
}

impl std::fmt::Debug for StaleDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaleDecoder").finish_non_exhaustive()
    }
}

impl StaleDecoder {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn gate(&self, name: &str) -> Arc<Gate> {
        Arc::clone(lock(&self.gates).entry(name.to_lowercase()).or_default())
    }

    fn open(&self, name: &str) {
        self.gate(name).open();
    }

    /// The preview image the decode of `name` returns (one per file, registered before the first decode).
    fn preview_of(&self, name: &str) -> PreviewImage {
        Arc::clone(
            lock(&self.images)
                .entry(name.to_lowercase())
                .or_insert_with(|| sized_image(100, 100, 100, 100, false)),
        )
    }

    fn has_started(&self, name: &str) -> bool {
        lock(&self.log).started.contains(&name.to_lowercase())
    }

    /// Waits until the decode of `name` entered the decoder; `false` when [`TIMEOUT`] passes first.
    fn wait_started(&self, name: &str) -> bool {
        let key = name.to_lowercase();
        let guard = lock(&self.log);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |log| !log.started.contains(&key))
            .unwrap_or_else(PoisonError::into_inner);
        guard.started.contains(&key)
    }

    fn count_of(list: &[String], name: &str) -> usize {
        list.iter().filter(|entry| entry.as_str() == name).count()
    }

    fn started_count(&self, name: &str) -> usize {
        Self::count_of(&lock(&self.log).started, &name.to_lowercase())
    }

    /// Waits until every decode of `name` that has started also finished (a decode that has not started is not waited for).
    fn wait_finished_if_started(&self, name: &str) -> bool {
        let key = name.to_lowercase();
        let guard = lock(&self.log);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |log| {
                Self::count_of(&log.finished, &key) < Self::count_of(&log.started, &key)
            })
            .unwrap_or_else(PoisonError::into_inner);
        Self::count_of(&guard.finished, &key) >= Self::count_of(&guard.started, &key)
    }

    /// True when no decode is running (every started decode finished).
    fn is_quiet(&self) -> bool {
        let log = lock(&self.log);
        log.started.len() == log.finished.len()
    }

    fn timed_out(&self) -> Vec<String> {
        lock(&self.timed_out).clone()
    }

    /// The header read of `name` signals `info_started` and blocks until `release_info`.
    fn hold_info(&self, name: &str) {
        lock(&self.held_info).insert(name.to_lowercase(), (Gate::new(), Gate::new()));
    }

    fn held(&self, name: &str) -> Option<HeldInfo> {
        lock(&self.held_info).get(&name.to_lowercase()).cloned()
    }

    fn info_started(&self, name: &str) -> bool {
        lock(&self.info_started).contains(&name.to_lowercase())
    }

    fn release_info(&self, name: &str) {
        if let Some((_, release)) = self.held(name) {
            release.open();
        }
    }
}

impl ImageDecoder for StaleDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let name = name_of(&request.path);
        lock(&self.log).started.push(name.clone());
        self.changed.notify_all();
        let opened = self.gate(&name).wait();
        let result: Result<Arc<dyn DecodedImageView>, DecodeError> = if opened {
            Ok(self.preview_of(&name))
        } else {
            lock(&self.timed_out).push(name.clone());
            Err(DecodeError::InvalidData {
                message: format!("decode gate of {name} was never opened"),
            })
        };
        lock(&self.log).finished.push(name);
        self.changed.notify_all();
        result
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        let name = name_of(Path::new(path));
        if let Some((started, release)) = self.held(&name) {
            lock(&self.info_started).insert(name);
            started.open();
            if !release.wait() {
                return Err(DecodeError::InvalidData {
                    message: format!("header gate of {path} was never opened"),
                });
            }
        }
        Ok(ImageInfo::new(100, 100))
    }
}

// ---- the thumbnail reader -------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ThumbLog {
    started: Vec<String>,
    returned: Vec<String>,
}

/// C# `StalePreviewHarness` thumbnail gates: the embedded-thumbnail read of each file blocks on that file's gate and then returns that file's own thumbnail.
#[derive(Default)]
struct ThumbGates {
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    images: Mutex<HashMap<String, PreviewImage>>,
    log: Mutex<ThumbLog>,
    changed: Condvar,
}

impl std::fmt::Debug for ThumbGates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThumbGates").finish_non_exhaustive()
    }
}

impl ThumbGates {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn gate(&self, name: &str) -> Arc<Gate> {
        Arc::clone(lock(&self.gates).entry(name.to_lowercase()).or_default())
    }

    fn open(&self, name: &str) {
        self.gate(name).open();
    }

    fn thumbnail_of(&self, name: &str) -> PreviewImage {
        Arc::clone(
            lock(&self.images)
                .entry(name.to_lowercase())
                .or_insert_with(|| sized_image(40, 30, 100, 100, true)),
        )
    }

    /// Waits until a read of `name` that has started also returned.
    fn wait_returned_if_started(&self, name: &str) -> bool {
        let key = name.to_lowercase();
        let guard = lock(&self.log);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |log| {
                StaleDecoder::count_of(&log.returned, &key) < StaleDecoder::count_of(&log.started, &key)
            })
            .unwrap_or_else(PoisonError::into_inner);
        StaleDecoder::count_of(&guard.returned, &key) >= StaleDecoder::count_of(&guard.started, &key)
    }

    fn is_quiet(&self) -> bool {
        let log = lock(&self.log);
        log.started.len() == log.returned.len()
    }

    fn reader(self: &Arc<Self>) -> EmbeddedThumbnailReaderFn {
        let gates = Arc::clone(self);
        Arc::new(move |path: &str, _cancel: &CancelToken| {
            let name = name_of(Path::new(path));
            lock(&gates.log).started.push(name.clone());
            gates.changed.notify_all();
            let opened = gates.gate(&name).wait();
            assert!(opened, "the thumbnail gate of {name} was never opened");
            let image = gates.thumbnail_of(&name);
            lock(&gates.log).returned.push(name);
            gates.changed.notify_all();
            Ok(Some(image))
        })
    }
}

// ---- the orders -----------------------------------------------------------------------------------------------------

/// C# `Op`: a navigation (by ordinal), or the end of the preview / thumbnail read of a file (by file index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Op {
    Navigate(usize),
    Preview(usize),
    Thumbnail(usize),
}

/// C# `Orders(targets, thumbnails)`: every order in which the navigations of `targets` are issued (in sequence) and the preview (and, with `thumbnails`,
/// thumbnail) of each file finishes, a file finishing only after its first navigation.
fn orders(targets: &[usize], thumbnails: bool) -> Vec<Vec<Op>> {
    struct Walk<'a> {
        targets: &'a [usize],
        thumbnails: bool,
        total: usize,
        result: Vec<Vec<Op>>,
        current: Vec<Op>,
    }

    fn walk(state: &mut Walk<'_>, issued: usize, navigated: &mut BTreeSet<usize>, released: &mut HashSet<Op>) {
        if state.current.len() == state.total {
            state.result.push(state.current.clone());
            return;
        }
        if issued < state.targets.len() {
            state.current.push(Op::Navigate(issued));
            let added = navigated.insert(state.targets[issued]);
            walk(state, issued + 1, navigated, released);
            if added {
                navigated.remove(&state.targets[issued]);
            }
            state.current.pop();
        }
        let files: Vec<usize> = navigated.iter().copied().collect();
        for file in files {
            let kinds: &[fn(usize) -> Op] = if state.thumbnails {
                &[Op::Thumbnail, Op::Preview]
            } else {
                &[Op::Preview]
            };
            for kind in kinds {
                let op = kind(file);
                if !released.insert(op) {
                    continue;
                }
                state.current.push(op);
                walk(state, issued, navigated, released);
                state.current.pop();
                released.remove(&op);
            }
        }
    }

    let files: BTreeSet<usize> = targets.iter().copied().collect();
    let per_file = if thumbnails { 2 } else { 1 };
    let mut state = Walk {
        targets,
        thumbnails,
        total: targets.len() + files.len() * per_file,
        result: Vec::new(),
        current: Vec::new(),
    };
    walk(&mut state, 0, &mut BTreeSet::new(), &mut HashSet::new());
    state.result
}

/// Fixed-seed Fisher-Yates over xorshift64 (the C# `new Random(20261012)` cannot be reproduced; the sample only has to be fixed).
fn seeded_sample(mut all: Vec<Vec<Op>>, seed: u64, count: usize) -> Vec<Vec<Op>> {
    let mut state = seed | 1;
    for position in (1..all.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let other = usize::try_from(state % (position as u64 + 1)).unwrap_or(0);
        all.swap(position, other);
    }
    all.truncate(count);
    all
}

// ---- the run of one order -------------------------------------------------------------------------------------------

struct World {
    decoder: Arc<StaleDecoder>,
    thumbs: Arc<ThumbGates>,
    fixture: PresenterFixture,
    paths: Vec<String>,
    _scratch: pr_test_support::TempRoot,
}

fn world(with_thumbnails: bool) -> World {
    let decoder = StaleDecoder::new();
    let thumbs = ThumbGates::new();
    for name in NAMES {
        decoder.preview_of(name);
        thumbs.thumbnail_of(name);
    }
    let scratch = pr_test_support::TempRoot::new("g20-stale-thumbs");
    let options = if with_thumbnails {
        FixtureOptions {
            thumbnails: Some(build_thumbnail_cache_with(&scratch.join("thumbs"), thumbs.reader())),
            ..FixtureOptions::default()
        }
    } else {
        FixtureOptions::default()
    };
    let mut fixture = PresenterFixture::with_options(Arc::clone(&decoder) as _, &NAMES, options);
    fixture.settings.loading_mode = Some(LoadingMode::Preview);
    let paths = NAMES.iter().map(|name| fixture.path(name)).collect();
    World {
        decoder,
        thumbs,
        fixture,
        paths,
        _scratch: scratch,
    }
}

/// What of the sink log was already checked.
#[derive(Default)]
struct Checked {
    images: usize,
    statuses: usize,
}

impl World {
    /// Which file an image the sink got belongs to (its preview or its thumbnail); an unknown image fails the test (the check would be blind).
    fn owner_of(&self, image: &PlatformImage, scenario: &str) -> usize {
        for (file, name) in NAMES.iter().enumerate() {
            if same_platform_image(image, self.decoder.preview_of(name).platform_image())
                || same_platform_image(image, self.thumbs.thumbnail_of(name).platform_image())
            {
                return file;
            }
        }
        panic!(
            "{scenario}: the sink got an image that is neither a preview nor a thumbnail of the fixture (the stale check would be blind)"
        );
    }

    /// C# `AssertNoStaleOverwrite`: everything that reached the sink since the last check belongs to the navigation that is current now.
    fn assert_new_events_belong_to(&self, checked: &mut Checked, current: usize, scenario: &str) {
        let log = lock(&self.fixture.sink_log);
        for call in &log.images[checked.images..] {
            if let Some(image) = &call.image {
                assert_eq!(
                    self.owner_of(image, scenario),
                    current,
                    "{scenario}: an image of an older navigation reached the sink"
                );
            }
        }
        for status in &log.statuses[checked.statuses..] {
            for (file, name) in NAMES.iter().enumerate() {
                assert!(
                    file == current || !status.contains(name),
                    "{scenario}: a status of the older navigation {name} reached the sink: {status}"
                );
            }
        }
        checked.images = log.images.len();
        checked.statuses = log.statuses.len();
    }
}

/// C# `RunOrderAsync` (with every decode of the first two navigations really inside the decoder when it is superseded, as C# does with thumbnails).
fn run_order(targets: &[usize], ops: &[Op]) {
    let scenario = format!("targets {targets:?}, order {ops:?}");
    let mut w = world(true);
    let mut checked = Checked::default();
    let mut issued = 0;
    for op in ops {
        match *op {
            Op::Navigate(ordinal) => {
                let file = targets[ordinal];
                start(&mut w.fixture, index_of(file));
                issued = ordinal + 1;
                // At most the two viewer slots can be occupied by decodes that were superseded while running.
                if ordinal < 2 {
                    let decoder = Arc::clone(&w.decoder);
                    w.fixture.pump_until(|_| decoder.has_started(NAMES[file]));
                }
            }
            Op::Preview(file) => {
                w.decoder.open(NAMES[file]);
                assert!(
                    w.decoder.wait_finished_if_started(NAMES[file]),
                    "{scenario}: decode of {file} never finished"
                );
                w.fixture.pump();
            }
            Op::Thumbnail(file) => {
                w.thumbs.open(NAMES[file]);
                assert!(
                    w.thumbs.wait_returned_if_started(NAMES[file]),
                    "{scenario}: thumbnail of {file} never returned"
                );
                w.fixture.pump();
            }
        }
        w.assert_new_events_belong_to(&mut checked, targets[issued - 1], &scenario);
    }

    // C# `SettleAsync`: open everything still closed and wait until the presenter ended.
    for name in NAMES {
        w.decoder.open(name);
        w.thumbs.open(name);
    }
    let (decoder, thumbs) = (Arc::clone(&w.decoder), Arc::clone(&w.thumbs));
    w.fixture.pump_until(|_| decoder.is_quiet() && thumbs.is_quiet());
    w.fixture.pump_until_idle();
    let last = targets[targets.len() - 1];
    w.assert_new_events_belong_to(&mut checked, last, &scenario);

    assert!(
        w.decoder.timed_out().is_empty(),
        "{scenario}: {:?}",
        w.decoder.timed_out()
    );
    assert_eq!(presented_paths(&w.fixture).last(), Some(&w.paths[last]), "{scenario}");
    assert_eq!(w.fixture.catalog.current_index(), index_of(last), "{scenario}");
    let on_screen = w
        .fixture
        .presenter
        .current_image()
        .unwrap_or_else(|| panic!("{scenario}: nothing is shown"));
    assert!(
        same_platform_image(on_screen, w.decoder.preview_of(NAMES[last]).platform_image()),
        "{scenario}: the settled image is not the preview of the last navigation"
    );
}

fn run_all(targets: &[usize], all: &[Vec<Op>]) {
    assert!(!all.is_empty());
    for ops in all {
        run_order(targets, ops);
    }
}

// C#: StalePreviewOverwriteTests.Burst_TwoFiles_EveryThumbnailAndPreviewOrder_NeverShowsAnOlderImage (40 orders; a late thumbnail never replaces the new image)
#[test]
fn burst_two_files_every_thumbnail_and_preview_order_never_shows_an_older_image() {
    let all = orders(&[0, 1], true);

    assert_eq!(all.len(), 40, "guards the enumerator itself");
    run_all(&[0, 1], &all);
}

// C#: StalePreviewOverwriteTests.Burst_ThreeFilesWithThumbnails_SeededSampleOfOrders_NeverShowsAnOlderImage (12 orders of the large space, fixed seed 20261012)
#[test]
fn burst_three_files_with_thumbnails_seeded_sample_of_orders_never_shows_an_older_image() {
    let all = orders(&[0, 1, 2], true);

    assert!(
        all.len() > 1000,
        "the order space is large; it is sampled, not enumerated"
    );
    let sample = seeded_sample(all, 20_261_012, 12);
    assert_eq!(sample.len(), 12);
    run_all(&[0, 1, 2], &sample);
}

// C#: StalePreviewOverwriteTests.JoinedPreloadDecodes_FinishingInReverseOrder_NeverOverwriteTheNewImage
// Three preload decodes are in flight (no cancellation possible); the user visits each file (each joins its preload); the decodes finish 2, 1, 0.
#[test]
fn joined_preload_decodes_finishing_in_reverse_order_never_overwrite_the_new_image() {
    let mut w = world(false);
    let preloads: Vec<_> = w
        .paths
        .iter()
        .map(|path| w.fixture.service.get_preview_path(path))
        .collect();
    for name in NAMES {
        assert!(
            w.decoder.wait_started(name),
            "the preload decode of {name} never started"
        );
    }
    for file in 0..NAMES.len() {
        start(&mut w.fixture, index_of(file));
        pump_one(&mut w.fixture); // the stat: the viewer request joins the preload of its file
    }
    assert!(
        w.fixture.metrics.snapshot().inflight_joins >= 3,
        "each navigation joined the preload of its file"
    );

    for file in [2_usize, 1, 0] {
        w.decoder.open(NAMES[file]);
        assert!(w.decoder.wait_finished_if_started(NAMES[file]));
        w.fixture.pump(); // file 1 and 0 finish after file 2 was (or is being) presented
    }
    for preload in preloads {
        let result = preload
            .wait_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("a preload never completed"));
        assert!(result.is_ok(), "the preload failed: {:?}", result.err());
    }
    w.fixture.pump_until_idle();

    let mut checked = Checked::default();
    w.assert_new_events_belong_to(&mut checked, 2, "joined preloads in reverse");
    assert!(w.decoder.timed_out().is_empty());
    assert_eq!(presented_paths(&w.fixture), vec![w.paths[2].clone()]);
    assert_eq!(images_shown(&w.fixture), 1);
    assert_eq!(w.fixture.catalog.current_index(), 2);
    let on_screen = w.fixture.presenter.current_image().expect("an image is shown");
    assert!(same_platform_image(
        on_screen,
        w.decoder.preview_of(NAMES[2]).platform_image()
    ));
    for name in NAMES {
        assert_eq!(w.decoder.started_count(name), 1, "{name}: the joins share one decode");
    }
}

// C#: StalePreviewOverwriteTests.PostPresentStat_OfASupersededNavigation_FinishingLate_AppliesNothing
// File a is on screen and its refresh stat is held; the user moves to b (presented); THEN a's refresh stat answers (with a changed length, so applying it
// would update a's catalog entry and write a's "ready" status).
#[test]
fn post_present_stat_of_a_superseded_navigation_finishing_late_applies_nothing() {
    let decoder = StaleDecoder::new();
    let fs = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &NAMES[..2],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            ..FixtureOptions::default()
        },
    );
    f.settings.loading_mode = Some(LoadingMode::Preview);
    let (a, b) = (f.path(NAMES[0]), f.path(NAMES[1]));
    let real = real_stat(&a);
    let real_length = i64::try_from(real.length).unwrap_or(0);
    let release_second = Gate::new();
    {
        let release = Arc::clone(&release_second);
        fs.script(&a, move |call| {
            if call == 1 {
                return Ok(Some(real)); // the initial stat is the real one (the decode verifies the source against it)
            }
            assert!(release.wait(), "the refresh stat of a was never released");
            Ok(Some(FileStat {
                length: real.length + 1,
                last_write_utc: real.last_write_utc,
            }))
        });
    }
    decoder.open(NAMES[0]);
    decoder.open(NAMES[1]);

    start(&mut f, 0);
    f.pump_until(|_| fs.stat_count(&a) >= 2); // a IS on screen and its refresh stat entered the file system (held)
    assert_eq!(images_shown(&f), 1);
    assert_eq!(presented_paths(&f), vec![a.clone()]);
    // The stat worker is one FIFO thread (as in C#): b's stat queues behind a's held refresh stat, so b cannot finish before a's stat answers.
    start(&mut f, 1);
    let statuses_before = statuses(&f);
    assert_eq!(f.catalog.find(&a).and_then(|entry| entry.length), Some(real_length));

    release_second.open(); // a's refresh stat answers, AFTER the user moved on
    pump_one(&mut f); // exactly that (stale) completion
    f.pump_until_idle(); // then b is presented

    assert_eq!(
        statuses(&f)[..statuses_before.len()],
        statuses_before[..],
        "the late stat wrote no status"
    );
    assert!(
        statuses(&f)[statuses_before.len()..]
            .iter()
            .all(|status| !status.contains(NAMES[0])),
        "a status of a reached the sink after b was started: {:?}",
        statuses(&f)
    );
    assert_eq!(images_shown(&f), 2, "a's image, then b's image: nothing else");
    assert_eq!(presented_paths(&f), vec![a.clone(), b.clone()]);
    assert_eq!(
        f.catalog.find(&a).and_then(|entry| entry.length),
        Some(real_length),
        "a's catalog entry was not touched by the late stat"
    );
    assert_eq!(f.catalog.current_index(), 1);
    assert_eq!(f.catalog.current().map(|entry| entry.path()), Some(b.as_str()));
    assert_eq!(
        f.metrics.snapshot().presented_images,
        1,
        "only b finished its presentation"
    );
}

/// File a is on screen and its source-size (header) read is held; the user moves to b; a's header read returns late. `b_settled_first`: b is fully presented
/// before a's read returns (the C# order); otherwise b is still decoding when it returns, so the stale completion meets the live navigation of b.
fn header_read_returns_late(b_settled_first: bool) {
    let decoder = StaleDecoder::new();
    let scratch = pr_test_support::TempRoot::new("g20-stale-header");
    let service = build_service_full(
        &scratch.join("cache"),
        Arc::clone(&decoder) as _,
        ServiceOptions::default(),
        Arc::new(pr_core::diagnostics::metrics::ReviewMetrics::new()),
        None,
    );
    let fs = ScriptedFs::new();
    let hook_service = Arc::clone(&service);
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &NAMES[..2],
        FixtureOptions {
            prebuilt_service: Some(service),
            file_system: Some(Arc::clone(&fs) as _),
            // The dimensions the decode seeded are evicted right after the image is shown, so the status lookup has to read the header.
            on_presented_hook: Some(Box::new(move |_path: &str| hook_service.clear_cache())),
            ..FixtureOptions::default()
        },
    );
    f.settings.loading_mode = Some(LoadingMode::Preview);
    let (a, b) = (f.path(NAMES[0]), f.path(NAMES[1]));
    decoder.hold_info(NAMES[0]);
    decoder.open(NAMES[0]);
    if b_settled_first {
        decoder.open(NAMES[1]);
    }

    start(&mut f, 0);
    let held = Arc::clone(&decoder);
    f.pump_until(|_| held.info_started(NAMES[0])); // a IS on screen; its source size is still being read
    assert_eq!(images_shown(&f), 1);
    start(&mut f, 1);
    if b_settled_first {
        f.pump_until(|f| f.metrics.snapshot().presented_images >= 1); // b is fully presented
    } else {
        let started = Arc::clone(&decoder);
        f.pump_until(|_| started.has_started(NAMES[1])); // b is really inside its decode
    }
    let events_before = lock(&f.sink_log).call_count();
    let statuses_before = statuses(&f);
    let stats_before = fs.stat_count(&a);

    decoder.release_info(NAMES[0]); // a's header read returns LATE
    pump_one(&mut f); // exactly that completion
    assert_eq!(
        lock(&f.sink_log).call_count(),
        events_before,
        "nothing reached the sink"
    );
    decoder.open(NAMES[1]);
    f.pump_until_idle();

    assert_eq!(
        fs.stat_count(&a),
        stats_before,
        "no refresh stat for the superseded navigation"
    );
    assert_eq!(stats_before, 1, "only a's initial stat");
    assert!(
        statuses(&f)[statuses_before.len()..]
            .iter()
            .all(|status| !status.contains(NAMES[0])),
        "a status of a reached the sink after b was started: {:?}",
        statuses(&f)
    );
    assert_eq!(presented_paths(&f), vec![a, b.clone()]);
    assert_eq!(images_shown(&f), 2, "a's image, then b's image: nothing else");
    assert_eq!(f.catalog.current_index(), 1);
    assert_eq!(f.catalog.current().map(|entry| entry.path()), Some(b.as_str()));
    assert_eq!(f.metrics.snapshot().presented_images, 1);
    let on_screen = f.presenter.current_image().expect("an image is shown");
    assert!(same_platform_image(
        on_screen,
        decoder.preview_of(NAMES[1]).platform_image()
    ));
}

// C#: StalePreviewOverwriteTests.HeaderReadFinishingLate_OfASupersededNavigation_DoesNotStatNorTouchAnything
// b is presented first; THEN a's header read returns: no refresh stat for a, nothing reaches the sink.
#[test]
fn header_read_finishing_late_of_a_superseded_navigation_does_not_stat_nor_touch_anything() {
    header_read_returns_late(true);
}

// No C# counterpart (the C# test only returns the read after b settled): the same, but a's read returns while b is still decoding, i.e. the stale completion meets
// the live navigation of b (the Rust presenter keeps one navigation record, so this is where a missing token check would corrupt b).
#[test]
fn header_read_finishing_late_while_the_new_navigation_is_loading_does_not_touch_it() {
    header_read_returns_late(false);
}
