//! W8-08b: `MainViewModel` navigation, open folder, instance ownership, capture member, compare toggle and sibling folders
//! (`contracts/W8-08b.md`, C# `MainViewModel.cs` lines 327 to 564). Each test names the C# test it ports (`// C#: Class.Method`) or is
//! marked `// New test`. The window under test is the whole `MainViewModel` over real coordinators and a fake decoder
//! (`support/main_env.rs`); nothing here depends on an absolute local path, a sleep or a native resource.

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use pr_app::coordinators::sibling_navigator::SiblingNavError;
use pr_app::viewmodels::format::status::sibling_folder_boundary;
use pr_app::viewmodels::main::{MainProperty, OpenFolderError};
use pr_core::instance::ownership::FolderOpenDecision;
use pr_core::model::{LoadingMode, RawPairMode};
use pr_core::session::store::SessionState;
use pr_core::text::eq_ignore_case;
use pr_l10n::template::LocArg;
use support::main_env::{EnvOptions, MainEnv, RecordingOwnership, en_with};
use support::presenter_fakes::FormatFixture;

/// One step command of the view model, called through a plain function pointer.
type StepCommand = fn(&mut MainEnv);

// ---- helpers ----------------------------------------------------------------------------------------------------------------

/// Opens `1.png 2.png 3.png` at `2.png` (index 1) with the Explorer query held back: the opened file is presented and the Explorer
/// order stays pending (INV-9), so every command that moves away from it must wait.
fn open_with_pending_order(env: &mut MainEnv) -> (String, Vec<String>) {
    let (folder, files) = env.images("pending_order", &["1.png", "2.png", "3.png"]);
    env.explorer.set_order(files.clone());
    env.vm
        .open_folder(&folder, Some(&files[1]))
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("the opened file to be presented", &["Explorer"], |_| false);
    assert_eq!(env.vm.current_index(), 1, "the opened file is presented");
    assert_eq!(env.runner.count_of("Explorer"), 1, "the Explorer query is held back");
    (folder, files)
}

fn pair_settings(settings: &mut pr_core::settings::app_settings::AppSettings) {
    settings.raw_support_enabled = true;
    settings.raw_pair_mode = RawPairMode::PreferJpeg;
}

/// A folder `pairs` with `a.jpg + a.cr2` (and `b.jpg + b.cr2` when `two`), RAW support on, pair mode `PreferJpeg`.
fn pair_env(two: bool) -> (MainEnv, Vec<String>) {
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(pair_settings),
        ..EnvOptions::default()
    });
    let names: &[&str] = if two {
        &["a.jpg", "a.cr2", "b.jpg", "b.cr2"]
    } else {
        &["a.jpg", "a.cr2"]
    };
    let (folder, files) = env.images("pairs", names);
    env.open(&folder);
    (env, files)
}

// ---- the Explorer-order gate (INV-9, K-01) ----------------------------------------------------------------------------------

#[test]
fn skip_while_the_explorer_order_is_pending_waits_until_the_order_gate_is_released() {
    // C#: MainViewModelAdvancedTests.SkipAsync_WhileTheExplorerOrderIsPending_WaitsForIt
    let mut env = MainEnv::new();
    let (_, files) = open_with_pending_order(&mut env);

    let parked = env.vm.skip();

    assert!(parked.is_empty(), "a parked command returns an empty change set");
    assert_eq!(
        env.vm.current_index(),
        1,
        "nothing was skipped while the order is pending"
    );
    assert!(
        env.vm.session().is_some_and(|session| session.skipped.is_empty()),
        "session: {:?}",
        env.vm.session()
    );
    assert!(!env.vm.is_idle(), "the parked command is outstanding work");

    env.settle();

    assert_eq!(
        env.vm.current_index(),
        2,
        "the order gate was released and the skip ran"
    );
    let skipped = &env.vm.session().map(|s| s.skipped.clone()).unwrap_or_default();
    assert_eq!(skipped, &vec![files[1].clone()]);
}

#[test]
fn next_while_the_explorer_order_is_pending_waits_until_the_order_gate_is_released() {
    // New test (the gate of NextAsync)
    let mut env = MainEnv::new();
    open_with_pending_order(&mut env);

    assert!(env.vm.next().is_empty());
    assert_eq!(env.vm.current_index(), 1);
    env.settle();
    assert_eq!(env.vm.current_index(), 2);
}

#[test]
fn previous_while_the_explorer_order_is_pending_waits_until_the_order_gate_is_released() {
    // New test (the gate of PreviousAsync)
    let mut env = MainEnv::new();
    open_with_pending_order(&mut env);

    assert!(env.vm.previous().is_empty());
    assert_eq!(env.vm.current_index(), 1);
    env.settle();
    assert_eq!(env.vm.current_index(), 0);
}

#[test]
fn first_while_the_explorer_order_is_pending_waits_until_the_order_gate_is_released() {
    // New test (the gate of FirstAsync)
    let mut env = MainEnv::new();
    open_with_pending_order(&mut env);

    assert!(env.vm.first().is_empty());
    assert_eq!(env.vm.current_index(), 1);
    env.settle();
    assert_eq!(env.vm.current_index(), 0);
}

#[test]
fn last_while_the_explorer_order_is_pending_waits_until_the_order_gate_is_released() {
    // New test (the gate of LastAsync)
    let mut env = MainEnv::new();
    open_with_pending_order(&mut env);

    assert!(env.vm.last().is_empty());
    assert_eq!(env.vm.current_index(), 1);
    env.settle();
    assert_eq!(env.vm.current_index(), 2);
}

#[test]
fn toggle_capture_group_member_while_the_explorer_order_is_pending_waits_until_the_order_gate_is_released() {
    // New test (the gate of ToggleCaptureGroupMemberAsync)
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(pair_settings),
        ..EnvOptions::default()
    });
    let (folder, files) = env.images("pair_pending", &["a.jpg", "a.cr2", "b.jpg", "b.cr2"]);
    env.explorer.set_order(vec![files[0].clone(), files[2].clone()]);
    env.vm
        .open_folder(&folder, Some(&files[0]))
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("the opened file to be presented", &["Explorer"], |_| false);
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[0].as_str()));

    assert!(env.vm.toggle_capture_group_member().is_empty());
    assert_eq!(
        env.vm.presenter().current_presented_path(),
        Some(files[0].as_str()),
        "the member did not change while the order is pending"
    );
    env.settle();
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[1].as_str()));
}

#[test]
fn parked_commands_run_in_submission_order_and_a_later_command_does_not_overtake() {
    // New test (T3: FIFO of the parked commands)
    let mut env = MainEnv::new();
    open_with_pending_order(&mut env);

    // Submitted `last`, then `first`: the final state is the one `first` leaves.
    assert!(env.vm.last().is_empty());
    assert!(env.vm.first().is_empty());
    assert_eq!(env.vm.current_index(), 1);
    env.settle();
    assert_eq!(env.vm.current_index(), 0, "first ran after last");
}

#[test]
fn a_load_that_supersedes_the_one_holding_the_gate_releases_the_parked_commands() {
    // New test (the gate is released by `load`, not only by the Explorer event)
    let mut env = MainEnv::new();
    open_with_pending_order(&mut env);
    let (other, _) = env.images("other", &["x.png"]);
    assert!(env.vm.last().is_empty());

    env.vm
        .open_folder(&other, None)
        .unwrap_or_else(|error| panic!("open: {error}"));

    // The superseded load's gate was released inside `load`: the parked command ran against the catalog that was still loaded.
    assert_eq!(env.vm.current_index(), 2, "last ran on the old catalog");
    env.settle();
    assert_eq!(env.names(), vec!["x.png"]);
}

