using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.App.ViewModels;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.IO;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Imaging.Metadata;

// W8-02b golden generator (contract: contracts/W8-02b.md, "Golden package").
// NOT RUN by the writer; the lead wires and runs it.
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-02b (ViewModels formatters, App.Shared)
//     "format-exif" => Commands.FormatExif(inputs),
//     "format-status" => Commands.FormatStatus(inputs),
//     "format-title-bar" => Commands.FormatTitleBar(inputs),
//     "settings-reload-path" => Commands.SettingsReloadPathCases(inputs),
// Program.cs also sets `Commands.FormatInputsDir = Path.GetDirectoryName(Path.GetFullPath(inputPath))!;` next to the
// PerfCommands.FixturesDir line: "languages_dir" is resolved against it (../../assets/languages, the synced catalog).
//
// Link lines for tools/oracle-cs/OracleCs.csproj (CSharpSrcDir is set by tools/oracle.ps1, as for W7-03d). The four C#
// files are compiled into the oracle as they are, so the `internal` members (CameraText, ShutterText, FormatDateTime,
// SettingsReloadPath.Choose) are called directly and nothing is copied by hand:
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\ViewModels\ExifFormatter.cs" Link="cs-baseline\ExifFormatter.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\ViewModels\StatusFormatter.cs" Link="cs-baseline\StatusFormatter.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\ViewModels\TitleBarFormatter.cs" Link="cs-baseline\TitleBarFormatter.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\ViewModels\SettingsReloadPath.cs" Link="cs-baseline\SettingsReloadPath.cs" />
// Needs only PhotoReview.Core and PhotoReview.Imaging (already referenced). The `Tr` accessors are generated into Core.

// Golden semantics (deterministic, machine-independent):
//  * Localizer: "en" = BuiltInCatalog.EnglishLocalizer; "vi" = LanguageLoader over assets/languages (vi.json, English fallback).
//    Set with Localizer.SetCurrent for the case and restored after it (the generated Tr API reads the ambient one).
//  * Culture: "invariant" or a named culture; set as CultureInfo.CurrentCulture for the case (FormatFileSize and the N0 text
//    read CurrentCulture) and passed as the provider. The number facts of the culture are recorded in every case.
//  * Local time: a case gives its modified time as a wall clock; the oracle builds it with DateTimeKind.Local, so the
//    C# `ToLocalTime()` inside the formatter is the identity and nothing depends on the machine time zone.
//  * Date text: each date the formatter can show is recorded as {ticks, general (or null when "g" throws
//    ArgumentOutOfRangeException), invariant}; the Rust golden test replays these texts (contract Semantics 5).
//  * A throw other than the ones above is recorded as {"throws": "<type>"} and makes the case fail on the Rust side.

static partial class Commands
{
    /// <summary>Folder of the case files (set by Program.cs); "languages_dir" in an input file is relative to it.</summary>
    public static string FormatInputsDir = "";

    private static CultureInfo FormatCulture(string name) =>
        name == "invariant" ? CultureInfo.InvariantCulture : CultureInfo.GetCultureInfo(name);

    private static Localizer FormatLocalizer(string language, Dictionary<string, Localizer> cache)
    {
        if (cache.TryGetValue(language, out var known)) return known;
        var localizer = language switch
        {
            "en" => BuiltInCatalog.EnglishLocalizer,
            // The languages folder is the synced copy in the Rust repo (same bytes as the baseline Languages folder).
            "vi" => new LanguageLoader(new PhysicalFileSystem(), Path.GetFullPath(Path.Combine(FormatInputsDir, "..", "..", "assets", "languages")), userDir: null).Load("vi"),
            _ => throw new ArgumentException("unknown language " + language),
        };
        cache[language] = localizer;
        return localizer;
    }

    private static JsonObject NumberFacts(CultureInfo culture) => new()
    {
        ["negative_sign"] = culture.NumberFormat.NegativeSign,
        ["positive_sign"] = culture.NumberFormat.PositiveSign,
        ["decimal_separator"] = culture.NumberFormat.NumberDecimalSeparator,
        ["group_separator"] = culture.NumberFormat.NumberGroupSeparator,
        ["nan_symbol"] = culture.NumberFormat.NaNSymbol,
        ["positive_infinity_symbol"] = culture.NumberFormat.PositiveInfinitySymbol,
        ["negative_infinity_symbol"] = culture.NumberFormat.NegativeInfinitySymbol,
    };

