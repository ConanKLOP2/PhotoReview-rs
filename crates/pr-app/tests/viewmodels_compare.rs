//! C# CompareViewModel tests at baseline d92d6aae, adapted to the staged Rust API.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pr_app::misc::file_hash::{FileHashError, FileHasher};
use pr_app::viewmodels::compare::{
    CompareChanges, CompareError, CompareLoadOptions, CompareLoadStep, CompareNext, CompareOutcome,
    CompareProperty as P, CompareViewModel, CompareWork, CompareWorkResult, HashOutcome, hash_outcome,
    try_get_file_size,
};
use pr_app::viewmodels::format::context::{DateText, FormatContext};
use pr_app::viewmodels::format::status::{
    self, CompareStatus, compare_hash_text, compare_hash_unknown, compare_size_suffix,
};
use pr_core::abstractions::{AppendStream, FileStat, FileSystem, Log, ReadSeek};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::decode_types::{DecodedImage, DecodedImageOptions, PlatformImage};
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;

const LEFT: &str = r"C:\photos\img1.jpg";
const RIGHT: &str = r"C:\photos\img2.jpg";
const RAW: &str = r"C:\photos\img1.cr2";

#[derive(Debug)]
struct Dates;
impl DateText for Dates {
    fn general(&self, _: DateTimeUtc) -> Option<String> {
        None
    }
    fn invariant_general(&self, _: DateTimeUtc) -> String {
        String::new()
    }
}

fn with_ctx<R>(f: impl FnOnce(&FormatContext<'_>) -> R) -> R {
    let mut warnings = Vec::new();
    let catalog = LanguageCatalog::try_parse(
        include_str!("../../../assets/languages/en.json"),
        "en.json",
        &mut warnings,
    )
    .unwrap();
    let texts = Localizer::create(&catalog, &[], &warnings);
    let numbers = NumberCulture::invariant();
    let dates = Dates;
    f(&FormatContext {
        texts: &texts,
        numbers: &numbers,
        group_separator: ",",
        dates: &dates,
    })
}

fn image() -> Arc<DecodedImage> {
    Arc::new(
        DecodedImage::new(
            PlatformImage::Opaque(Arc::new(())),
            1,
            1,
            4,
            DecodedImageOptions::default(),
        )
        .unwrap(),
    )
}

fn pair() -> (String, String) {
    (LEFT.into(), RIGHT.into())
}
fn options() -> CompareLoadOptions {
    CompareLoadOptions {
        current_index: 0,
        total_files: 10,
        ..Default::default()
    }
}
fn images(left: Option<Arc<DecodedImage>>, right: Option<Arc<DecodedImage>>) -> CompareWorkResult {
    CompareWorkResult::Images {
        left: Ok(left),
        right: Ok(right),
    }
}
fn hashes(left: HashOutcome, right: HashOutcome) -> CompareWorkResult {
    CompareWorkResult::Hashes { left, right }
}
fn done(step: CompareLoadStep) -> CompareOutcome {
    match step.next {
        CompareNext::Done(outcome) => outcome,
        CompareNext::Await(load) => panic!("unexpected work: {:?}", load.work()),
    }
}
fn next_work(step: CompareLoadStep) -> pr_app::viewmodels::compare::CompareLoad {
    match step.next {
        CompareNext::Await(load) => load,
        CompareNext::Done(outcome) => panic!("unexpected outcome: {outcome:?}"),
    }
}
fn drive(
    vm: &mut CompareViewModel,
    pair: (String, String),
    opts: CompareLoadOptions,
    current: &dyn Fn(i64) -> bool,
    ctx: &FormatContext<'_>,
    mut answer: impl FnMut(&CompareWork) -> CompareWorkResult,
) -> (CompareOutcome, Vec<CompareWork>) {
    let mut step = vm.begin_load(pair, 7, current, opts, ctx);
    let mut work = Vec::new();
    loop {
        match step.next {
            CompareNext::Done(outcome) => return (outcome, work),
            CompareNext::Await(load) => {
                let request = load.work();
                let result = answer(&request);
                work.push(request);
                step = vm.resume_load(load, result, current, ctx);
            }
        }
    }
}

#[test]
// C#: no twin; staged form of LoadAsync starting both preview loaders before awaiting them
fn begin_load_requests_both_previews_and_retains_token() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let load = next_work(vm.begin_load(pair(), 7, &|_| true, options(), ctx));
        assert_eq!(load.token(), 7);
        assert_eq!(
            load.work(),
            CompareWork::LoadImages {
                left: LEFT.into(),
                right: RIGHT.into(),
            }
        );
    });
}
fn normal_answer(work: &CompareWork) -> CompareWorkResult {
    match work {
        CompareWork::LoadImages { .. } => images(Some(image()), Some(image())),
        CompareWork::StatSizes { .. } => CompareWorkResult::Sizes {
            left: Some(1234),
            right: None,
        },
        CompareWork::ComputeHashes { .. } => hashes(HashOutcome::Hash("ABC".into()), HashOutcome::Hash("abc".into())),
    }
}
fn changes(properties: &[P]) -> CompareChanges {
    let mut result = CompareChanges::EMPTY;
    for &property in properties {
        result.insert(property);
    }
    result
}