// ---- stepping ---------------------------------------------------------------------------------------------------------------

#[test]
fn next_and_previous_advance_and_bump_the_interaction_generation() {
    // C#: MainViewModelNavigationTests.NextAsync_And_PreviousAsync_AdvancesAndUpdatesInteraction
    let mut env = MainEnv::new();
    let (folder, _) = env.images("album_nav", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open(&folder);
    let initial = env.clock.current_interaction();

    env.vm.next();
    env.settle();
    assert_eq!(env.vm.current_index(), 1);
    assert!(env.clock.current_interaction() > initial);

    env.vm.next();
    env.settle();
    assert_eq!(env.vm.current_index(), 2);
    assert!(!env.vm.can_navigate_next());
    assert!(env.vm.can_navigate_previous());

    env.vm.previous();
    env.settle();
    assert_eq!(env.vm.current_index(), 1);
    assert!(env.vm.can_navigate_next());
    assert!(env.vm.can_navigate_previous());

    env.vm.first();
    env.settle();
    assert_eq!(env.vm.current_index(), 0);
    assert!(!env.vm.can_navigate_previous());
}

#[test]
fn last_goes_to_the_last_image_and_first_comes_back() {
    // C#: MainViewModelNavigationTests.LastAsync_GoesToLastImage_AndBackWithFirst
    let mut env = MainEnv::new();
    let (folder, _) = env.images("album_last", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open(&folder);
    let interaction = env.clock.current_interaction();

    env.vm.last();
    env.settle();

    assert_eq!(env.vm.current_index(), 2);
    assert!(!env.vm.can_navigate_next());
    assert!(env.clock.current_interaction() > interaction);
    env.vm.first();
    env.settle();
    assert_eq!(env.vm.current_index(), 0);
}

#[test]
fn last_without_images_is_a_no_op() {
    // C#: MainViewModelNavigationTests.LastAsync_WithoutImages_IsNoOp
    let mut env = MainEnv::new();
    let interaction = env.clock.current_interaction();

    let changes = env.vm.last();

    assert!(changes.is_empty());
    assert_eq!(env.vm.current_index(), -1);
    assert_eq!(env.clock.current_interaction(), interaction);
}

#[test]
fn next_makes_no_explicit_navigation_state_notification() {
    // C#: MainViewModelNavigationTests.NextAsync_RaisesNavigationStateChanged_ExactlyThreeTimes (a `Different` row, W8-08a: the C#
    // pin counts three raises of CurrentIndex; the change set only holds WHICH properties changed, so the test asserts the set after
    // pumping and that the catalog-changed event is not raised by a plain navigation (that one is raised by the explicit sink path).
    let mut env = MainEnv::new();
    let (folder, _) = env.images("album_notify", &["a.jpg", "b.jpg"]);
    env.update_settings(|settings| settings.loading_mode = Some(LoadingMode::Original));
    env.open(&folder);
    env.changes = Default::default();

    let sync_part = env.vm.next();
    env.settle();

    // The synchronous part of `next` and the pumped part together (a cached image is assigned inside the call itself).
    let mut all = sync_part.clone();
    all.merge(env.changes.clone());
    for property in [
        MainProperty::CanNavigateNext,
        MainProperty::CanNavigatePrevious,
        MainProperty::HasImages,
        MainProperty::CurrentIndex,
        MainProperty::TotalFiles,
        MainProperty::CurrentImage,
        MainProperty::StatusText,
    ] {
        assert!(all.main.contains(property), "{property:?} was not reported");
    }
    assert!(!all.catalog_changed, "a navigation does not touch the catalog");
    assert!(
        all.image_changing.is_some_and(|args| args.is_file_change),
        "navigating to another file raises ImageChanging before CurrentImage"
    );
    // The synchronous part (before any worker result) already refreshed the state: the presenter drives the notification (R05).
    assert!(sync_part.main.contains(MainProperty::CurrentIndex));
    assert_eq!(env.vm.current_index(), 1);
}

#[test]
fn empty_folder_every_command_is_a_no_op() {
    // C#: MainViewModelNavigationTests.Model.EmptyFolder_EveryCommandIsANoOp (the recycle / action / undo / zoom commands are added
    // by W8-08c once they exist: they are listed in `empty_folder_file_and_zoom_commands_are_no_ops` of viewmodels_main_commands.rs)
    let mut env = MainEnv::new();
    let folder = env.folder("empty_model");
    std::fs::write(Path::new(&folder).join("notes.txt"), "not an image")
        .unwrap_or_else(|error| panic!("write: {error}"));

    env.open(&folder);
    env.vm.next();
    env.vm.previous();
    env.vm.first();
    env.vm.last();
    env.vm.skip();
    env.vm.toggle_compare();
    env.settle();

    assert!(!env.vm.has_images());
    assert_eq!(env.vm.total_files(), 0);
    assert!(!env.vm.can_navigate_next());
    assert!(!env.vm.can_navigate_previous());
    assert!(env.vm.current_image().is_none());
    assert!(!env.vm.current_has_compare_pair());
    assert!(env.vm.session().is_some_and(|session| session.skipped.is_empty()));
}

#[test]
fn next_at_the_last_image_and_previous_at_the_first_present_the_same_image_again() {
    // New test (the clamps `min(current + 1, count - 1)` and `max(current - 1, 0)`: the image stays but is presented again)
    let mut env = MainEnv::new();
    let (folder, _) = env.images("album_clamp", &["a.png", "b.png"]);
    env.open(&folder);

    let before = env.clock.current_navigation();
    env.vm.previous();
    env.settle();
    assert_eq!(env.vm.current_index(), 0);
    assert!(
        env.clock.current_navigation() > before,
        "previous at the first image presents it again"
    );

    env.vm.last();
    env.settle();
    let before = env.clock.current_navigation();
    env.vm.next();
    env.settle();
    assert_eq!(env.vm.current_index(), 1);
    assert!(
        env.clock.current_navigation() > before,
        "next at the last image presents it again"
    );

    let before = env.clock.current_navigation();
    env.vm.skip();
    env.settle();
    assert_eq!(env.vm.current_index(), 1, "skip at the last image stays on it");
    assert!(env.clock.current_navigation() > before);
}

#[test]
fn every_step_command_clears_the_event_status() {
    // New test (C# `_statusText = string.Empty` before the present of Next / Previous / First / Last / Skip)
    let mut env = MainEnv::new();
    let (folder, _) = env.images("album_status", &["a.png", "b.png", "c.png"]);
    env.open(&folder);
    env.vm.next();
    env.settle();
    let commands: [(&str, StepCommand); 5] = [
        ("next", |env| {
            env.vm.next();
        }),
        ("previous", |env| {
            env.vm.previous();
        }),
        ("first", |env| {
            env.vm.first();
        }),
        ("last", |env| {
            env.vm.last();
        }),
        ("skip", |env| {
            env.vm.skip();
        }),
    ];
    for (name, run) in commands {
        env.vm.set_status_text("action result");
        assert_eq!(env.vm.status_text(), "action result");
        run(&mut env);
        assert_ne!(
            env.vm.status_text(),
            "action result",
            "{name} left the event status in place"
        );
        env.settle();
    }
}

#[test]
fn single_image_navigation_stays_put() {
    // C#: MainViewModelNavigationTests.Model.SingleImage_NavigationStaysPut
    let mut env = MainEnv::new();
    let (folder, _) = env.images("single_model", &["only.png"]);
    env.open(&folder);

    for _ in 0..5 {
        env.vm.next();
        env.settle();
        env.vm.previous();
        env.settle();
        env.vm.first();
        env.settle();
        env.vm.last();
        env.settle();
    }

    assert_eq!(env.vm.current_index(), 0);
    assert_eq!(env.vm.total_files(), 1);
    assert!(!env.vm.can_navigate_next());
    assert!(!env.vm.can_navigate_previous());
    assert!(env.vm.current_image().is_some());
}

// ---- the reference model ----------------------------------------------------------------------------------------------------

/// The .NET `System.Random(seed)` generator (the compatibility algorithm that a seeded `Random` keeps), so a seed replays the very
/// sequence the C# theory row does.
struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    const MBIG: i32 = i32::MAX;
    const MSEED: i32 = 161_803_398;

    fn new(seed: i32) -> Self {
        let mut seed_array = [0_i32; 56];
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = Self::MSEED - subtraction;
        seed_array[55] = mj;
        let mut mk = 1_i32;
        let mut ii = 0_usize;
        for _ in 1..55 {
            ii += 21;
            if ii >= 55 {
                ii -= 55;
            }
            seed_array[ii] = mk;
            mk = mj - mk;
            if mk < 0 {
                mk += Self::MBIG;
            }
            mj = seed_array[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let other = seed_array[1 + (i + 30) % 55];
                seed_array[i] = seed_array[i].wrapping_sub(other);
                if seed_array[i] < 0 {
                    seed_array[i] += Self::MBIG;
                }
            }
        }
        Self {
            seed_array,
            inext: 0,
            inextp: 21,
        }
    }

    fn internal_sample(&mut self) -> i32 {
        self.inext += 1;
        if self.inext >= 56 {
            self.inext = 1;
        }
        self.inextp += 1;
        if self.inextp >= 56 {
            self.inextp = 1;
        }
        let mut value = self.seed_array[self.inext] - self.seed_array[self.inextp];
        if value == Self::MBIG {
            value -= 1;
        }
        if value < 0 {
            value += Self::MBIG;
        }
        self.seed_array[self.inext] = value;
        value
    }

    /// `Random.Next(maxValue)`.
    fn next(&mut self, max: i32) -> i32 {
        // Truncation is the .NET `(int)(Sample() * maxValue)`.
        (f64::from(self.internal_sample()) * (1.0 / f64::from(Self::MBIG)) * f64::from(max)) as i32
    }
}

/// The obviously-correct model of what the catalog, the current index and the skipped list must be (C# `NavigationModel`).
struct NavigationModel {
    list: Vec<String>,
    current: i32,
    deleted: HashSet<String>,
    skipped: HashSet<String>,
    last_presented: Option<String>,
}

impl NavigationModel {
    fn new(names: &[String]) -> Self {
        Self {
            list: names.to_vec(),
            current: 0,
            deleted: HashSet::new(),
            skipped: HashSet::new(),
            last_presented: names.first().cloned(),
        }
    }

    fn count(&self) -> i32 {
        i32::try_from(self.list.len()).unwrap_or(i32::MAX)
    }

    /// Presents `index`; a file that no longer exists is dropped and the neighbour that takes its place is tried.
    fn present(&mut self, index: i32) {
        if index < 0 || index >= self.count() {
            return;
        }
        self.current = index;
        while self.deleted.contains(&self.list[self.current as usize]) {
            let removed_at = self.current;
            self.list.remove(removed_at as usize);
            if self.list.is_empty() {
                self.current = -1;
                return;
            }
            self.current = removed_at.min(self.count() - 1);
        }
        self.last_presented = Some(self.list[self.current as usize].clone());
    }

    fn next(&mut self) {
        if !self.list.is_empty() {
            self.present((self.current + 1).min(self.count() - 1));
        }
    }

    fn previous(&mut self) {
        if !self.list.is_empty() {
            self.present((self.current - 1).max(0));
        }
    }

    fn first(&mut self) {
        if !self.list.is_empty() {
            self.present(0);
        }
    }

    fn last(&mut self) {
        if !self.list.is_empty() {
            self.present(self.count() - 1);
        }
    }

    fn skip(&mut self) {
        if self.current < 0 || self.current >= self.count() {
            return;
        }
        self.skipped.insert(self.list[self.current as usize].clone());
        self.present((self.current + 1).min(self.count() - 1));
    }

    fn reopen(&mut self) {
        let alive: Vec<String> = self
            .list
            .iter()
            .filter(|name| !self.deleted.contains(*name))
            .cloned()
            .collect();
        // Files deleted while not yet reached simply are not scanned again.
        self.list = alive;
        if self.list.is_empty() {
            self.current = -1;
            return;
        }
        let resume = self.last_presented.as_ref().and_then(|last| {
            self.list
                .iter()
                .position(|name| eq_ignore_case(name, last))
                .and_then(|position| i32::try_from(position).ok())
        });
        self.present(resume.unwrap_or(0));
    }
}

fn random_navigation_sequence(seed: i32) {
    let mut rng = DotNetRandom::new(seed);
    let mut env = MainEnv::new();
    let names: Vec<String> = (0..7).map(|i| format!("img{i}.png")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (folder, _) = env.images(&format!("model{seed}"), &name_refs);
    env.open(&folder);
    // AR16: the readability probe ran in `settle`; files may be deleted behind the app's back from here on.
    let mut model = NavigationModel::new(&names);
    let mut log: Vec<String> = Vec::new();

    for step in 0..80 {
        match rng.next(9) {
            0 | 1 => {
                log.push("Next".into());
                model.next();
                env.vm.next();
                env.settle();
            }
            2 => {
                log.push("Previous".into());
                model.previous();
                env.vm.previous();
                env.settle();
            }
            3 => {
                log.push("First".into());
                model.first();
                env.vm.first();
                env.settle();
            }
            4 => {
                log.push("Last".into());
                model.last();
                env.vm.last();
                env.settle();
            }
            5 => {
                log.push("Skip".into());
                model.skip();
                env.vm.skip();
                env.settle();
            }
            6 | 7 => {
                if model.list.is_empty() {
                    continue;
                }
                let at = usize::try_from(rng.next(model.count())).unwrap_or(0);
                let victim = model.list[at].clone();
                log.push(format!("Delete {victim}"));
                // C# `File.Delete` ignores a file that is already gone (a victim picked twice).
                match std::fs::remove_file(Path::new(&folder).join(&victim)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => panic!("delete: {error}"),
                }
                model.deleted.insert(victim);
                continue; // nothing changes until the app reaches the file
            }
            _ => {
                log.push("Reopen".into());
                // Not only `flush`: a debounced write the writer's timer thread already drained is in flight and invisible to it.
                env.session_writer.flush();
                env.session_writer.when_idle();
                model.reopen();
                env.open(&folder);
            }
        }

        let tail: Vec<&String> = log.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
        let context = format!("seed {seed}, step {step}: {tail:?}");
        assert_eq!(env.names(), model.list, "{context}: catalog");
        assert_eq!(env.vm.current_index(), model.current, "{context}: index");
        assert_eq!(env.vm.has_images(), !model.list.is_empty(), "{context}");
        assert_eq!(
            env.vm.can_navigate_next(),
            !model.list.is_empty() && model.current < model.count() - 1,
            "{context}"
        );
        assert_eq!(
            env.vm.can_navigate_previous(),
            !model.list.is_empty() && model.current > 0,
            "{context}"
        );
        let skipped: Vec<String> = env
            .vm
            .session()
            .map(|session| session.skipped.iter().map(|path| MainEnv::leaf(path)).collect())
            .unwrap_or_default();
        assert_eq!(
            skipped.len(),
            model.skipped.len(),
            "{context}: no duplicates in the persisted list"
        );
        assert!(
            skipped.iter().all(|name| model.skipped.contains(name)),
            "{context}: skipped {skipped:?}, expected {:?}",
            model.skipped
        );
    }
}

macro_rules! navigation_seed_tests {
    ($($name:ident => $seed:expr),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                // C#: MainViewModelNavigationTests.Model.RandomNavigationSequence_MatchesTheReferenceModel (theory, seeds 1 to 10)
                random_navigation_sequence($seed);
            }
        )*
    };
}

