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