#[test]
// C#: CompareViewModelTests.DefaultState_AllPropertiesEmptyOrNull_NotVisible
fn default_state_all_properties_empty_or_null_not_visible() {
    with_ctx(|ctx| {
        let vm = CompareViewModel::new(ctx);
        assert!(!vm.is_visible());
        assert_eq!(vm.left_path(), None);
        assert_eq!(vm.right_path(), None);
        assert!(vm.left_image().is_none() && vm.right_image().is_none());
        assert_eq!(vm.selected_path(), None);
        assert!(!vm.is_left_selected() && !vm.is_right_selected());
        assert_eq!(vm.left_size_text(), "");
        assert_eq!(vm.right_size_text(), "");
        assert_eq!(vm.size_text(ctx), " ↔ ");
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
        assert_eq!(vm.status_text(), "");
    });
}

#[test]
// C#: CompareViewModelTests.Toggle_FlipsIsVisible
fn toggle_flips_is_visible() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        assert_eq!(vm.toggle(), changes(&[P::IsVisible]));
        assert!(vm.is_visible());
        assert_eq!(vm.toggle(), changes(&[P::IsVisible]));
        assert!(!vm.is_visible());
    });
}

#[test]
// C#: CompareViewModelTests.Select_LeftAndRight_UpdatesSelectedPathAndFlags
fn select_left_and_right_updates_selected_path_and_flags() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        vm.set_left_path(Some(LEFT.into()));
        vm.set_right_path(Some(RIGHT.into()));
        let expected = changes(&[P::SelectedPath, P::IsLeftSelected, P::IsRightSelected]);
        assert_eq!(vm.select_left(), expected);
        assert!(vm.is_left_selected());
        assert!(!vm.is_right_selected());
        assert_eq!(vm.select_right(), expected);
        assert!(!vm.is_left_selected());
        assert!(vm.is_right_selected());
        assert_eq!(vm.select(Some(LEFT.into())), expected);
        assert_eq!(vm.selected_path(), Some(LEFT));
    });
}