navigation_seed_tests! {
    random_navigation_sequence_matches_the_reference_model_seed_1 => 1,
    random_navigation_sequence_matches_the_reference_model_seed_2 => 2,
    random_navigation_sequence_matches_the_reference_model_seed_3 => 3,
    random_navigation_sequence_matches_the_reference_model_seed_4 => 4,
    random_navigation_sequence_matches_the_reference_model_seed_5 => 5,
    random_navigation_sequence_matches_the_reference_model_seed_6 => 6,
    random_navigation_sequence_matches_the_reference_model_seed_7 => 7,
    random_navigation_sequence_matches_the_reference_model_seed_8 => 8,
    random_navigation_sequence_matches_the_reference_model_seed_9 => 9,
    random_navigation_sequence_matches_the_reference_model_seed_10 => 10,
}

// ---- skip -------------------------------------------------------------------------------------------------------------------

#[test]
fn skip_adds_to_the_session_skipped_list_and_advances() {
    // C#: MainViewModelNavigationTests.SkipAsync_AddsToSessionSkipped_AndAdvances
    let mut env = MainEnv::new();
    let (folder, files) = env.images("album_skip", &["skip1.jpg", "skip2.jpg"]);
    env.open(&folder);

    env.vm.skip();
    env.settle();

    let session = env.vm.session().cloned().expect("a session");
    assert!(session.skipped.contains(&files[0]));
    assert_eq!(env.vm.current_index(), 1);
    env.session_writer.flush();
    env.session_writer.when_idle();
    let saved = env
        .sessions
        .load(&folder)
        .unwrap_or_else(|error| panic!("load session: {error}"));
    assert!(saved.skipped.contains(&files[0]), "the skipped image was persisted");
}

