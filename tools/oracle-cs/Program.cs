using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;

// Usage: oracle-cs <command> <inputs.json> <output.json> <csharp-commit>
// Each command reads its case list from tests/golden-inputs/<command>.json and writes tests/golden/<command>.json:
//   { "generator": "oracle-cs <command>", "csharp_commit": "<sha>", "cases": [ { ...input, ...expected } ] }
// Rust tests read the golden file only; they never run this tool.

if (args.Length != 4)
{
    Console.Error.WriteLine("usage: oracle-cs <command> <inputs.json> <output.json> <csharp-commit>");
    return 2;
}

var (command, inputPath, outputPath, commit) = (args[0], args[1], args[2], args[3]);
var inputs = JsonNode.Parse(File.ReadAllText(inputPath))!.AsObject();
Commands.FormatInputsDir = Path.GetDirectoryName(Path.GetFullPath(inputPath))!;
PerfCommands.FixturesDir = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, "..", "fixtures");

if (command == "ordinal-casing-table")
{
    // Writes Rust source, not JSON: the table pr_core::text uses to reproduce StringComparison.OrdinalIgnoreCase.
    var rustPath = inputs["rust_output"]!.GetValue<string>();
    var full = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(inputPath))!, rustPath);
    var count = Commands.WriteOrdinalCasingTable(full, commit);
    Console.WriteLine($"{command}: {count} mapping(s) -> {full}");
    return 0;
}