#[test]
// C#: CompareViewModelTests.Clear_ResetsAllProperties
fn clear_resets_all_properties() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let (outcome, _) = drive(&mut vm, pair(), options(), &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(vm.clear(ctx), CompareChanges::all());
        assert!(!vm.is_visible());
        assert_eq!(vm.left_path(), None);
        assert_eq!(vm.right_path(), None);
        assert!(vm.left_image().is_none() && vm.right_image().is_none());
        assert_eq!(vm.selected_path(), None);
        assert!(!vm.is_left_selected() && !vm.is_right_selected());
        assert_eq!(vm.left_size_text(), "");
        assert_eq!(vm.right_size_text(), "");
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
        assert_eq!(vm.status_text(), "");
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_LoadsPreviewsConcurrently_AndSetsProperties
fn load_loads_previews_concurrently_and_sets_properties() {
    with_ctx(|ctx| {
        let left = image();
        let right = image();
        let mut vm = CompareViewModel::new(ctx);
        let (outcome, work) = drive(&mut vm, pair(), options(), &|_| true, ctx, |_| {
            images(Some(left.clone()), Some(right.clone()))
        });
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(
            work,
            vec![CompareWork::LoadImages {
                left: LEFT.into(),
                right: RIGHT.into()
            }]
        );
        assert!(Arc::ptr_eq(vm.left_image().unwrap(), &left));
        assert!(Arc::ptr_eq(vm.right_image().unwrap(), &right));
        assert_eq!(vm.left_path(), Some(LEFT));
        assert_eq!(vm.right_path(), Some(RIGHT));
        assert_eq!(vm.selected_path(), Some(LEFT));
        assert!(vm.is_visible());
        assert_eq!(
            vm.status_text(),
            "1/10 | Compare | img1.jpg ↔ img2.jpg | hash off | click to select"
        );
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_WithCompareSize_FormatsSizesCorrectly
fn load_with_compare_size_formats_sizes_correctly() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_size_enabled = true;
        opts.get_file_size = Some(Arc::new(|path| if path == LEFT { Some(1234) } else { Some(5678) }));
        let (outcome, work) = drive(&mut vm, pair(), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.left_size_text(), compare_size_suffix(1234, ctx));
        assert_eq!(vm.right_size_text(), compare_size_suffix(5678, ctx));
        assert_eq!(
            vm.status_text(),
            status::compare(
                &CompareStatus {
                    index: 0,
                    count: 10,
                    left_name: "img1.jpg",
                    left_size: &compare_size_suffix(1234, ctx),
                    right_name: "img2.jpg",
                    right_size: &compare_size_suffix(5678, ctx),
                    hash_text: &compare_hash_text(None, ctx)
                },
                ctx
            )
        );
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_WithCompareSizeAndNoInjectedSizes_StatsTheFiles
fn load_with_compare_size_and_no_injected_sizes_stats_the_files() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_size_enabled = true;
        let fs = FakeFs::new(vec![(LEFT.into(), Some(1234))], None);
        let log = RecordingLog::default();
        let (outcome, work) = drive(&mut vm, pair(), opts, &|_| true, ctx, |request| match request {
            CompareWork::StatSizes { left, right } => CompareWorkResult::Sizes {
                left: try_get_file_size(left, &fs, &log),
                right: try_get_file_size(right, &fs, &log),
            },
            _ => normal_answer(request),
        });
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(
            work.get(1),
            Some(&CompareWork::StatSizes {
                left: LEFT.into(),
                right: RIGHT.into()
            })
        );
        assert_eq!(vm.left_size_text(), compare_size_suffix(1234, ctx));
        assert_eq!(vm.right_size_text(), "");
    });
}

fn hash_case(left: HashOutcome, right: HashOutcome) -> (String, CompareOutcome, Vec<CompareWork>) {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        opts.hash_source_available = true;
        let (outcome, work) = drive(&mut vm, pair(), opts, &|_| true, ctx, |w| match w {
            CompareWork::ComputeHashes { .. } => hashes(left.clone(), right.clone()),
            _ => normal_answer(w),
        });
        (vm.hash_text().into(), outcome, work)
    })
}