#[test]
fn skip_the_same_image_twice_is_recorded_once() {
    // C#: MainViewModelNavigationTests.SkipAsync_SameImageTwice_IsRecordedOnce (R2-F-10)
    let mut env = MainEnv::new();
    let (folder, files) = env.images("album_skip_twice", &["skip1.jpg", "skip2.jpg"]);
    env.open(&folder);

    env.vm.skip();
    env.settle();
    env.vm.first();
    env.settle();
    env.vm.skip();
    env.settle();

    let session = env.vm.session().cloned().expect("a session");
    assert_eq!(session.skipped, vec![files[0].clone()]);
    env.session_writer.flush();
    env.session_writer.when_idle();
    let saved = env
        .sessions
        .load(&folder)
        .unwrap_or_else(|error| panic!("load session: {error}"));
    assert_eq!(saved.skipped.len(), 1);
}

#[test]
fn skip_with_different_case_is_recorded_once() {
    // New test (T8: the skipped list compares paths ignoring case)
    let mut env = MainEnv::new();
    let (folder, files) = env.images("album_skip_case", &["skip1.jpg", "skip2.jpg"]);
    let mut previous = SessionState::new(folder.clone());
    previous.skipped.push(files[0].to_uppercase());
    env.sessions
        .save(&previous)
        .unwrap_or_else(|error| panic!("save session: {error}"));
    env.open(&folder);

    env.vm.skip();
    env.settle();

    let session = env.vm.session().cloned().expect("a session");
    assert_eq!(
        session.skipped,
        vec![files[0].to_uppercase()],
        "the path already listed in another case is not added again"
    );
}

// ---- open folder / path -----------------------------------------------------------------------------------------------------

#[test]
fn open_folder_loads_the_folder_and_updates_catalog_and_title() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_LoadsFolderAndUpdatesCatalogAndTitle
    let mut env = MainEnv::new();
    let (folder, _) = env.images("album1", &["img1.jpg", "img2.jpg"]);

    env.open(&folder);

    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.current_index(), 0);
    assert!(env.vm.has_images());
    assert!(env.vm.can_navigate_next());
    assert!(!env.vm.can_navigate_previous());
    // Default TitleBarFields = FolderName only (not the full path).
    assert!(env.vm.folder_title().contains("album1"), "{}", env.vm.folder_title());
}

#[test]
fn open_folder_with_a_blank_folder_is_an_error_and_changes_nothing() {
    // New test (OpenFolderCoreAsync: ArgumentException.ThrowIfNullOrWhiteSpace)
    let mut env = MainEnv::new();
    let epoch = env.vm.folder_load_epoch();

    assert_eq!(env.vm.open_folder("", None).unwrap_err(), OpenFolderError::BlankFolder);
    assert_eq!(
        env.vm.open_folder("   ", None).unwrap_err(),
        OpenFolderError::BlankFolder
    );

    assert_eq!(env.vm.folder_load_epoch(), epoch);
    assert!(env.vm.is_idle());
    assert!(!OpenFolderError::BlankFolder.to_string().is_empty());
}

#[test]
fn open_folder_shows_the_sibling_image_folders_that_page_up_and_page_down_open() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_ShowsSiblingImageFolders_ThatPageUpPageDownOpen
    let mut env = MainEnv::new();
    let parent = env.folder("sibling_info");
    for name in ["2024-05-01", "2024-05-02", "2024-05-02b-empty", "2024-05-03"] {
        std::fs::create_dir_all(Path::new(&parent).join(name)).unwrap_or_else(|error| panic!("create: {error}"));
    }
    env.add_image(&Path::new(&parent).join("2024-05-01").to_string_lossy(), "a.jpg");
    env.add_image(&Path::new(&parent).join("2024-05-02").to_string_lossy(), "b.jpg");
    env.add_image(&Path::new(&parent).join("2024-05-03").to_string_lossy(), "c.jpg");
    env.update_settings(|settings| settings.show_folder_info = true); // off by default (Q-R20): opt in for this test

    env.open(&Path::new(&parent).join("2024-05-02").to_string_lossy());

    assert!(env.vm.info_overlay().is_folder_info_visible(&env.vm.settings()));
    let part = |key: &str, args: &[LocArg]| en_with(key, args);
    let line = |previous: Option<&str>, current: &str, next: Option<&str>| {
        let mut parts = Vec::new();
        if let Some(previous) = previous {
            parts.push(part(
                "main.folderInfo.previous",
                &[LocArg::new("key", "PageUp"), LocArg::new("folderName", previous)],
            ));
        }
        parts.push(part("main.folderInfo.current", &[LocArg::new("folderName", current)]));
        if let Some(next) = next {
            parts.push(part(
                "main.folderInfo.next",
                &[LocArg::new("key", "PageDown"), LocArg::new("folderName", next)],
            ));
        }
        parts.join("     ")
    };
    assert_eq!(
        env.vm.info_overlay().folder_info_text(),
        line(Some("2024-05-01"), "2024-05-02", Some("2024-05-03"))
    );

    // The display agrees with the key: PageDown opens the folder shown on the right (the empty one is skipped).
    env.vm
        .navigate_sibling_folder(1)
        .unwrap_or_else(|error| panic!("navigate: {error}"));
    env.settle();
    assert!(
        env.vm.folder_title().contains("2024-05-03"),
        "{}",
        env.vm.folder_title()
    );
    assert_eq!(
        env.vm.info_overlay().folder_info_text(),
        line(Some("2024-05-02"), "2024-05-03", None)
    );
}