JsonArray cases = command switch
{
    "datetime-json" => Commands.DateTimeJson(inputs),
    "ordinal-ignore-case" => Commands.OrdinalIgnoreCase(inputs),
    "natural-cmp" => Commands.NaturalCompare(inputs),
    // Wave 1 commands: one file per unit in tools/oracle-cs/Commands/. Later waves add their own file and a line here.
    // W1-01..05 contracts
    "model-enums" => Commands.ModelEnums(inputs),
    "lenient-enum" => Commands.LenientEnum(inputs),
    "flag-enum-json" => Commands.FlagEnumJson(inputs),
    "flag-fields" => Commands.FlagFields(inputs),
    "shortcut-canonical" => Commands.ShortcutCanonical(inputs),
    "perf-options" => Commands.PerfOptions(inputs),
    // W1-06 .. W1-08
    "natural-key" => Commands.NaturalKey(inputs),
    "image-file-types" => Commands.ImageFileTypesCases(inputs),
    "lru-trace" => Commands.LruTrace(inputs),
    // W1-10, W1-11
    "app-version" => Commands.AppVersionCases(inputs),
    "update-url-policy" => Commands.UpdateUrlPolicyCases(inputs),
    "loc-template" => Commands.LocTemplateCases(inputs),
    "loc-render" => Commands.LocRenderCases(inputs),
    "loc-pseudo" => Commands.LocPseudoCases(inputs),
    // W1-13, W1-14, W1-15, W1-22
    "path-root" => Commands.PathRoot(inputs),
    "journal-error-text" => Commands.JournalErrorText(inputs),
    "journal-lines" => Commands.JournalLineCases(inputs),
    "percentile" => Commands.PercentileCases(inputs),
    "diag-options" => Commands.DiagOptionsCases(inputs),
    "vblank" => Commands.VBlankCases(inputs),
    // W1-16 .. W1-21 (PerfAnalysis, tools/oracle-cs/Commands/Perf.cs)
    "perf-csv" => PerfCommands.Csv(inputs),
    "perf-nav" => PerfCommands.Nav(inputs),
    "perf-stats" => PerfCommands.Stats(inputs),
    "perf-rules" => PerfCommands.Rules(inputs),
    "perf-report" => PerfCommands.Report(inputs),
    "perf-analyze" => PerfCommands.Analyze(inputs),
    // Wave 2 commands (W2-00..W2-20)
    "capture-groups" => Commands.CaptureGroups(inputs),
    "compare-pair" => Commands.ComparePair(inputs),
    "context-menu" => Commands.ContextMenuCases(inputs),
    "destination-policy" => Commands.DestinationPolicy(inputs),
    "dotnet-path" => Commands.DotnetPath(inputs),
    "drag-drop" => Commands.DragDrop(inputs),
    "duplicates" => Commands.Duplicates(inputs),
    "file-log-backup-name" => Commands.FileLogBackupNameCases(inputs),
    "file-log-bytes" => Commands.FileLogBytesCases(inputs),
    "file-log-format" => Commands.FileLogFormatCases(inputs),
    "forwarded-path" => Commands.ForwardedPathCases(inputs),
    "image-sort-entries" => Commands.ImageSortEntries(inputs),
    "image-sort-paths" => Commands.ImageSortPaths(inputs),
    "l10n-catalog" => Commands.L10nCatalogCases(inputs),
    "l10n-flat-json" => Commands.L10nFlatJsonCases(inputs),
    "l10n-generator" => Commands.L10nGeneratorCases(inputs),
    "l10n-loader" => Commands.L10nLoaderCases(inputs),
    "l10n-localizer" => Commands.L10nLocalizerCases(inputs),
    "navigation-stat-worker" => Commands.NavigationStatWorkerCases(inputs),
    "numbered-pattern" => Commands.NumberedPattern(inputs),
    "perf-csv-escape" => Commands.PerfCsvEscapeCases(inputs),
    "perf-csv-file" => Commands.PerfCsvFileCases(inputs),
    "perf-csv-row" => Commands.PerfCsvRowCases(inputs),
    "perf-path-id" => Commands.PerfPathIdCases(inputs),
    "physical-fs-ops" => Commands.PhysFsOpsCases(inputs),
    "physical-fs-pattern" => Commands.PhysFsPatternCases(inputs),
    "physical-fs-read-text" => Commands.PhysFsReadTextCases(inputs),
    "physical-fs-resolve" => Commands.PhysFsResolveCases(inputs),
    "review-catalog" => Commands.ReviewCatalogTrace(inputs),
    "review-metrics" => Commands.ReviewMetricsCases(inputs),
    "session-json-read" => Commands.SessionJsonReadCases(inputs),
    "session-json-write" => Commands.SessionJsonWriteCases(inputs),
    "session-key" => Commands.SessionKeyCases(inputs),
    "session-writer-sequence" => Commands.SessionWriterSequenceCases(inputs),
    "settings-json" => Commands.SettingsJson(inputs),
    "settings-normalize" => Commands.SettingsNormalize(inputs),
    "settings-parse-text" => Commands.SettingsParseText(inputs),
    "settings-repair-text" => Commands.SettingsRepairText(inputs),
    "settings-store" => Commands.SettingsStoreScenarios(inputs),
    "settings-validate" => Commands.SettingsValidate(inputs),
    "sibling-folders" => Commands.SiblingFolders(inputs),
    "snapshot-validator" => Commands.SnapshotValidator(inputs),
    "update-checker" => Commands.UpdateCheckerCases(inputs),
    // Wave 3 commands (W3-01..W3-08)
    "file-action-scenarios" => Commands.FileActionScenarios(inputs),
    "journal-committed-moves" => Commands.JournalCommittedMoves(inputs),
    "journal-compact" => Commands.JournalCompact(inputs),
    "journal-compaction-plan" => Commands.JournalCompactionPlanCases(inputs),
    "journal-ops" => Commands.JournalOps(inputs),
    "journal-reconcile" => Commands.JournalReconcile(inputs),
    "journal-startup-recovery" => Commands.JournalStartupRecoveryCases(inputs),
    "journal-stream-lines" => Commands.JournalStreamLines(inputs),
    "journal-tail-committed-move" => Commands.JournalTailCommittedMoveCases(inputs),
    "journal-transaction" => Commands.JournalTransactionCases(inputs),
    "partial-cleanup" => Commands.PartialCleanup(inputs),
    "recovery-check" => Commands.RecoveryCheck(inputs),
    "recovery-decide" => Commands.RecoveryDecide(inputs),
    "recovery-retry" => Commands.RecoveryRetry(inputs),
    "undo-scenarios" => Commands.UndoScenarios(inputs),
    // Wave 4 commands (W4-01a..W4-16b)
    // W4-01a
    "image-cache-key" => Commands.ImageCacheKeyCases(inputs),
    // W4-01b
    "decode-box" => Commands.DecodeBoxCases(inputs),
    "decode-types" => Commands.DecodeTypesCases(inputs),
    // W4-01c
    "decoder-exceptions" => Commands.DecoderExceptions(inputs),
    // W4-02a
    "orientation-values" => Commands.OrientationValues(inputs),
    "orientation-read-wic" => Commands.OrientationReadWic(inputs),
    "orientation-pixels" => Commands.OrientationPixels(inputs),
    // W4-02b
    "embedded-thumbnail-size" => Commands.EmbeddedThumbnailSize(inputs),
    "embedded-thumbnail-fixtures" => Commands.EmbeddedThumbnailFixtures(inputs, inputPath),
    "embedded-thumbnail-read" => Commands.EmbeddedThumbnailRead(inputs, inputPath),
    // W4-03a
    "tiff-structure" => Commands.TiffStructureCases(inputs),
    // W4-03b
    "exif-parser" => Commands.ExifParserCases(inputs),
    // W4-04a
    "exif-summary-create" => Commands.ExifSummaryCreate(inputs),
    "exif-summary-text" => Commands.ExifSummaryText(inputs),
    "exif-summary-date" => Commands.ExifSummaryDate(inputs),
    "exif-summary-encode" => Commands.ExifSummaryEncode(inputs),
    "exif-summary-decode" => Commands.ExifSummaryDecode(inputs),
    // W4-04b
    "exif-query" => Commands.ExifQuery(inputs),
    // W4-05a
    "adaptive-preview-box" => Commands.AdaptivePreviewBox(inputs),
    "adaptive-preview-quantize" => Commands.AdaptivePreviewQuantize(inputs),
    // W4-05b
    "preload-order" => Commands.PreloadOrder(inputs),
    "preload-window" => Commands.PreloadWindowCases(inputs),
    "preload-options" => Commands.PreloadOptionsDefaults(inputs),
    // W4-06a
    "navigation-pace" => Commands.NavigationPaceScript(inputs),
    // W4-06b
    "preload-backoff-script" => Commands.PreloadBackoffScript(inputs),
    "preview-size-sampler-script" => Commands.PreviewSizeSamplerScript(inputs),
    // W4-07a
    "ram-budget-scalar" => Commands.RamBudgetScalar(inputs),
    "ram-budget-estimate" => Commands.RamBudgetEstimate(inputs),
    // W4-07b
    "memory-headroom" => Commands.MemoryHeadroomCases(inputs),
    // W4-08a
    "atomic-cache-file" => Commands.AtomicCacheFileCases(inputs),
    // W4-08b
    "preview-entry-read" => Commands.PreviewEntryRead(inputs, inputPath),
    "preview-entry-write" => Commands.PreviewEntryWrite(inputs),
    // W4-09
    "disk-cache-prune" => Commands.DiskCachePrune(inputs),
    "disk-cache-passes" => Commands.DiskCachePasses(inputs),
    "disk-cache-note-accessed" => Commands.DiskCacheNoteAccessed(inputs),
    "disk-cache-stale-temp" => Commands.DiskCacheStaleTemp(inputs),
    "disk-cache-delete" => Commands.DiskCacheDelete(inputs),
    "cache-entry-failure" => Commands.CacheEntryFailure(inputs),
    // W4-10a
    "source-bytes-scenarios" => Commands.SourceBytesScenarios(inputs),
    // W4-10b
    "thumbnail-cache-key" => Commands.ThumbnailCacheKey(inputs),
    "thumbnail-disk-decode" => Commands.ThumbnailDiskDecode(inputs),
    // W4-11a
    "wic-interop-constants" => Commands.WicInteropConstants(inputs),
    "wic-hresult-exception" => Commands.WicHresultException(inputs),
    "wic-managed-istream" => Commands.WicManagedIStream(inputs),
    // W4-11b
    "wic-codec-probe" => Commands.WicCodecProbeCases(inputs),
    // W4-11c
    "wic-direct-internals" => Commands.WicDirectInternals(inputs),
    "wic-direct-fixtures" => Commands.WicDirectFixtures(inputs, inputPath),
    "wic-direct-decode" => Commands.WicDirectDecode(inputs, inputPath),
    "wic-direct-read-info" => Commands.WicDirectReadInfo(inputs, inputPath),
    // W4-11d
    "wic-exif-values" => Commands.WicExifValues(inputs),
    "wic-exif-variant" => Commands.WicExifVariant(inputs),
    // W4-11e
    "wic-bgrx-to-bgr" => Commands.WicBgrxToBgrCases(inputs),
    "wic-cache-classify" => Commands.WicCacheClassifyCases(inputs),
    "wic-scale-resize" => Commands.WicScaleResizeCases(inputs),
    "wic-cache-codec" => Commands.WicCacheCodecCases(inputs),
    // W4-12a
    "decoder-factory" => Commands.DecoderFactory(inputs),
    // W4-12b
    "fallback-is-fallbackable" => Commands.FallbackIsFallbackable(inputs),
    "fallback-decoder" => Commands.FallbackDecoderCases(inputs),
    // W4-12c
    "webp-heic-routing" => Commands.WebpHeicRouting(inputs),
    // W4-14a
    "preview-budget-line" => Commands.PreviewBudgetLine(inputs),
    "preview-current-key" => Commands.PreviewCurrentKey(inputs),
    // W4-14b
    "preview-service-sequence" => Commands.PreviewServiceSequence(inputs),
    // W4-14c
    "preview-disk-cache-path" => Commands.PreviewDiskCachePath(inputs),
    // W4-15a
    "preload-policy" => Commands.PreloadPolicy(inputs),
    // W4-15b
    "preload-scheduler-script" => Commands.PreloadSchedulerScript(inputs),
    // W4-16a
    "pixel-buffer" => Commands.PixelBufferCases(inputs),
    "pixel-ops" => Commands.PixelOpsCases(inputs),
    // W4-16b
    "native-memory" => Commands.NativeMemory(inputs),
    "readonly-memory-stream" => Commands.ReadOnlyMemoryStream(inputs),
    "pixel-codec" => Commands.PixelCodec(inputs),
    // Commands\W5-01a
    "raw-formats" => Commands.RawFormats(inputs),
    // Commands\W5-01b
    "raw-contract-records" => Commands.RawContractRecords(inputs),
    "raw-header-budget" => Commands.RawHeaderBudget(inputs),
    // Commands\W5-02a
    "tiff-navigator" => Commands.TiffNavigatorCases(inputs),
    // Commands\W5-02b
    "jpeg-marker-probe" => Commands.JpegMarkerProbeCases(inputs),
    // Commands\W5-03
    "cr2-container" => Commands.Cr2Container(inputs),
    "cr2-container-corpus" => Commands.Cr2ContainerCorpus(inputs),
    // Commands\W5-04
    "nef-container" => Commands.NefContainer(inputs),
    // Commands\W5-05
    "orf-container" => Commands.OrfContainerCases(inputs),
    // Commands\W5-06
    "raw-dng-synthetic" => Commands.RawDngSynthetic(inputs),
    "raw-dng-corpus" => Commands.RawDngCorpus(inputs),
    // Commands\W5-07
    "raw-arw-reader" => Commands.RawArwReader(inputs, inputPath),
    // Commands\W5-08
    "rw2-container" => Commands.Rw2Container(inputs),
    "rw2-corpus" => Commands.Rw2Corpus(inputs),
    // Commands\W5-09
    "raf-container" => Commands.RafContainerCases(inputs),
    "raf-can-read" => Commands.RafCanReadCases(inputs),
    "raf-corpus" => Commands.RafCorpusCases(inputs),
    // Commands\W5-10a
    "bmff-navigator" => Commands.BmffNavigatorCases(inputs),
    // Commands\W5-10b
    "raw-cr3-reader" => Commands.RawCr3Reader(inputs, inputPath),
    // Commands\W5-11a
    "raw-registry-find" => Commands.RawRegistryFind(inputs),
    "raw-icc-ensure" => Commands.RawIccEnsure(inputs),
    // Commands\W5-11b
    "preview-selector-select" => Commands.PreviewSelectorSelectCases(inputs),
    "preview-selector-jpeg-frame" => Commands.PreviewSelectorJpegFrameCases(inputs),
    "adobe-rgb-exif" => Commands.AdobeRgbExifCases(inputs),
    // Commands\W5-11c
    "raw-exif" => Commands.RawExifCases(inputs),
    "raw-exif-corpus" => Commands.RawExifCorpus(inputs),
    // Commands\W5-12a
    "raw-header-source-io" => Commands.RawHeaderSourceIo(inputs),
    // Commands\W5-12b
    "raw-decoder-core" => Commands.RawDecoderCore(inputs),
    "raw-decoder-fallback" => Commands.RawDecoderFallback(inputs),
    // Commands\W5-12c
    "raw-routing" => Commands.RawRouting(inputs),
    // Commands\W5-13a
    "turbojpeg-native-types" => Commands.TurboJpegNativeTypes(inputs),
    "turbojpeg-native-handle" => Commands.TurboJpegNativeHandle(inputs),
    "turbojpeg-native-decode" => Commands.TurboJpegNativeDecode(inputs, inputPath),
    // Commands\W5-14a
    "pixel-area-resample" => Commands.PixelAreaResample(inputs),
    // Commands\W5-14b
    "turbojpeg-fixtures" => Commands.TurboJpegFixtures(inputs, inputPath),
    "turbojpeg-pure" => Commands.TurboJpegPure(inputs),
    "turbojpeg-decode" => Commands.TurboJpegDecode(inputs, inputPath),
    "turbojpeg-read-info" => Commands.TurboJpegReadInfo(inputs, inputPath),
    // Commands\W5-15b
    "libraw-version" => Commands.LibRawVersionCases(inputs),
    // Commands\W5-16a
    "full-decode-gate" => Commands.FullDecodeGateScenarios(inputs),
    // Commands\W5-16b
    "libraw-memory-guard" => Commands.LibRawMemoryGuard(inputs),
    // Commands\W5-16c
    "libraw-decoder" => Commands.LibRawDecoderCases(inputs),
    "libraw-decoder-native" => Commands.LibRawDecoderNativeCases(inputs),
    // Commands\W5-17
    "rgb-bgra-resample" => Commands.RgbBgraResample(inputs),
    "rgb-bgra-validate" => Commands.RgbBgraValidate(inputs),
    // Commands\W7-01a
    "bench-profile" => Commands.BenchProfile(inputs),
    "bench-phase" => Commands.BenchPhase(inputs),
    "bench-rank" => Commands.BenchRank(inputs),
    "bench-json" => Commands.BenchJson(inputs),
    // Commands\W7-01b
    "benchmark-profiles" => Commands.BenchmarkProfilesCases(inputs),
    "benchmark-metrics-since" => Commands.BenchmarkMetricsSinceCases(inputs),
    // Commands\W7-02a
    "slow-link-fs" => Commands.SlowLinkFs(inputs),
    // Commands\W7-02b
    "slow-link-source-reader" => Commands.SlowLinkSourceReaderCases(inputs),
    "bench-recycle-bin" => Commands.BenchRecycleBinCases(inputs),
    // Commands\W7-03a
    "bench-engine-run" => Commands.BenchEngineRun(inputs),
    "bench-engine-format-ms" => Commands.BenchEngineFormatMs(inputs),
    // Commands\W7-03b
    "bench-profile-scope" => Commands.BenchProfileScope(inputs),
    "bench-windowed-target" => Commands.BenchWindowedTarget(inputs),
    // Commands\W7-03c
    "bench-random" => Commands.BenchRandom(inputs),
    "bench-select-indices" => Commands.BenchSelectIndices(inputs),
    "bench-images-per-sample" => Commands.BenchImagesPerSample(inputs),
    "bench-prepare-edge" => Commands.BenchPrepareEdge(inputs),
    // Commands\W7-03d (cli-process is not dispatched: it needs the WPF-bound baseline Cli exe, see contracts/README.md Wave 7)
    "cli-arguments" => Commands.CliArguments(inputs),
    "cli-tool-path-guard" => Commands.CliToolPathGuard(inputs),
    "cli-dataset-plan" => Commands.CliDatasetPlan(inputs),
    // Wave 8 batch B1 (App.Shared / Shell.Rendering / Shell.Win32 pure logic)
    // Commands\W8-01a
    "viewport-state" => Commands.W801aViewportStateCases(inputs),
    "arrow-pan" => Commands.W801aArrowPanCases(inputs),
    "viewport-snapshot-text" => Commands.W801aSnapshotTextCases(inputs),
    "viewport-values" => Commands.W801aViewportValuesCases(inputs),
    "render-geometry" => Commands.RenderGeometry(inputs),
    // Commands\W8-01b
    "viewport-layout-engine" => Commands.ViewportLayoutEngineCases(inputs),
    // Commands\W8-02a
    "menu-model" => Commands.MenuModelCases(inputs),
    // Commands\W8-02b
    "format-exif" => Commands.FormatExif(inputs),
    "format-status" => Commands.FormatStatus(inputs),
    "format-title-bar" => Commands.FormatTitleBar(inputs),
    "settings-reload-path" => Commands.SettingsReloadPathCases(inputs),
    // Commands\W8-03a
    "window-placement-rules" => Commands.WindowPlacementRulesCases(inputs),
    "window-placement-visibility" => Commands.WindowPlacementVisibilityCases(inputs),
    "window-placement-json" => Commands.WindowPlacementJsonCases(inputs),
    // Commands\W8-04a
    "language-options" => Commands.W804aLanguageOptionsCases(inputs),
    "translation-problems" => Commands.W804aTranslationProblemsCases(inputs),
    "localization-service" => Commands.W804aLocalizationServiceCases(inputs),
    // Commands\W8-04b
    "translation-export" => Commands.TranslationExportCases(inputs),
    // Commands\W8-05b
    "key-id" => Commands.KeyIdCases(inputs),
    // Commands\W8-05c
    "key-id-mapping" => Commands.KeyIdMappingCases(inputs),
    // Commands\W8-05d
    "shortcut-router" => Commands.W805dShortcutRouterCases(inputs),
    // Commands\W8-05e
    "mouse-gestures" => Commands.W805eMouseGestures(inputs),
    "touchpad-gestures" => Commands.W805eTouchpadGestures(inputs),
    // Commands\W8-05f
    "kinetic-velocity" => Commands.W805fVelocityCases(inputs),
    "kinetic-scroller" => Commands.W805fScrollerCases(inputs),
    "keyboard-pan" => Commands.W805fKeyboardCases(inputs),
    "glide-clock" => Commands.W805fGlideCases(inputs),
    // Commands\W8-06a
    "viewer-state-trace" => Commands.W806aViewerStateTraceCases(inputs),
    "viewer-step-zoom" => Commands.W806aStepZoomCases(inputs),
    "viewer-display-size" => Commands.W806aDisplaySizeCases(inputs),
    // Commands\W8-06b
    "info-overlay-autohide" => Commands.W806bInfoOverlayAutoHideCases(inputs),
    "toolbar-autohide" => Commands.W806bToolbarAutoHideCases(inputs),
    "fade-target-gate" => Commands.W806bFadeTargetGateCases(inputs),
    "image-transition" => Commands.W806bImageTransitionCases(inputs),
    "info-overlay-view-model" => Commands.W806bInfoOverlayViewModelCases(inputs),
    // Commands\W8-10a
    "action-list-json" => Commands.ActionListJsonCases(inputs),
    "action-profiles" => Commands.ActionProfilesCases(inputs),
    "action-profiles-io" => Commands.ActionProfilesIoCases(inputs),
    // Commands\W8-10b
    "settings-field-map" => Commands.SettingsFieldMapCases(inputs),
    // Commands\W8-10f
    "recovery-presentation" => Commands.RecoveryPresentationCases(inputs),
    // Commands\W8-10h
    "benchmark-presentation" => Commands.W810hBenchmarkPresentationCases(inputs),
    // Commands\W8-11a
    "win32-constants" => Commands.W811aWin32ConstantsCases(inputs),
    // Commands\W8-15b
    "overlay-animator" => Commands.W815bAnimatorCases(inputs),
    "overlay-frame-delay" => Commands.W815bFrameDelayCases(inputs),
    "overlay-palette" => Commands.W815bPaletteCases(inputs),
    // Commands\W8-16a
    "scaling-policy" => Commands.W816aScalingPolicyCases(inputs),
    "frame-stats" => Commands.W816aFrameStatsCases(inputs),
    "frame-pacer" => Commands.W816aFramePacerCases(inputs),
    "frame-log" => Commands.W816aFrameLogCases(inputs),
    // Commands\W8-16e
    "scrollbar-geometry" => Commands.W816eScrollBarGeometryCases(inputs),
    "scrollbar-repeat-timing" => Commands.W816eRepeatTimingCases(inputs),
    // Commands\Commands\W8-18b
    "folder-picker-policy" => Commands.W818bFolderPickerPolicyCases(inputs),
    // Commands\Commands\W8-18d
    "dialog-template" => Commands.W818dDialogTemplateCases(inputs),
    // Commands\Commands\W8-18i
    "dialog-layout" => Commands.W818iDialogLayoutCases(inputs),
    // Commands\Commands\W8-18l
    "painter-kinds" => Commands.W818lPainterKindsCases(inputs),
    // Commands\Commands\W8-18o
    "secondary-window-supports" => Commands.W818oSupportsCases(inputs),
    // Commands\Commands\W8-18p
    "dialog-zoom-diagnostics" => Commands.W818pZoomDiagnosticsCases(inputs),
    // Commands\Commands\W8-18q
    "dialog-action-profiles" => Commands.W818qActionProfilesCases(inputs),
    // Commands\Commands\W8-18r
    "dialog-benchmark-layout" => Commands.W818rBenchmarkLayoutCases(inputs),
    // Commands\Commands\W8-18s
    "dialog-benchmark-results" => Commands.W818sBenchmarkResultsCases(inputs),
    // Commands\Commands\W8-18t
    "recovery-dialog-layout" => Commands.W818tRecoveryDialogLayoutCases(inputs),
    // Commands\Commands\W8-18u
    "recovery-dialog-details" => Commands.W818uRecoveryDialogDetailsCases(inputs),
    // Commands\Commands\W8-18v
    "recovery-dialog-dismiss" => Commands.W818vRecoveryDialogDismissCases(inputs),
    // Commands\Commands\W8-19a
    "settings-catalog" => Commands.W819aSettingsCatalogCases(inputs),
    // Commands\Commands\W8-19b
    "settings-layout" => Commands.W819bSettingsLayoutCases(inputs),
    // Commands\Commands\W8-19c
    "settings-builder" => Commands.W819cSettingsBuilderCases(inputs),
    // Commands\Commands\W8-19d
    "settings-pages-diag-perf" => Commands.W819dSettingsPagesCases(inputs),
    // Commands\Commands\W8-19e
    "settings-page-display" => Commands.W819eSettingsPageDisplayCases(inputs),
    // Commands\Commands\W8-19f
    "settings-page-files" => Commands.W819fSettingsPageFilesCases(inputs),
    // Commands\Commands\W8-19g
    "settings-page-general" => Commands.W819gSettingsPageGeneralCases(inputs),
    // Commands\Commands\W8-19h
    "settings-page-mouse" => Commands.W819hSettingsPageMouseCases(inputs),
    // Commands\Commands\W8-19i
    "settings-page-shortcuts" => Commands.W819iSettingsPageShortcutsCases(inputs),
    // Commands\Commands\W8-19j
    "settings-dialog-frame" => Commands.W819jSettingsDialogFrameCases(inputs),
    // Commands\Commands\W8-19k
    "settings-dialog-state" => Commands.W819kSettingsDialogStateCases(inputs),
    // Commands\Commands\W8-19l
    "settings-dialog-apply" => Commands.W819lSettingsDialogApplyCases(inputs),
    // Commands\Commands\W8-19m
    "settings-dialog-layout" => Commands.W819mSettingsDialogLayoutCases(inputs),
    _ => throw new ArgumentException($"unknown command '{command}'"),
};