#[test]
// C#: CompareViewModelTests.LoadAsync_WithCompareHash_MatchingHashes_SetsTrung
fn load_with_compare_hash_matching_hashes_sets_same() {
    with_ctx(|ctx| {
        let (text, outcome, work) = hash_case(HashOutcome::Hash("ABC".into()), HashOutcome::Hash("abc".into()));
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(text, compare_hash_text(Some(true), ctx));
        assert_eq!(
            work.get(1),
            Some(&CompareWork::ComputeHashes {
                left: LEFT.into(),
                right: RIGHT.into()
            })
        );
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_WithCompareHash_DifferentHashes_SetsKhac
fn load_with_compare_hash_different_hashes_sets_different() {
    with_ctx(|ctx| {
        let (text, outcome, _) = hash_case(HashOutcome::Hash("ABC".into()), HashOutcome::Hash("XYZ".into()));
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(text, compare_hash_text(Some(false), ctx));
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_WithCompareHash_JpegAndRawPair_DoesNotHashEitherFile
fn load_with_compare_hash_jpeg_and_raw_pair_does_not_hash_either_file() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        opts.hash_source_available = true;
        let (outcome, work) = drive(&mut vm, (LEFT.into(), RAW.into()), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.hash_text(), compare_hash_text(Some(false), ctx));
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_TokenMismatch_AbortsBeforeImageSet
fn load_token_mismatch_aborts_before_image_set() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let calls = std::cell::Cell::new(0);
        let current = |_: i64| {
            calls.set(calls.get() + 1);
            calls.get() == 1
        };
        let (outcome, work) = drive(&mut vm, pair(), options(), &current, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Superseded);
        assert_eq!(work.len(), 1);
        assert!(vm.left_image().is_none() && vm.right_image().is_none());
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_TokenMismatch_AbortsBeforeHashSet
fn load_token_mismatch_aborts_before_hash_set() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        opts.hash_source_available = true;
        let calls = std::cell::Cell::new(0);
        let current = |_: i64| {
            calls.set(calls.get() + 1);
            calls.get() < 3
        };
        let (outcome, work) = drive(&mut vm, pair(), opts, &current, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Superseded);
        assert_eq!(work.len(), 2);
        assert!(vm.left_image().is_some());
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_HashFailsAfterPreviewsLoaded_KeepsPreviewsAndSizesAndShowsUnknownHash
fn load_hash_fails_after_previews_loaded_keeps_previews_and_sizes_and_shows_unknown_hash() {
    with_ctx(|ctx| {
        for (fail_left, fail_right) in [(true, false), (false, true), (true, true)] {
            let mut vm = CompareViewModel::new(ctx);
            let mut opts = options();
            opts.compare_size_enabled = true;
            opts.compare_hash_enabled = true;
            opts.hash_source_available = true;
            opts.get_file_size = Some(Arc::new(|p| if p == LEFT { Some(42) } else { Some(77) }));
            let (outcome, _) = drive(&mut vm, pair(), opts, &|_| true, ctx, |w| match w {
                CompareWork::ComputeHashes { .. } => hashes(
                    if fail_left {
                        HashOutcome::Failed
                    } else {
                        HashOutcome::Hash("A".into())
                    },
                    if fail_right {
                        HashOutcome::Failed
                    } else {
                        HashOutcome::Hash("A".into())
                    },
                ),
                _ => normal_answer(w),
            });
            assert_eq!(outcome, CompareOutcome::Completed);
            assert!(vm.left_image().is_some() && vm.right_image().is_some());
            assert_eq!(vm.left_size_text(), compare_size_suffix(42, ctx));
            assert_eq!(vm.right_size_text(), compare_size_suffix(77, ctx));
            assert_eq!(vm.hash_text(), compare_hash_unknown(ctx));
        }
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_PreviewFails_StillThrowsSoTheCallerCanClearTheComparison
fn load_preview_fails_still_reports_failed_so_the_caller_can_clear() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let (outcome, _) = drive(&mut vm, pair(), options(), &|_| true, ctx, |_| {
            CompareWorkResult::Images {
                left: Ok(Some(image())),
                right: Err(CompareError::Failed("decode".into())),
            }
        });
        assert_eq!(outcome, CompareOutcome::Failed(CompareError::Failed("decode".into())));
        assert!(vm.left_image().is_none() && vm.right_image().is_none());
        vm.clear(ctx);
        assert!(!vm.is_visible());
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_HashCanceled_StillPropagatesCancellation
fn load_hash_canceled_still_propagates_cancellation() {
    with_ctx(|ctx| {
        let (text, outcome, _) = hash_case(HashOutcome::Cancelled, HashOutcome::Hash("A".into()));
        assert_eq!(outcome, CompareOutcome::Cancelled);
        assert_eq!(text, compare_hash_text(None, ctx));
    });
}

#[test]
// C#: CompareViewModelTests.LoadAsync_HashFailsButNavigationMovedOn_ReturnsFalse
fn load_hash_fails_but_navigation_moved_on_returns_superseded() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        opts.hash_source_available = true;
        let calls = std::cell::Cell::new(0);
        let current = |_: i64| {
            calls.set(calls.get() + 1);
            calls.get() < 3
        };
        let (outcome, _) = drive(&mut vm, pair(), opts, &current, ctx, |w| match w {
            CompareWork::ComputeHashes { .. } => hashes(HashOutcome::Failed, HashOutcome::Hash("A".into())),
            _ => normal_answer(w),
        });
        assert_eq!(outcome, CompareOutcome::Superseded);
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.LeftPath_WhenSet_AlsoRefreshesTheLeftSelectionFlag
fn left_path_when_set_also_refreshes_the_left_selection_flag() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        vm.select(Some(LEFT.into()));
        assert_eq!(
            vm.set_left_path(Some(LEFT.into())),
            changes(&[P::LeftPath, P::IsLeftSelected])
        );
        assert!(vm.is_left_selected());
        assert!(vm.set_left_path(Some(LEFT.into())).is_empty());
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.RightPath_WhenSet_AlsoRefreshesTheRightSelectionFlag
fn right_path_when_set_also_refreshes_the_right_selection_flag() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        vm.select(Some(RIGHT.into()));
        assert_eq!(
            vm.set_right_path(Some(RIGHT.into())),
            changes(&[P::RightPath, P::IsRightSelected])
        );
        assert!(vm.is_right_selected());
        assert!(vm.set_right_path(Some(RIGHT.into())).is_empty());
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.LeftSizeText_WhenSet_AlsoRefreshesThePairedSizeText
fn left_size_text_when_set_also_refreshes_the_paired_size_text() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        assert_eq!(
            vm.set_left_size_text("left".into()),
            changes(&[P::LeftSizeText, P::SizeText])
        );
        assert_eq!(vm.size_text(ctx), "left ↔ ");
        assert!(vm.set_left_size_text("left".into()).is_empty());
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.RightSizeText_WhenSet_AlsoRefreshesThePairedSizeText
fn right_size_text_when_set_also_refreshes_the_paired_size_text() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        assert_eq!(
            vm.set_right_size_text("right".into()),
            changes(&[P::RightSizeText, P::SizeText])
        );
        assert_eq!(vm.size_text(ctx), " ↔ right");
        assert!(vm.set_right_size_text("right".into()).is_empty());
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.LoadAsync_WithAnInitialSelection_SelectsThatPathInsteadOfTheLeftOne
fn load_with_an_initial_selection_selects_that_path_instead_of_the_left_one() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.initial_selected_path = Some(RIGHT.into());
        let (outcome, _) = drive(&mut vm, pair(), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(vm.selected_path(), Some(RIGHT));
        assert!(!vm.is_left_selected() && vm.is_right_selected());
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.LoadAsync_WithHashDisabled_NeverCallsTheHashFunction
fn load_with_hash_disabled_never_asks_for_a_hash() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.hash_source_available = true;
        let (outcome, work) = drive(&mut vm, pair(), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.LoadAsync_WithHashEnabledButNoHashFunction_LeavesTheHashUndetermined
fn load_with_hash_enabled_but_no_hash_source_leaves_the_hash_undetermined() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        let (outcome, work) = drive(&mut vm, pair(), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
    });
}

#[test]
// C#: CompareViewModelMutationGapTests.LoadAsync_WhenTheSizeLookupThrows_OnlyLosesTheCosmeticSuffix
fn load_when_the_size_lookup_fails_only_loses_the_cosmetic_suffix() {
    with_ctx(|ctx| {
        let fs = FakeFs::new(
            vec![(LEFT.into(), Some(42))],
            Some((RIGHT.into(), io::ErrorKind::InvalidInput)),
        );
        let log = RecordingLog::default();
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_size_enabled = true;
        let (outcome, _) = drive(&mut vm, pair(), opts, &|_| true, ctx, |w| match w {
            CompareWork::StatSizes { left, right } => CompareWorkResult::Sizes {
                left: try_get_file_size(left, &fs, &log),
                right: try_get_file_size(right, &fs, &log),
            },
            _ => normal_answer(w),
        });
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(vm.left_size_text(), compare_size_suffix(42, ctx));
        assert_eq!(vm.right_size_text(), "");
        assert!(vm.left_image().is_some() && vm.right_image().is_some());
        assert!(
            log.messages()
                .iter()
                .any(|s| s == &format!("Compare file size unavailable path={RIGHT}"))
        );
    });
}

#[test]
// C#: AppRound3GapTests.Compare_RawAgainstJpeg_WithHashingOff_NeverClaimsTheyDiffer
fn compare_raw_against_jpeg_with_hashing_off_never_claims_they_differ() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.hash_source_available = true;
        let (outcome, work) = drive(&mut vm, (LEFT.into(), RAW.into()), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
    });
}

#[test]
// C#: AppRound3GapTests.Compare_RawAgainstJpeg_WithoutAHashFunction_NeverClaimsTheyDiffer
fn compare_raw_against_jpeg_without_a_hash_function_never_claims_they_differ() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        let (outcome, work) = drive(&mut vm, (LEFT.into(), RAW.into()), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.hash_text(), compare_hash_text(None, ctx));
    });
}

#[test]
// C#: AppRound3GapTests.Compare_RawAgainstJpeg_WithHashingOn_SaysTheyDifferWithoutHashing
fn compare_raw_against_jpeg_with_hashing_on_says_they_differ_without_hashing() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_hash_enabled = true;
        opts.hash_source_available = true;
        let (outcome, work) = drive(&mut vm, (LEFT.into(), RAW.into()), opts, &|_| true, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(work.len(), 1);
        assert_eq!(vm.hash_text(), compare_hash_text(Some(false), ctx));
    });
}

#[test]
// C#: AppRound3GapTests.Compare_WhenOneHashThrows_ShowsTheHashAsUnknownAndStillLoads
fn compare_when_one_hash_throws_shows_the_hash_as_unknown_and_still_loads() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let log = RecordingLog::default();
        let mut opts = options();
        opts.compare_hash_enabled = true;
        opts.hash_source_available = true;
        let hasher = FakeHasher {
            result: Err(FileHashError::Io(Arc::new(io::Error::other("hash failed")))),
        };
        let cancel = CancelToken::new();
        let (outcome, _) = drive(&mut vm, pair(), opts, &|_| true, ctx, |w| match w {
            CompareWork::ComputeHashes { left, .. } => hashes(
                hash_outcome(&hasher, left, &cancel, &log),
                HashOutcome::Hash("ABC".into()),
            ),
            _ => normal_answer(w),
        });
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(vm.hash_text(), compare_hash_unknown(ctx));
        assert!(
            log.messages()
                .iter()
                .any(|s| s == &format!("Compare hash failed path={LEFT}"))
        );
    });
}