#[test]
fn open_path_with_invalid_input_sets_the_warning_status() {
    // C#: MainViewModelNavigationTests.OpenPathAsync_InvalidInput_SetsWarningStatus (the C# test pins the Vietnamese text; this one
    // compares with the catalog text of the warning's key, English)
    let mut env = MainEnv::new();
    let missing = env.photos.join("non_existent_file.xyz").to_string_lossy().into_owned();

    let changes = env.vm.open_path(&missing);

    assert_eq!(env.vm.status_text(), env.text("core.dragDrop.noSupportedImages"));
    assert!(changes.main.contains(MainProperty::StatusText));
    assert!(env.vm.is_idle(), "no open was started");
}

#[test]
fn open_path_with_a_blank_path_does_nothing() {
    // New test (OpenPathAsync: IsNullOrWhiteSpace)
    let mut env = MainEnv::new();
    assert!(env.vm.open_path("  ").is_empty());
    assert_eq!(env.vm.folder_load_epoch(), 0);
}

fn open_path_webp(enabled: bool) {
    let mut env = MainEnv::new();
    let folder = env.folder(&format!("webp_open_{enabled}"));
    let webp = Path::new(&folder).join("photo.webp");
    std::fs::write(&webp, [0_u8; 64]).unwrap_or_else(|error| panic!("write: {error}"));
    env.update_settings(|settings| settings.webp_heic_support_enabled = enabled);

    env.vm.open_path(&webp.to_string_lossy());
    env.settle();

    assert_eq!(env.vm.total_files(), usize::from(enabled));
}

#[test]
fn open_path_webp_is_opened_when_the_webp_heic_switch_is_on() {
    // C#: MainViewModelAdvancedTests.WebpHeic.OpenPathAsync_WebpFile_IsOpenedOnlyWhileTheWebpHeicSwitchIsOn (case true)
    open_path_webp(true);
}

#[test]
fn open_path_webp_is_not_opened_when_the_webp_heic_switch_is_off() {
    // C#: MainViewModelAdvancedTests.WebpHeic.OpenPathAsync_WebpFile_IsOpenedOnlyWhileTheWebpHeicSwitchIsOn (case false)
    open_path_webp(false);
}

#[test]
fn open_path_with_a_file_opens_its_folder_at_that_file() {
    // New test (OpenPathAsync: a dropped image opens its folder at the image)
    let mut env = MainEnv::new();
    let (_, files) = env.images("drop_file", &["a.png", "b.png", "c.png"]);

    env.vm.open_path(&files[2]);
    env.settle();

    assert_eq!(env.vm.total_files(), 3);
    assert_eq!(env.vm.current_index(), 2);
}

#[test]
fn starting_a_load_clears_the_event_status_and_the_first_image_does_not_fade_from_the_previous_folder() {
    // New test (C# lines 427 to 431: `_statusText = string.Empty` and `_presenter.ResetFileIdentity()` before the load)
    let mut env = MainEnv::new();
    let (a, _) = env.images("fade_a", &["a1.png", "a2.png"]);
    let (b, _) = env.images("fade_b", &["b1.png", "b2.png"]);
    env.open(&a);
    env.vm.set_status_text("action result");

    env.vm
        .open_folder(&b, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert_ne!(
        env.vm.status_text(),
        "action result",
        "the load start cleared the event status"
    );
    env.changes = Default::default();
    env.settle();

    assert_eq!(env.names(), vec!["b1.png", "b2.png"]);
    assert!(
        env.changes.image_changing.is_none(),
        "the first image of a new folder is not a navigation to another file (no crossfade from the previous folder)"
    );
}

// ---- instance ownership (Q-R18) ---------------------------------------------------------------------------------------------

#[test]
fn ownership_forwards_keeps_the_current_folder_and_shows_status() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_OwnershipForwards_KeepsCurrentFolderAndShowsStatus
    let mut env = MainEnv::new();
    let (a, _) = env.images("a", &["a1.jpg"]);
    let (b, _) = env.images("b", &["b1.jpg", "b2.jpg"]);
    env.open(&a);
    let ownership = RecordingOwnership::new();
    ownership.decide(&b, FolderOpenDecision::ForwardedToOtherInstance);
    env.vm.set_folder_ownership(Some(ownership.clone()));

    env.open(&b);

    assert_eq!(env.vm.total_files(), 1);
    assert!(env.vm.folder_title().contains("a"), "{}", env.vm.folder_title());
    assert_eq!(
        env.vm.status_text(),
        env.text_with("status.folderOpenedInOtherWindow", &[LocArg::new("folder", "b")])
    );
    assert!(ownership.after_opens().is_empty(), "no AfterOpen for a forwarded open");
}

#[test]
fn ownership_refuses_shows_the_no_response_status() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_OwnershipRefuses_ShowsNoResponseStatus
    let mut env = MainEnv::new();
    let (b, _) = env.images("b", &["b1.jpg"]);
    let ownership = RecordingOwnership::new();
    ownership.decide(&b, FolderOpenDecision::OwnedByOtherInstance);
    env.vm.set_folder_ownership(Some(ownership.clone()));

    env.open(&b);

    assert_eq!(env.vm.total_files(), 0);
    assert_eq!(
        env.vm.status_text(),
        env.text_with(
            "status.folderOpenInOtherWindowNoResponse",
            &[LocArg::new("folder", "b")]
        )
    );
    assert!(
        ownership.after_opens().is_empty(),
        "AfterOpen is never called for a refusal"
    );
}

#[test]
fn ownership_proceeds_reports_the_shown_folder_and_the_finished_open() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_OwnershipProceeds_ReportsShownFolderAndFinishedOpen
    let mut env = MainEnv::new();
    let (a, _) = env.images("a", &["a1.jpg"]);
    let missing = env.photos.join("missing").to_string_lossy().into_owned();
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));

    env.open(&a);
    env.open(&missing); // fails: the window keeps showing A

    assert_eq!(ownership.before_opens(), vec![a.clone(), missing.clone()]);
    assert_eq!(ownership.shown(), vec![a.clone()]);
    assert_eq!(
        ownership.after_opens(),
        vec![(a.clone(), Some(a.clone())), (missing, Some(a))]
    );
    assert_eq!(env.vm.total_files(), 1);
}

#[test]
fn window_closed_while_ownership_pending_drops_the_open_and_reports_after_open() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_WindowClosedWhileOwnershipPending_DropsTheOpenAndReportsAfterOpen
    let mut env = MainEnv::new();
    let (a, _) = env.images("a", &["a1.jpg"]);
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));

    env.vm
        .open_folder(&a, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.vm.close_session(); // disposes the load coordinator while before_open is still pending
    env.settle(); // before_open answers Proceed after the window closed: must not load

    assert_eq!(env.vm.total_files(), 0);
    assert_eq!(ownership.after_opens(), vec![(a, None)]);
}