var doc = new JsonObject
{
    ["generator"] = $"oracle-cs {command}",
    ["csharp_commit"] = commit,
    ["cases"] = cases,
};
Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(outputPath))!);
var options = new JsonSerializerOptions { WriteIndented = true, Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };
File.WriteAllText(outputPath, doc.ToJsonString(options) + "\n");
Console.WriteLine($"{command}: {cases.Count} case(s) -> {outputPath}");
return 0;

static partial class Commands
{
    private static readonly JsonSerializerOptions Plain = new();

    /// <summary>
    /// How System.Text.Json writes and reads a UTC DateTime (the on-disk form of every journal/session timestamp).
    /// Input: "ticks" (UTC DateTime ticks to write) and "parse" (texts to read back).
    /// </summary>
    public static JsonArray DateTimeJson(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["ticks"]!.AsArray())
        {
            var ticks = node!.GetValue<long>();
            var value = new DateTime(ticks, DateTimeKind.Utc);
            result.Add(new JsonObject
            {
                ["kind"] = "write",
                ["ticks"] = ticks,
                ["json"] = JsonSerializer.Deserialize<string>(JsonSerializer.Serialize(value, Plain)),
            });
        }
        foreach (var node in inputs["parse"]!.AsArray())
        {
            var text = node!.GetValue<string>();
            var item = new JsonObject { ["kind"] = "parse", ["json"] = text };
            try
            {
                var parsed = JsonSerializer.Deserialize<DateTime>(JsonSerializer.Serialize(text, Plain), Plain);
                // ToUniversalTime of a Local value depends on this machine's zone; inputs only use Z, offsets or no zone.
                var utc = parsed.Kind == DateTimeKind.Local ? parsed.ToUniversalTime() : parsed;
                item["ok"] = true;
                item["ticks"] = utc.Ticks;
                item["source_kind"] = parsed.Kind.ToString();
            }
            catch (JsonException)
            {
                item["ok"] = false;
            }
            result.Add(item);
        }
        return result;
    }

    /// <summary>string.Equals / string.Compare with StringComparison.OrdinalIgnoreCase (paths, file names).</summary>
    public static JsonArray OrdinalIgnoreCase(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var pair in inputs["pairs"]!.AsArray())
        {
            var a = pair![0]!.GetValue<string>();
            var b = pair[1]!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["a"] = a,
                ["b"] = b,
                ["equals"] = string.Equals(a, b, StringComparison.OrdinalIgnoreCase),
                ["compare"] = Math.Sign(string.Compare(a, b, StringComparison.OrdinalIgnoreCase)),
            });
        }
        return result;
    }

    /// <summary>
    /// Every code point (surrogates excluded) that OrdinalIgnoreCase treats as equal to a different single code point,
    /// mapped to that representative. The representative is the first of ToUpperInvariant(c), ToUpperInvariant(
    /// ToLowerInvariant(c)) that .NET itself reports equal, so e.g. KELVIN SIGN maps to 'K' while DOTLESS I stays itself.
    /// </summary>
    public static int WriteOrdinalCasingTable(string rustPath, string commit)
    {
        var pairs = new List<(int From, int To)>();
        for (var cp = 0x80; cp <= 0x10FFFF; cp++)
        {
            if (cp is >= 0xD800 and <= 0xDFFF) continue;
            var s = char.ConvertFromUtf32(cp);
            foreach (var candidate in new[] { s.ToUpperInvariant(), s.ToLowerInvariant().ToUpperInvariant() })
            {
                if (candidate == s || !IsSingleCodePoint(candidate)) continue;
                if (!string.Equals(s, candidate, StringComparison.OrdinalIgnoreCase)) continue;
                pairs.Add((cp, char.ConvertToUtf32(candidate, 0)));
                break;
            }
        }
        var sb = new System.Text.StringBuilder();
        sb.Append("// @generated by tools/oracle.ps1 -Command ordinal-casing-table from C# PhotoReview ")
          .Append(commit).Append(" (.NET ").Append(Environment.Version).Append("). Do not edit.\n");
        sb.Append("// Non-ASCII code points that StringComparison.OrdinalIgnoreCase maps to another code point, sorted.\n\n");
        sb.Append("pub(super) static ORDINAL_UPPER: &[(u32, u32)] = &[\n");
        foreach (var (from, to) in pairs) sb.Append("    (0x").Append(from.ToString("X4")).Append(", 0x").Append(to.ToString("X4")).Append("),\n");
        sb.Append("];\n");
        Directory.CreateDirectory(Path.GetDirectoryName(rustPath)!);
        File.WriteAllText(rustPath, sb.ToString());
        return pairs.Count;
    }

    private static bool IsSingleCodePoint(string s) =>
        s.Length == 1 ? !char.IsSurrogate(s[0]) : s.Length == 2 && char.IsSurrogatePair(s[0], s[1]);

    /// <summary>ManagedNaturalComparer: pairwise sign and the sorted order of each name list.</summary>
    public static JsonArray NaturalCompare(JsonObject inputs)
    {
        var comparer = ManagedNaturalComparer.Instance;
        var result = new JsonArray();
        foreach (var pair in inputs["pairs"]!.AsArray())
        {
            var a = pair![0]?.GetValue<string>();
            var b = pair[1]?.GetValue<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "pair",
                ["a"] = a,
                ["b"] = b,
                ["compare"] = Math.Sign(comparer.Compare(a, b)),
            });
        }
        foreach (var list in inputs["lists"]!.AsArray())
        {
            var names = list!.AsArray().Select(n => n!.GetValue<string>()).ToList();
            var sorted = names.ToList();
            sorted.Sort(comparer);
            result.Add(new JsonObject
            {
                ["kind"] = "sort",
                ["input"] = new JsonArray(names.Select(n => (JsonNode?)n).ToArray()),
                ["sorted"] = new JsonArray(sorted.Select(n => (JsonNode?)n).ToArray()),
            });
        }
        return result;
    }
}