#[test]
// C#: AppRound3GapTests.Compare_TryGetFileSize_SwallowsTheExpectedStatFailures
fn try_get_file_size_swallows_the_expected_stat_failures() {
    for kind in [
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::Other,
        io::ErrorKind::Unsupported,
        io::ErrorKind::InvalidInput,
    ] {
        let fs = FakeFs::new(vec![], Some((LEFT.into(), kind)));
        let log = RecordingLog::default();
        assert_eq!(try_get_file_size(LEFT, &fs, &log), None);
        assert_eq!(
            log.messages(),
            vec![format!("Compare file size unavailable path={LEFT}")]
        );
    }
}

#[test]
// C#: AppRound3GapTests.Compare_TryGetFileSize_DoesNotHideABug_AndReturnsTheLength
fn try_get_file_size_returns_the_length_and_none_for_a_missing_file() {
    let fs = FakeFs::new(vec![(LEFT.into(), Some(42))], None);
    let log = RecordingLog::default();
    assert_eq!(try_get_file_size(LEFT, &fs, &log), Some(42));
    assert_eq!(try_get_file_size(RIGHT, &fs, &log), None);
    assert!(log.messages().is_empty());
    // C# InvalidOperationException pass-through has no Rust twin: io::Result has only expected I/O errors.
}