    /// <summary>{ticks, general, invariant} for one wall-clock value: the two texts DateText answers in the Rust port.</summary>
    private static JsonObject DateTexts(DateTime date, CultureInfo culture)
    {
        string? general;
        try { general = date.ToString("g", culture); }
        catch (ArgumentOutOfRangeException) { general = null; }
        return new JsonObject
        {
            ["ticks"] = date.Ticks.ToString(CultureInfo.InvariantCulture),
            ["general"] = general,
            ["invariant"] = date.ToString("g", CultureInfo.InvariantCulture),
        };
    }

    private static DateTime? WallClock(JsonNode? node, DateTimeKind kind) =>
        node is null ? null : DateTime.SpecifyKind(DateTime.Parse(node.GetValue<string>(), CultureInfo.InvariantCulture, DateTimeStyles.None), kind);

    private static ExifRational? Rational(JsonNode? node) =>
        node is null ? null : new ExifRational((uint)node["n"]!.GetValue<long>(), (uint)node["d"]!.GetValue<long>());

    private static ExifSummary? Exif(JsonNode? node)
    {
        if (node is null) return null;
        var o = node.AsObject();
        return new ExifSummary
        {
            DateTaken = WallClock(o["date_taken"], DateTimeKind.Unspecified),
            CameraMake = o["camera_make"]?.GetValue<string>(),
            CameraModel = o["camera_model"]?.GetValue<string>(),
            LensModel = o["lens_model"]?.GetValue<string>(),
            Iso = o["iso"] is { } iso ? iso.GetValue<int>() : null,
            FocalLength = Rational(o["focal_length"]),
            FNumber = Rational(o["f_number"]),
            ExposureTime = Rational(o["exposure_time"]),
        };
    }