#[test]
fn older_open_resuming_after_a_newer_one_loaded_does_not_replace_it() {
    // C#: MainViewModelNavigationTests.OpenFolderAsync_OlderOpenResumesAfterNewerOneLoaded_DoesNotReplaceIt (R14)
    let mut env = MainEnv::new();
    let (b, _) = env.images("b", &["b1.jpg"]);
    let (c, _) = env.images("c", &["c1.jpg", "c2.jpg"]);
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));

    env.vm
        .open_folder(&b, None)
        .unwrap_or_else(|error| panic!("open: {error}")); // stays pending in before_open
    env.vm
        .open_folder(&c, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert!(env.runner.run_at(1), "the negotiation of C answers first");
    env.settle_holding("C to load", &["folder-ownership"], |_| false);
    assert_eq!(env.vm.total_files(), 2);
    assert!(env.runner.run_tagged("folder-ownership"), "now B answers");
    env.settle();

    assert_eq!(env.vm.total_files(), 2);
    assert!(env.vm.folder_title().contains('c'), "{}", env.vm.folder_title());
    assert_eq!(
        ownership.after_opens(),
        vec![(c.clone(), Some(c.clone())), (b, Some(c))],
        "the granted but superseded open still reports AfterOpen once"
    );
}

#[test]
fn refusal_superseded_by_a_newer_open_writes_no_status() {
    // New test (the newer open owns the status line)
    let mut env = MainEnv::new();
    let (b, _) = env.images("b", &["b1.jpg"]);
    let (c, _) = env.images("c", &["c1.jpg", "c2.jpg"]);
    let ownership = RecordingOwnership::new();
    ownership.decide(&b, FolderOpenDecision::ForwardedToOtherInstance);
    env.vm.set_folder_ownership(Some(ownership.clone()));
    env.vm
        .open_folder(&b, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.vm
        .open_folder(&c, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert!(env.runner.run_at(1));
    env.settle_holding("C to load", &["folder-ownership"], |_| false);

    assert!(env.runner.run_tagged("folder-ownership"));
    env.settle();

    let forwarded = env.text_with("status.folderOpenedInOtherWindow", &[LocArg::new("folder", "b")]);
    assert_ne!(env.vm.status_text(), forwarded);
    assert_eq!(env.vm.total_files(), 2);
    assert!(
        ownership.after_opens().iter().all(|(folder, _)| *folder != b),
        "a refusal never reports AfterOpen"
    );
}

#[test]
fn superseded_pending_open_is_finished_once_with_one_after_open() {
    // New test (note 5: exactly one AfterOpen per started open, also for the open a newer load supersedes)
    let mut env = MainEnv::new();
    let (a, _) = env.images("a", &["a1.jpg"]);
    let (b, _) = env.images("b", &["b1.jpg", "b2.jpg"]);
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    env.vm
        .open_folder(&a, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert!(env.runner.run_tagged("folder-ownership"));
    env.pump(); // A is granted and its load starts (the scan job is queued, not run)
    assert_eq!(env.vm.folder_load_epoch(), 1);
    assert!(ownership.after_opens().is_empty(), "A is still loading");

    env.vm
        .open_folder(&b, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    assert!(env.runner.run_tagged("folder-ownership"));
    env.pump(); // B is granted: its load supersedes A's
    assert_eq!(env.vm.folder_load_epoch(), 2);
    assert_eq!(
        ownership.after_opens(),
        vec![(a.clone(), None)],
        "A's open ended when B's load started"
    );

    env.settle();

    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(ownership.after_opens(), vec![(a, None), (b.clone(), Some(b))]);
}

#[test]
fn ownership_before_open_is_submitted_to_the_worker_not_run_on_the_ui_call() {
    // New test (T5: before_open can block on a named pipe, it must never run on the UI thread)
    let mut env = MainEnv::new();
    let (a, _) = env.images("a", &["a1.jpg"]);
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));

    let changes = env
        .vm
        .open_folder(&a, None)
        .unwrap_or_else(|error| panic!("open: {error}"));

    assert!(changes.is_empty());
    assert!(ownership.before_opens().is_empty(), "nothing ran inside the call");
    assert_eq!(
        env.runner.count_of("folder-ownership"),
        1,
        "the negotiation is a worker job"
    );
    assert!(!env.vm.is_idle());
    env.settle();
    assert_eq!(ownership.before_opens(), vec![a]);
    assert_eq!(env.vm.total_files(), 1);
}

#[test]
fn an_open_on_a_closed_window_is_ignored() {
    // C#: MainViewModelNavigationTests.Model.OpenFolderAfterCloseSession_IsIgnored
    let mut env = MainEnv::new();
    let (folder, _) = env.images("closed_model", &["a.png"]);
    let (other, _) = env.images("closed_other", &["b.png", "c.png"]);
    env.open(&folder);
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    env.vm.close_session();
    let epoch = env.vm.folder_load_epoch();

    env.open(&other);
    env.vm.open_path(&other);
    env.settle();

    assert_eq!(env.vm.folder_load_epoch(), epoch, "no load started");
    assert!(
        ownership.before_opens().is_empty(),
        "the ownership is not asked for a closed window"
    );
    assert!(
        env.log
            .entries()
            .iter()
            .all(|(level, _)| *level != pr_test_support::LogLevel::Error),
        "the dropped open is not an error: {:?}",
        env.log.entries()
    );

    assert_eq!(env.vm.total_files(), 1);
    assert_eq!(
        env.vm.catalog().current().map(|entry| MainEnv::leaf(entry.path())),
        Some("a.png".to_owned())
    );
}

// ---- capture group member and compare ---------------------------------------------------------------------------------------

#[test]
fn toggle_capture_group_member_switches_the_displayed_path_without_splitting_the_catalog_entry() {
    // C#: MainViewModelAdvancedTests.RawPair.ToggleCaptureGroupMemberAsync_SwitchesDisplayedPathWithoutSplittingCatalogEntry
    let (mut env, files) = pair_env(false);
    assert!(env.vm.current_has_capture_pair());
    assert_eq!(env.vm.capture_pair_badge(), env.text("main.capturePair.badge.jpeg"));

    let changes = env.vm.toggle_capture_group_member();
    env.settle();

    assert!(changes.main.contains(MainProperty::CapturePairBadge));
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[1].as_str()));
    assert_eq!(env.vm.capture_pair_badge(), env.text("main.capturePair.badge.raw"));
    assert_eq!(env.vm.current_index(), 0);
    assert_eq!(env.vm.total_files(), 1);

    env.vm.toggle_capture_group_member();
    env.settle();

    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[0].as_str()));
    assert_eq!(env.vm.capture_pair_badge(), env.text("main.capturePair.badge.jpeg"));
    assert_eq!(env.vm.current_index(), 0);
    assert_eq!(env.vm.total_files(), 1);
}

#[test]
fn toggle_capture_group_member_when_the_other_member_fails_to_decode_keeps_the_presented_path_on_the_shown_member() {
    // C#: MainViewModelAdvancedTests.RawSettings.ToggleCaptureGroupMemberAsync_WhenTheOtherMemberFailsToDecode_KeepsThePresentedPathOnTheShownMember
    let decoder = support::preview_fakes::SizedDecoder::new();
    decoder.fail("a.cr2");
    let mut env = MainEnv::with_options(EnvOptions {
        decoder: decoder.clone(),
        settings: Box::new(|settings| {
            pair_settings(settings);
            // The full decode is the one the fake can fail: Original mode decodes without a box.
            settings.loading_mode = Some(LoadingMode::Original);
        }),
        ..EnvOptions::default()
    });
    let (folder, files) = env.images("pair_broken", &["a.jpg", "a.cr2"]);
    env.open(&folder);
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[0].as_str()));

    env.vm.toggle_capture_group_member();
    env.settle();

    assert_eq!(
        env.vm.presenter().current_presented_path(),
        Some(files[0].as_str()),
        "the JPEG is still what is on screen"
    );
    assert_eq!(env.vm.capture_pair_badge(), env.text("main.capturePair.badge.jpeg"));
}