#[test]
// C#: no twin; T1 entry token check
fn begin_load_with_a_stale_token_touches_nothing() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let step = vm.begin_load(pair(), 7, &|_| false, options(), ctx);
        assert_eq!(done(step), CompareOutcome::Superseded);
        assert_eq!(vm.left_path(), None);
        assert_eq!(vm.right_path(), None);
        assert!(!vm.is_visible());
    });
}

#[test]
// C#: no twin; T1 injected-size path has no extra token check
fn injected_sizes_are_read_without_a_token_check() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_size_enabled = true;
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = reads.clone();
        opts.get_file_size = Some(Arc::new(move |_| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(5)
        }));
        let checks = std::cell::Cell::new(0);
        let current = |_: i64| {
            checks.set(checks.get() + 1);
            checks.get() <= 2
        };
        let (outcome, _) = drive(&mut vm, pair(), opts, &current, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Completed);
        assert_eq!(checks.get(), 2);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 2);
    });
}

#[test]
// C#: no twin; T1 an already applied stage remains visible after supersession
fn superseded_load_keeps_what_it_already_applied() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let mut opts = options();
        opts.compare_size_enabled = true;
        let checks = std::cell::Cell::new(0);
        let current = |_: i64| {
            checks.set(checks.get() + 1);
            checks.get() <= 2
        };
        let (outcome, _) = drive(&mut vm, pair(), opts, &current, ctx, normal_answer);
        assert_eq!(outcome, CompareOutcome::Superseded);
        assert!(vm.is_visible());
        assert_eq!(vm.left_path(), Some(LEFT));
        assert!(vm.left_image().is_some());
        assert_eq!(vm.left_size_text(), "");
        assert_eq!(vm.status_text(), "");
    });
}