    /// <summary>
    /// ExifFormatter.Format (kind "format"), CameraText ("camera") and ShutterText ("shutter") for each case
    /// {name, kind, language, culture, fields?, file_name?, width?, height?, exif?, modified_local?, raw_preview_width?,
    /// raw_preview_height?, make?, model?, exposure? {n, d}}. Result: "text" (null when the C# method returns null) and the
    /// date texts and number facts the Rust side replays.
    /// </summary>
    public static JsonArray FormatExif(JsonObject inputs)
    {
        var result = new JsonArray();
        var localizers = new Dictionary<string, Localizer>(StringComparer.Ordinal);
        var previousLocalizer = Localizer.Current;
        var previousCulture = CultureInfo.CurrentCulture;
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var culture = FormatCulture(c["culture"]!.GetValue<string>());
                Localizer.SetCurrent(FormatLocalizer(c["language"]!.GetValue<string>(), localizers));
                CultureInfo.CurrentCulture = culture;
                var output = (JsonObject)c.DeepClone();
                output["localizer_code"] = Localizer.Current.Code;
                output["number_culture"] = NumberFacts(culture);
                var dates = new JsonArray();
                var exif = Exif(c["exif"]);
                var modified = WallClock(c["modified_local"], DateTimeKind.Local);
                if (exif?.DateTaken is { } taken) dates.Add(DateTexts(taken, culture));
                if (modified is { } local) dates.Add(DateTexts(local, culture));
                output["date_texts"] = dates;

                string? text = c["kind"]!.GetValue<string>() switch
                {
                    "format" => ExifFormatter.Format(
                        (ExifInfoFields)c["fields"]!.GetValue<int>(),
                        c["file_name"]?.GetValue<string>(),
                        c["width"]!.GetValue<int>(),
                        c["height"]!.GetValue<int>(),
                        exif,
                        modified,
                        culture,
                        c["raw_preview_width"]?.GetValue<int>() ?? 0,
                        c["raw_preview_height"]?.GetValue<int>() ?? 0),
                    "camera" => ExifFormatter.CameraText(c["make"]?.GetValue<string>(), c["model"]?.GetValue<string>()),
                    "shutter" => ExifFormatter.ShutterText(Rational(c["exposure"]), culture),
                    var other => throw new ArgumentException("unknown kind " + other),
                };
                output["text"] = text;
                result.Add(output);
            }
        }
        finally
        {
            Localizer.SetCurrent(previousLocalizer);
            CultureInfo.CurrentCulture = previousCulture;
        }
        return result;
    }

    /// <summary>
    /// StatusFormatter.* for each case {name, op, language, culture, args:{...}} (op and args as listed in the contract,
    /// Semantics 8 to 10). Result: "text" and the number facts and date texts (only file_size and compare_size_suffix use numbers).
    /// </summary>
    public static JsonArray FormatStatus(JsonObject inputs)
    {
        var result = new JsonArray();
        var localizers = new Dictionary<string, Localizer>(StringComparer.Ordinal);
        var previousLocalizer = Localizer.Current;
        var previousCulture = CultureInfo.CurrentCulture;
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var culture = FormatCulture(c["culture"]!.GetValue<string>());
                Localizer.SetCurrent(FormatLocalizer(c["language"]!.GetValue<string>(), localizers));
                CultureInfo.CurrentCulture = culture;
                var a = c["args"]!.AsObject();
                string text = c["op"]!.GetValue<string>() switch
                {
                    "file_size" => StatusFormatter.FormatFileSize(a["bytes"]!.GetValue<long>()),
                    "no_supported_images" => StatusFormatter.NoSupportedImages(),
                    "no_supported_images_but_subfolders" => StatusFormatter.NoSupportedImagesButSubfolders(a["count"]!.GetValue<int>()),
                    "folder_open_failed" => StatusFormatter.FolderOpenFailed(a["message"]!.GetValue<string>()),
                    "index_only" => StatusFormatter.IndexOnly(a["index"]!.GetValue<int>(), a["count"]!.GetValue<int>()),
                    "loading" => StatusFormatter.Loading(a["index"]!.GetValue<int>(), a["count"]!.GetValue<int>(), a["size"]!.GetValue<long>()),
                    "loading_full_res" => StatusFormatter.LoadingFullRes(a["index"]!.GetValue<int>(), a["count"]!.GetValue<int>(), a["size"]!.GetValue<long>()),
                    "decoding_raw" => StatusFormatter.DecodingRaw(),
                    "ready" => StatusFormatter.Ready(a["index"]!.GetValue<int>(), a["count"]!.GetValue<int>(), a["size"]!.GetValue<long>(), a["file_name"]!.GetValue<string>()),
                    "with_dimensions" => StatusFormatter.WithDimensions(a["index"]!.GetValue<int>(), a["count"]!.GetValue<int>(), a["size"]!.GetValue<long>(),
                        a["width"]!.GetValue<int>(), a["height"]!.GetValue<int>(), a["file_name"]!.GetValue<string>()),
                    "compare" => StatusFormatter.Compare(a["index"]!.GetValue<int>(), a["count"]!.GetValue<int>(), a["left_name"]!.GetValue<string>(),
                        a["left_size"]!.GetValue<string>(), a["right_name"]!.GetValue<string>(), a["right_size"]!.GetValue<string>(), a["hash_text"]!.GetValue<string>()),
                    "compare_size_suffix" => StatusFormatter.CompareSizeSuffix(a["bytes"]!.GetValue<long>()),
                    "compare_hash_text" => StatusFormatter.CompareHashText(a["same"] is { } same ? same.GetValue<bool>() : null),
                    "compare_hash_unknown" => StatusFormatter.CompareHashUnknown(),
                    "image_error" => StatusFormatter.ImageError(a["file_name"]!.GetValue<string>(), a["message"]!.GetValue<string>()),
                    "no_images_remaining" => StatusFormatter.NoImagesRemaining(),
                    "all_images_processed" => StatusFormatter.AllImagesProcessed(),
                    "cache_cleared" => StatusFormatter.CacheCleared(),
                    "duplicate_check_failed" => StatusFormatter.DuplicateCheckFailed(a["message"]!.GetValue<string>()),
                    "duplicate_check_canceled_folder_changed" => StatusFormatter.DuplicateCheckCanceledFolderChanged(),
                    "duplicate_check_running" => StatusFormatter.DuplicateCheckRunning(),
                    "duplicate_check_canceled" => StatusFormatter.DuplicateCheckCanceled(),
                    "no_duplicates_found" => StatusFormatter.NoDuplicatesFound(),
                    "batch_canceled" => StatusFormatter.BatchCanceled(),
                    "batch_done" => StatusFormatter.BatchDone(a["succeeded"]!.GetValue<int>(), a["failures"]!.GetValue<int>()),
                    "sibling_folder_boundary" => StatusFormatter.SiblingFolderBoundary(a["direction"]!.GetValue<int>()),
                    "action_completed" => StatusFormatter.ActionCompleted(a["action_name"]!.GetValue<string>()),
                    "action_invalid_operation" => StatusFormatter.ActionInvalidOperation(a["action_name"]!.GetValue<string>()),
                    "action_no_destination" => StatusFormatter.ActionNoDestination(a["action_name"]!.GetValue<string>()),
                    "action_failed" => StatusFormatter.ActionFailed(a["action_name"]!.GetValue<string>(), a["message"]?.GetValue<string>()),
                    "copied_to" => StatusFormatter.CopiedTo(a["file_name"]?.GetValue<string>()),
                    "nothing_to_undo" => StatusFormatter.NothingToUndo(),
                    var other => throw new ArgumentException("unknown op " + other),
                };
                var output = (JsonObject)c.DeepClone();
                output["localizer_code"] = Localizer.Current.Code;
                output["number_culture"] = NumberFacts(culture);
                output["text"] = text;
                result.Add(output);
            }
        }
        finally
        {
            Localizer.SetCurrent(previousLocalizer);
            CultureInfo.CurrentCulture = previousCulture;
        }
        return result;
    }

    /// <summary>
    /// TitleBarFormatter.Format for each case {name, fields, folder_path, index?, count?, file_name?, length?, width, height,
    /// modified_local?, exif?, language, culture}. Result: "text" plus the number facts and date texts.
    /// </summary>
    public static JsonArray FormatTitleBar(JsonObject inputs)
    {
        var result = new JsonArray();
        var localizers = new Dictionary<string, Localizer>(StringComparer.Ordinal);
        var previousLocalizer = Localizer.Current;
        var previousCulture = CultureInfo.CurrentCulture;
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var culture = FormatCulture(c["culture"]!.GetValue<string>());
                Localizer.SetCurrent(FormatLocalizer(c["language"]!.GetValue<string>(), localizers));
                CultureInfo.CurrentCulture = culture;
                var exif = Exif(c["exif"]);
                var modified = WallClock(c["modified_local"], DateTimeKind.Local);
                var dates = new JsonArray();
                if (exif?.DateTaken is { } taken) dates.Add(DateTexts(taken, culture));
                if (modified is { } local) dates.Add(DateTexts(local, culture));
                var text = TitleBarFormatter.Format(
                    (TitleBarFields)c["fields"]!.GetValue<int>(),
                    c["folder_path"]!.GetValue<string>(),
                    c["index"] is { } index ? index.GetValue<int>() : null,
                    c["count"] is { } count ? count.GetValue<int>() : null,
                    c["file_name"]?.GetValue<string>(),
                    c["length"] is { } length ? length.GetValue<long>() : null,
                    c["width"]!.GetValue<int>(),
                    c["height"]!.GetValue<int>(),
                    modified,
                    exif,
                    culture);
                var output = (JsonObject)c.DeepClone();
                output["localizer_code"] = Localizer.Current.Code;
                output["number_culture"] = NumberFacts(culture);
                output["date_texts"] = dates;
                output["text"] = text;
                result.Add(output);
            }
        }
        finally
        {
            Localizer.SetCurrent(previousLocalizer);
            CultureInfo.CurrentCulture = previousCulture;
        }
        return result;
    }

    /// <summary>
    /// SettingsReloadPath.Choose for each case {name, presented?, group?: {jpeg, raw}, entry?, raw_enabled, pair_mode}.
    /// Result: "result" (null when Choose returns null).
    /// </summary>
    public static JsonArray SettingsReloadPathCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var group = c["group"] is { } g
                ? new CaptureGroup(g["jpeg"]!.GetValue<string>(), g["raw"]!.GetValue<string>())
                : null;
            var mode = Enum.Parse<RawPairMode>(c["pair_mode"]!.GetValue<string>());
            var chosen = SettingsReloadPath.Choose(
                c["presented"]?.GetValue<string>(),
                group,
                c["entry"]?.GetValue<string>(),
                c["raw_enabled"]!.GetValue<bool>(),
                mode);
            var output = (JsonObject)c.DeepClone();
            output["result"] = chosen;
            result.Add(output);
        }
        return result;
    }
}
