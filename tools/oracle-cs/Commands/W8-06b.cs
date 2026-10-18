using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using PhotoReview.App.Coordinators;
using PhotoReview.App.ViewModels;
using PhotoReview.Core.Settings;

// Destination: tools/oracle-cs/Commands/W8-06b.cs (this draft is moved there by the lead).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-06b
//     "info-overlay-autohide" => Commands.W806bInfoOverlayAutoHideCases(inputs),
//     "toolbar-autohide" => Commands.W806bToolbarAutoHideCases(inputs),
//     "fade-target-gate" => Commands.W806bFadeTargetGateCases(inputs),
//     "image-transition" => Commands.W806bImageTransitionCases(inputs),
//     "info-overlay-view-model" => Commands.W806bInfoOverlayViewModelCases(inputs),
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: Reference to PhotoReview.App.Shared.dll (see W8-01a O6: App.Shared targets
//     net10.0-windows and needs CommunityToolkit.Mvvm next to oracle-cs.exe).
//   * tools/oracle.ps1: add 'PhotoReview.App.Shared.dll' to $requiredDlls (same step as W8-01a).
//
// Conventions (shared with W8-01a and Perf.cs):
//   * Inputs: a double is a JSON number, or one of the strings "NaN", "Infinity", "-Infinity", "-0".
//   * Outputs: the same strings for non-finite values and for negative zero, JSON numbers otherwise.
//   * Every case object is the input case with the expected fields added, so the Rust side reads only the golden file.
//   * Culture is pinned to Invariant in every command. The folder-info labels are the English catalog strings
//     (main.folderInfo.* in en.json): the English localizer is the default, no catalog is loaded here.
//   * No ambient state: nothing reads the clock, the cwd or the file system. The finder is a scripted fake and
//     throws only IOException / UnauthorizedAccessException, which C# maps to "no siblings" WITHOUT logging
//     (any other exception makes AppLog write a file, which a golden must never do).
//
// Scripted sibling search (view-model command): the case `finder` object maps an exact folder string to
//   {"previous": string|null, "next": string|null}  (Found) or {"fault": "IOException"|"UnauthorizedAccessException"}.
// A folder not in the map returns the default SiblingImageFolders (no siblings). Every op is awaited before the
// expectation is read (the search continuation runs on the thread pool), so the outcome is deterministic.

static partial class Commands
{
    private static double W806bNum(JsonNode? node)
    {
        if (node is JsonValue value && value.TryGetValue(out string? text))
        {
            return text switch
            {
                "NaN" => double.NaN,
                "Infinity" => double.PositiveInfinity,
                "-Infinity" => double.NegativeInfinity,
                "-0" => -0.0,
                _ => double.Parse(text!, NumberStyles.Float, CultureInfo.InvariantCulture),
            };
        }
        return node!.GetValue<double>();
    }

    private static JsonNode W806bOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    private static JsonNode W806bBool(bool value) => JsonValue.Create(value)!;

    private static JsonNode W806bInt(int value) => JsonValue.Create(value)!;

    private static JsonNode W806bStr(string? value) => value is null ? null! : JsonValue.Create(value)!;

    private static bool W806bFlag(JsonObject o, string key) => o[key]!.GetValue<bool>();

    private static int W806bIntIn(JsonObject o, string key) => o[key]!.GetValue<int>();

    // Each output case is a fresh copy of the input case (JsonNode cannot be re-parented).
    private static JsonObject W806bClone(JsonNode node) => JsonNode.Parse(node.ToJsonString())!.AsObject();

    // ---- info-overlay-autohide: InfoOverlayAutoHidePolicy ----
    // kinds: "must_stay" (enabled, folder, attention, compare, active -> must_stay_visible),
    //        "evaluate"  (the same + idle -> opacity, hit_test_visible, must_stay_visible),
    //        "delay"     (info_delay_ms, toolbar_delay_ms -> delay_ms).
    public static JsonArray W806bInfoOverlayAutoHideCases(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = W806bClone(node!);
            var kind = c["kind"]!.GetValue<string>();
            switch (kind)
            {
                case "must_stay":
                    c["must_stay_visible"] = W806bBool(InfoOverlayAutoHidePolicy.MustStayVisible(
                        W806bFlag(c, "enabled"), W806bFlag(c, "folder"), W806bFlag(c, "attention"),
                        W806bFlag(c, "compare"), W806bFlag(c, "active")));
                    break;
                case "evaluate":
                    var outcome = InfoOverlayAutoHidePolicy.Evaluate(
                        W806bFlag(c, "enabled"), W806bFlag(c, "folder"), W806bFlag(c, "attention"),
                        W806bFlag(c, "compare"), W806bFlag(c, "active"), W806bFlag(c, "idle"));
                    c["opacity"] = W806bOut(outcome.Opacity);
                    c["hit_test_visible"] = W806bBool(outcome.IsHitTestVisible);
                    c["must_stay_visible"] = W806bBool(InfoOverlayAutoHidePolicy.MustStayVisible(
                        W806bFlag(c, "enabled"), W806bFlag(c, "folder"), W806bFlag(c, "attention"),
                        W806bFlag(c, "compare"), W806bFlag(c, "active")));
                    break;
                case "delay":
                    var settings = new AppSettings
                    {
                        InfoOverlayAutoHideDelayMs = W806bIntIn(c, "info_delay_ms"),
                        ToolbarAutoHideDelayMs = W806bIntIn(c, "toolbar_delay_ms"),
                    };
                    c["delay_ms"] = W806bInt(InfoOverlayAutoHidePolicy.DelayMs(settings));
                    break;
                default:
                    throw new InvalidOperationException("unknown info-overlay-autohide kind: " + kind);
            }
            cases.Add(c);
        }
        return cases;
    }

    // ---- toolbar-autohide: ToolbarAutoHidePolicy ----
    // kinds: "must_stay" (enabled, folder, popup, focus -> must_stay_visible),
    //        "is_shown"  (the same + mouse -> shown),
    //        "delay"     (info_delay_ms, toolbar_delay_ms -> delay_ms),
    //        "target_opacity" (percent -> opacity),
    //        "hot_zone"  (mouse_x, mouse_y, left, top, width, height, margin -> inside).
    public static JsonArray W806bToolbarAutoHideCases(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = W806bClone(node!);
            var kind = c["kind"]!.GetValue<string>();
            switch (kind)
            {
                case "must_stay":
                    c["must_stay_visible"] = W806bBool(ToolbarAutoHidePolicy.MustStayVisible(
                        W806bFlag(c, "enabled"), W806bFlag(c, "folder"), W806bFlag(c, "popup"), W806bFlag(c, "focus")));
                    break;
                case "is_shown":
                    c["shown"] = W806bBool(ToolbarAutoHidePolicy.IsShown(
                        W806bFlag(c, "enabled"), W806bFlag(c, "folder"), W806bFlag(c, "popup"),
                        W806bFlag(c, "focus"), W806bFlag(c, "mouse")));
                    break;
                case "delay":
                    var settings = new AppSettings
                    {
                        InfoOverlayAutoHideDelayMs = W806bIntIn(c, "info_delay_ms"),
                        ToolbarAutoHideDelayMs = W806bIntIn(c, "toolbar_delay_ms"),
                    };
                    c["delay_ms"] = W806bInt(ToolbarAutoHidePolicy.DelayMs(settings));
                    break;
                case "target_opacity":
                    c["opacity"] = W806bOut(ToolbarAutoHidePolicy.TargetOpacity(W806bIntIn(c, "percent")));
                    break;
                case "hot_zone":
                    c["inside"] = W806bBool(ToolbarAutoHidePolicy.IsInsideHotZone(
                        W806bNum(c["mouse_x"]), W806bNum(c["mouse_y"]),
                        W806bNum(c["left"]), W806bNum(c["top"]), W806bNum(c["width"]), W806bNum(c["height"]),
                        W806bNum(c["margin"])));
                    break;
                default:
                    throw new InvalidOperationException("unknown toolbar-autohide kind: " + kind);
            }
            cases.Add(c);
        }
        return cases;
    }

    // ---- fade-target-gate: FadeTargetGate ----
    // targets: array of doubles (JSON number or "NaN", "-0", ...). Output "results": one bool per target, in order,
    // from ONE gate instance per case.
    public static JsonArray W806bFadeTargetGateCases(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = W806bClone(node!);
            var gate = new FadeTargetGate();
            var results = new JsonArray();
            foreach (var target in c["targets"]!.AsArray())
            {
                results.Add(W806bBool(gate.TryChange(W806bNum(target))));
            }
            c["results"] = results;
            cases.Add(c);
        }
        return cases;
    }

    // ---- image-transition: ImageTransitionDecision ----
    // previous / new: string or JSON null; compare: bool. Output "should": bool.
    public static JsonArray W806bImageTransitionCases(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = W806bClone(node!);
            var previous = c["previous"]?.GetValue<string>();
            var next = c["new"]?.GetValue<string>();
            c["should"] = W806bBool(ImageTransitionDecision.ShouldTransition(previous, next, W806bFlag(c, "compare")));
            cases.Add(c);
        }
        return cases;
    }

    // ---- info-overlay-view-model: InfoOverlayViewModel (scripted sibling search, awaited per op) ----
    // Case: {name, kind:"script", settings:{...patch...}, view:{has_image, zoom_percent}, finder:{...}, steps:[...]}.
    // Settings patch keys: show_info_overlay, show_file_info, show_folder_info, show_zoom_indicator (bool),
    //   info_overlay_font_size (number), previous_folder, next_folder (string). Unset keys keep the C# defaults.
    // Step ops: {op:"set_folder", folder: string|null}, {op:"refresh"}, {op:"set_settings", ...patch},
    //   {op:"set_view", has_image, zoom_percent}.
    // Each step gets "expect": {raised: [PropertyName...] in raise order (sync and search continuation together),
    //   searches: number of finder calls made by this step, state: {file_info_visible, folder_info_visible, font_size,
    //   exif_font_size, zoom_visible, zoom_text, folder_text}}.
    public static JsonArray W806bInfoOverlayViewModelCases(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = W806bClone(node!);
            var settings = new AppSettings();
            W806bApplySettings(settings, c["settings"]?.AsObject());
            var view = c["view"]?.AsObject();
            var hasImage = view is null || W806bFlag(view, "has_image");
            var zoomPercent = view is null ? 100 : W806bIntIn(view, "zoom_percent");

            var finderMap = c["finder"]?.AsObject();
            var calls = 0;
            Func<string, CancellationToken, SiblingImageFolders> finder = (folder, ct) =>
            {
                calls++;
                if (finderMap is null || !finderMap.TryGetPropertyValue(folder, out var spec) || spec is null)
                {
                    return default;
                }
                var so = spec.AsObject();
                if (so["fault"] is { } fault)
                {
                    throw fault.GetValue<string>() == "UnauthorizedAccessException"
                        ? new UnauthorizedAccessException("boom")
                        : new IOException("boom");
                }
                return new SiblingImageFolders((string?)so["previous"], (string?)so["next"]);
            };

            var vm = new InfoOverlayViewModel(() => settings, finder, () => hasImage, () => zoomPercent);
            var raised = new List<string>();
            vm.PropertyChanged += (_, e) => { if (e.PropertyName is not null) raised.Add(e.PropertyName); };
            var pendingProperty = typeof(InfoOverlayViewModel).GetProperty(
                "PendingSiblings", BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public)!;

            var stepsOut = new JsonArray();
            foreach (var stepNode in c["steps"]!.AsArray())
            {
                var step = W806bClone(stepNode!);
                var op = step["op"]!.GetValue<string>();
                var before = calls;
                raised.Clear();
                switch (op)
                {
                    case "set_folder":
                        var folderNode = step["folder"];
                        vm.SetFolder(folderNode is null ? null : folderNode.GetValue<string>());
                        break;
                    case "refresh":
                        vm.Refresh();
                        break;
                    case "set_settings":
                        W806bApplySettings(settings, step);
                        break;
                    case "set_view":
                        hasImage = W806bFlag(step, "has_image");
                        zoomPercent = W806bIntIn(step, "zoom_percent");
                        break;
                    default:
                        throw new InvalidOperationException("unknown view-model op: " + op);
                }
                // Await the search continuation (if any) so the expectation is deterministic.
                if (pendingProperty.GetValue(vm) is Task pending)
                {
                    pending.GetAwaiter().GetResult();
                }

                var state = new JsonObject
                {
                    ["file_info_visible"] = W806bBool(vm.IsFileInfoVisible),
                    ["folder_info_visible"] = W806bBool(vm.IsFolderInfoVisible),
                    ["font_size"] = W806bOut(vm.FontSize),
                    ["exif_font_size"] = W806bOut(vm.ExifFontSize),
                    ["zoom_visible"] = W806bBool(vm.IsZoomIndicatorVisible),
                    ["zoom_text"] = W806bStr(vm.ZoomIndicatorText),
                    ["folder_text"] = W806bStr(vm.FolderInfoText),
                };
                var raisedArray = new JsonArray();
                foreach (var name in raised) raisedArray.Add(JsonValue.Create(name));
                step["expect"] = new JsonObject
                {
                    ["raised"] = raisedArray,
                    ["searches"] = W806bInt(calls - before),
                    ["state"] = state,
                };
                stepsOut.Add(step);
            }
            c["steps"] = stepsOut;
            cases.Add(c);
        }
        return cases;
    }

    // Applies the settings patch keys listed in W806bInfoOverlayViewModelCases. Missing keys are left unchanged.
    private static void W806bApplySettings(AppSettings settings, JsonObject? patch)
    {
        if (patch is null) return;
        if (patch["show_info_overlay"] is { } a) settings.ShowInfoOverlay = a.GetValue<bool>();
        if (patch["show_file_info"] is { } b) settings.ShowFileInfo = b.GetValue<bool>();
        if (patch["show_folder_info"] is { } d) settings.ShowFolderInfo = d.GetValue<bool>();
        if (patch["show_zoom_indicator"] is { } e) settings.ShowZoomIndicator = e.GetValue<bool>();
        if (patch["info_overlay_font_size"] is { } f) settings.InfoOverlayFontSize = W806bNum(f);
        if (patch["previous_folder"] is { } g) settings.Shortcuts.PreviousFolder = g.GetValue<string>();
        if (patch["next_folder"] is { } h) settings.Shortcuts.NextFolder = h.GetValue<string>();
    }
}