#[test]
// C#: no twin; T6 Clear always notifies all properties
fn clear_from_the_default_state_still_reports_all_thirteen_properties() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        assert_eq!(P::ALL.len(), 13);
        assert_eq!(vm.clear(ctx), CompareChanges::all());
        assert_eq!(vm.clear(ctx).iter().collect::<Vec<_>>(), P::ALL);
    });
}

#[test]
// C#: no twin; T4 selected flags use ordinal ignore case, setters use case-sensitive equality
fn selection_ignores_case_but_path_setter_does_not() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        vm.set_left_path(Some(LEFT.into()));
        vm.select(Some(LEFT.to_uppercase()));
        assert!(vm.is_left_selected());
        assert_eq!(
            vm.set_left_path(Some(LEFT.to_uppercase())),
            changes(&[P::LeftPath, P::IsLeftSelected])
        );
        assert!(vm.is_left_selected());
        assert!(vm.set_left_path(Some(LEFT.to_uppercase())).is_empty());
    });
}

#[test]
// C#: no twin; T5 image property compares references
fn set_image_with_the_same_arc_reports_nothing_and_a_different_arc_reports_it() {
    with_ctx(|ctx| {
        let mut vm = CompareViewModel::new(ctx);
        let first = image();
        let second = image();
        assert_eq!(vm.set_left_image(Some(first.clone())), changes(&[P::LeftImage]));
        assert!(vm.set_left_image(Some(first)).is_empty());
        assert_eq!(vm.set_left_image(Some(second)), changes(&[P::LeftImage]));
    });
}