#[test]
fn toggle_capture_group_member_is_a_no_op_while_compare_is_visible() {
    // New test (C# line 335: `_compare.IsVisible` returns)
    let (mut env, files) = pair_env(false);
    env.vm.toggle_compare();
    env.settle();
    assert!(env.vm.compare().is_visible());
    let interaction = env.clock.current_interaction();

    let changes = env.vm.toggle_capture_group_member();
    env.settle();

    assert!(changes.is_empty());
    assert_eq!(env.clock.current_interaction(), interaction);
    assert_eq!(env.vm.compare().left_path(), Some(files[0].as_str()));
}

#[test]
fn toggle_capture_group_member_without_a_capture_group_is_a_no_op() {
    // New test (C# line 335: the current entry has no capture group)
    let mut env = MainEnv::new();
    let (folder, _) = env.images("plain", &["a.png", "b.png"]);
    env.open(&folder);
    let interaction = env.clock.current_interaction();

    assert!(env.vm.toggle_capture_group_member().is_empty());

    assert_eq!(env.clock.current_interaction(), interaction);
}

#[test]
fn toggle_compare_when_the_presented_path_belongs_to_another_entry_compares_the_current_entry_without_throwing() {
    // C#: MainViewModelAdvancedTests.RawPair.ToggleCompare_WhenPresentedPathBelongsToAnotherEntry_ComparesTheCurrentEntryWithoutThrowing
    let (mut env, files) = pair_env(true);
    assert_eq!(env.vm.presenter().current_presented_path(), Some(files[0].as_str()));

    // The catalog moved on (probe / undo / etc.) while the presenter still reports entry A's path.
    env.vm.catalog_mut_for_tests().set_current(1);
    let changes = env.vm.toggle_compare();
    env.settle();

    assert!(
        changes
            .compare
            .contains(pr_app::viewmodels::compare::CompareProperty::IsVisible)
    );
    assert!(env.vm.compare().is_visible());
    assert_eq!(env.vm.compare().left_path(), Some(files[2].as_str()));
    assert_eq!(env.vm.compare().right_path(), Some(files[3].as_str()));
}

#[test]
fn compare_on_capture_then_next_keeps_comparing_the_next_capture_pair() {
    // C#: MainViewModelAdvancedTests.RawSettings.Compare_OnCaptureThenNext_KeepsComparingTheNextCapturePair
    let (mut env, files) = pair_env(true);

    env.vm.toggle_compare();
    env.settle();
    assert_eq!(env.vm.compare().right_path(), Some(files[1].as_str()));

    env.vm.next();
    env.settle();

    assert!(env.vm.compare().is_visible());
    // The pair used to be dropped on navigation while Compare stayed visible.
    assert_eq!(env.vm.compare().left_path(), Some(files[2].as_str()));
    assert_eq!(env.vm.compare().right_path(), Some(files[3].as_str()));
}

// ---- sibling folders --------------------------------------------------------------------------------------------------------

#[test]
fn navigate_sibling_folder_boundary_displays_status() {
    // C#: MainViewModelNavigationTests.NavigateSiblingFolderAsync_Boundary_DisplaysStatus
    let mut env = MainEnv::new();
    let parent = env.folder("siblings");
    let single = Path::new(&parent).join("only_one");
    std::fs::create_dir_all(&single).unwrap_or_else(|error| panic!("create: {error}"));
    env.add_image(&single.to_string_lossy(), "pic.jpg");
    env.open(&single.to_string_lossy());
    let fixture = FormatFixture::new();

    env.vm
        .navigate_sibling_folder(1)
        .unwrap_or_else(|error| panic!("navigate: {error}"));
    env.settle();
    assert_eq!(env.vm.status_text(), sibling_folder_boundary(1, &fixture.ctx()));

    env.vm
        .navigate_sibling_folder(-1)
        .unwrap_or_else(|error| panic!("navigate: {error}"));
    env.settle();
    assert_eq!(env.vm.status_text(), sibling_folder_boundary(-1, &fixture.ctx()));
}

#[test]
fn navigate_sibling_folder_with_a_bad_direction_is_an_error() {
    // New test (RV-A08: direction 0 never advanced the loop)
    let mut env = MainEnv::new();
    let (folder, _) = env.images("dir_error", &["a.png"]);
    env.open(&folder);

    assert_eq!(
        env.vm.navigate_sibling_folder(0).unwrap_err(),
        SiblingNavError::InvalidDirection(0)
    );
    assert_eq!(
        env.vm.navigate_sibling_folder(2).unwrap_err(),
        SiblingNavError::InvalidDirection(2)
    );
    assert_eq!(env.runner.pending(), 0, "no search was submitted");
}

#[test]
fn navigate_sibling_folder_result_after_a_folder_switch_is_dropped() {
    // New test (INV-5 style: the search result of a folder the user already left changes nothing)
    let mut env = MainEnv::new();
    let parent = env.folder("sibling_switch");
    let one = Path::new(&parent).join("one");
    let two = Path::new(&parent).join("two");
    for folder in [&one, &two] {
        std::fs::create_dir_all(folder).unwrap_or_else(|error| panic!("create: {error}"));
        env.add_image(&folder.to_string_lossy(), "pic.jpg");
    }
    env.open(&one.to_string_lossy());
    env.vm
        .navigate_sibling_folder(1)
        .unwrap_or_else(|error| panic!("navigate: {error}"));
    assert_eq!(env.runner.count_of("sibling-folder-search"), 1);
    let other = env.folder("elsewhere");
    env.add_image(&other, "x.png");
    env.vm
        .open_folder(&other, None)
        .unwrap_or_else(|error| panic!("open: {error}"));

    assert!(env.runner.run_tagged("sibling-folder-search"));
    env.settle();

    assert_eq!(env.names(), vec!["x.png"], "the sibling open was not performed");
    assert!(!env.vm.status_text().contains("sibling"));
}

// ---- misc -------------------------------------------------------------------------------------------------------------------

#[test]
fn folder_ownership_accessors_round_trip() {
    // New test (C# FolderOwnership property, Q-R18)
    let mut env = MainEnv::new();
    assert!(env.vm.folder_ownership().is_none());
    let ownership = RecordingOwnership::new();
    env.vm.set_folder_ownership(Some(ownership.clone()));
    assert!(env.vm.folder_ownership().is_some());
    env.vm.set_folder_ownership(None);
    assert!(env.vm.folder_ownership().is_none());
    assert!(env.vm.shown_folder().is_none());
    let _ = Arc::strong_count(&ownership);
}

// ---- zoom swap through the view model (needs the presenter units and the zoom commands of W8-08c) ---------------------------------

/// `(original width, original height, decoded width, decoded height)` of one file.
type SwapSizes = (i32, i32, i32, i32);