#[test]
// C#: no twin; T3 hash helper distinguishes success, cancellation and failure
fn hash_outcome_maps_cancel_failure_and_success() {
    let cancel = CancelToken::new();
    let log = RecordingLog::default();
    let success = FakeHasher {
        result: Ok("ABC".into()),
    };
    assert_eq!(
        hash_outcome(&success, LEFT, &cancel, &log),
        HashOutcome::Hash("ABC".into())
    );
    let canceled = FakeHasher {
        result: Err(FileHashError::Canceled),
    };
    assert_eq!(hash_outcome(&canceled, LEFT, &cancel, &log), HashOutcome::Cancelled);
    let failed = FakeHasher {
        result: Err(FileHashError::Io(Arc::new(io::Error::other("read")))),
    };
    assert_eq!(hash_outcome(&failed, LEFT, &cancel, &log), HashOutcome::Failed);
    assert_eq!(log.messages(), vec![format!("Compare hash failed path={LEFT}")]);
}

#[derive(Default)]
struct RecordingLog(Mutex<Vec<String>>);
impl RecordingLog {
    fn messages(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}
impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, message: &str, _: Option<&(dyn std::error::Error + 'static)>) {
        self.0.lock().unwrap().push(message.into());
    }
}

struct FakeHasher {
    result: Result<String, FileHashError>,
}
impl FileHasher for FakeHasher {
    fn get(&self, _: &Path, _: &CancelToken) -> Result<String, FileHashError> {
        self.result.clone()
    }
}

struct FakeFs {
    sizes: Vec<(String, Option<u64>)>,
    error: Option<(String, io::ErrorKind)>,
}
impl FakeFs {
    fn new(sizes: Vec<(String, Option<u64>)>, error: Option<(String, io::ErrorKind)>) -> Self {
        Self { sizes, error }
    }
    fn stat(&self, path: &Path) -> Option<FileStat> {
        let length = self.sizes.iter().find(|(p, _)| Path::new(p) == path)?.1?;
        Some(FileStat {
            length,
            last_write_utc: DateTimeUtc::MIN,
        })
    }
}
impl FileSystem for FakeFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.stat(path).is_some()
    }
    fn directory_exists(&self, _: &Path) -> bool {
        false
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.stat(path)
    }
    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        if let Some((name, kind)) = &self.error {
            if Path::new(name) == path {
                return Err(io::Error::new(*kind, "stat"));
            }
        }
        Ok(self.stat(path))
    }
    fn move_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("unused"))
    }
    fn copy_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("unused"))
    }
    fn delete(&self, _: &Path) -> io::Result<()> {
        Err(io::Error::other("unused"))
    }
    fn open_read_shared(&self, _: &Path, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        Err(io::Error::other("unused"))
    }
    fn open_append(&self, _: &Path, _: bool) -> io::Result<Box<dyn AppendStream>> {
        Err(io::Error::other("unused"))
    }
    fn write_all_text_atomic(&self, _: &Path, _: &str, _: bool) -> io::Result<()> {
        Err(io::Error::other("unused"))
    }
    fn read_all_text(&self, _: &Path) -> io::Result<String> {
        Err(io::Error::other("unused"))
    }
    fn enumerate_files(&self, _: &Path, _: &str) -> io::Result<Vec<PathBuf>> {
        Err(io::Error::other("unused"))
    }
    fn enumerate_directories(&self, _: &Path) -> io::Result<Vec<PathBuf>> {
        Err(io::Error::other("unused"))
    }
    fn create_directory(&self, _: &Path) -> io::Result<()> {
        Err(io::Error::other("unused"))
    }
}