/// C# `SizedDecoder` of `MainViewModelZoomSwapTests`: a bounded decode is the 600 px preview of the file's original size, an unbounded one is the full decode
/// (the file's own decoded size) held on the gate of its file.
struct SwapDecoder {
    sizes: std::collections::HashMap<String, SwapSizes>,
    original_gate: Arc<support::preview_fakes::Gate>,
    next_gate: Arc<support::preview_fakes::Gate>,
}

impl SwapDecoder {
    fn new(sizes: &[(&str, SwapSizes)]) -> Arc<Self> {
        Arc::new(Self {
            sizes: sizes.iter().map(|(name, size)| ((*name).to_owned(), *size)).collect(),
            original_gate: support::preview_fakes::Gate::new(),
            next_gate: support::preview_fakes::Gate::new(),
        })
    }

    fn size_of(&self, path: &str) -> SwapSizes {
        let name = Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.sizes.get(&name).copied().unwrap_or((6000, 4000, 6000, 4000))
    }
}

impl pr_imaging::decoder_traits::ImageDecoder for SwapDecoder {
    fn decode(
        &self,
        request: &pr_imaging::decode_types::DecodeRequest,
    ) -> Result<Arc<dyn pr_imaging::decode_types::DecodedImageView>, pr_imaging::decoder_traits::DecodeError> {
        let path = request.path.to_string_lossy();
        let (original_width, original_height, decoded_width, decoded_height) = self.size_of(&path);
        if request.decode_box().is_unbounded() {
            let gate = if path.ends_with("a.jpg") {
                &self.original_gate
            } else {
                &self.next_gate
            };
            gate.wait();
            return Ok(support::preview_fakes::sized_image(
                decoded_width,
                decoded_height,
                decoded_width,
                decoded_height,
                false,
            ));
        }
        Ok(support::preview_fakes::sized_image(
            600,
            600 * original_height / original_width,
            original_width,
            original_height,
            true,
        ))
    }

    fn read_info(
        &self,
        path: &str,
    ) -> Result<pr_imaging::decode_types::ImageInfo, pr_imaging::decoder_traits::DecodeError> {
        let (width, height, _, _) = self.size_of(path);
        Ok(pr_imaging::decode_types::ImageInfo::new(width, height))
    }
}

/// A window whose catalog holds `a.jpg` and `b.jpg` (no folder opened) and the first photo presented.
fn zoom_swap_env(decoder: Arc<SwapDecoder>, zoom_step_percent: i32) -> MainEnv {
    let mut env = MainEnv::with_options(EnvOptions {
        decoder,
        settings: Box::new(move |settings| settings.keyboard_zoom_step_percent = zoom_step_percent),
        catalog_builder: Some(Box::new(|photos: &Path| {
            let folder = photos.join("zoom_swap");
            std::fs::create_dir_all(&folder).unwrap_or_else(|error| panic!("create: {error}"));
            let mut entries = Vec::new();
            for name in ["a.jpg", "b.jpg"] {
                let path = folder.join(name);
                std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write: {error}"));
                entries.push(
                    pr_core::catalog::types::CatalogEntry::new(path.to_string_lossy().into_owned())
                        .unwrap_or_else(|error| panic!("{error:?}")),
                );
            }
            let mut catalog = pr_core::catalog::review_catalog::ReviewCatalog::new();
            catalog.reset(entries, RawPairMode::Separate, None);
            catalog
        })),
        ..EnvOptions::default()
    });
    env.vm.first();
    env.settle();
    env
}

fn decoded_original_arriving_while_zoomed(decoded_width: i32, decoded_height: i32, zoom: f64) {
    let decoder = SwapDecoder::new(&[
        ("a.jpg", (6720, 4480, decoded_width, decoded_height)),
        ("b.jpg", (6000, 4000, 6000, 4000)),
    ]);
    // A step of 100 % makes one `zoom_in` from Fit land on 2.0; `zoom_actual_size` lands on 1.0.
    let mut env = zoom_swap_env(Arc::clone(&decoder), 100);
    let source = |env: &MainEnv| {
        (
            env.viewer.borrow().source_pixel_width(),
            env.viewer.borrow().source_pixel_height(),
        )
    };
    assert_eq!(source(&env), (6720, 4480));
    let swapping_seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = std::rc::Rc::clone(&swapping_seen);
    env.bridge.set_source_size_swapping_hook(Some(Box::new(move |before| {
        seen.borrow_mut().push(before.source_pixel_width());
    })));

    if zoom == 1.0 {
        env.vm.zoom_actual_size(); // leaves Fit: the view model asks the presenter for the full-resolution decode
    } else {
        env.vm.zoom_in();
    }
    assert!(env.vm.presenter().zoom_detail().pending_load());
    assert!(swapping_seen.borrow().is_empty());

    decoder.original_gate.open();
    env.settle();

    // (1) raised exactly once, while the layout still had the preview size
    assert_eq!(*swapping_seen.borrow(), vec![6720]);
    // (2) the decoded original's own size is now the source size; the user zoom factor is untouched
    assert_eq!(source(&env), (decoded_width, decoded_height));
    assert_eq!(env.viewer.borrow().zoom(), zoom);
    assert!(!env.viewer.borrow().is_fit());

    // (3) the next image is a new image: the set-source-size path, no swapping event
    decoder.next_gate.open();
    env.vm.next();
    env.settle();
    assert_eq!(source(&env), (6000, 4000));
    assert_eq!(swapping_seen.borrow().len(), 1);
}

#[test]
fn decoded_original_arriving_while_zoomed_swaps_the_source_size_and_the_next_image_does_not_larger_original() {
    // C#: ZoomSwapTests.DecodedOriginalArriving_WhileZoomed_SwapsTheSourceSizeThroughTheViewModel_AndTheNextImageDoesNot (6744x4502 at 2.0)
    decoded_original_arriving_while_zoomed(6744, 4502, 2.0);
}

#[test]
fn decoded_original_arriving_while_zoomed_swaps_the_source_size_and_the_next_image_does_not_smaller_original() {
    // C#: ZoomSwapTests.DecodedOriginalArriving_WhileZoomed_SwapsTheSourceSizeThroughTheViewModel_AndTheNextImageDoesNot (6719x4479 at 1.0)
    decoded_original_arriving_while_zoomed(6719, 4479, 1.0);
}

#[test]
fn navigating_after_fit_width_with_keep_zoom_forgets_the_previous_images_fit_axis() {
    // C#: ZoomSwapTests.Navigating_AfterFitWidthWithKeepZoom_ForgetsThePreviousImagesFitAxis
    let decoder = SwapDecoder::new(&[("a.jpg", (6000, 4000, 6000, 4000)), ("b.jpg", (6000, 4000, 6000, 4000))]);
    let mut env = zoom_swap_env(Arc::clone(&decoder), 10);
    env.viewer.borrow_mut().set_dpi_scale(1.0);
    env.viewer.borrow_mut().apply_initial_view_mode(
        pr_core::model::enums::InitialViewMode::FitWidth,
        1200.0,
        800.0,
        100,
        false,
    );
    let fit_width_zoom = env.viewer.borrow().zoom();
    assert!((fit_width_zoom - 1200.0 / 6000.0).abs() < 1e-6);

    env.vm.next(); // KeepZoomAcrossImages: the zoom is kept, no apply_initial_view_mode
    env.settle();
    env.viewer.borrow_mut().swap_source_size(6016, 4016, &mut |_| {}); // B's original arrives

    assert!(
        (env.viewer.borrow().zoom() - fit_width_zoom).abs() < 1e-9,
        "not refitted against A's viewport"
    );
}
