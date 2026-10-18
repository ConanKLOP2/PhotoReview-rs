using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.Linq;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.ViewModels;
using PhotoReview.Core.Model;

// Destination: tools/oracle-cs/Commands/W8-06a.cs (this draft is moved there by the lead).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-06a
//     "viewer-state-trace" => Commands.W806aViewerStateTraceCases(inputs),
//     "viewer-step-zoom" => Commands.W806aStepZoomCases(inputs),
//     "viewer-display-size" => Commands.W806aDisplaySizeCases(inputs),
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * Same as W8-01a: PhotoReview.App.Shared.dll must be referenced and built into $coreDir (ViewerState is in App.Shared
//     and needs CommunityToolkit.Mvvm next to oracle-cs.exe). The C# repo is not touched.
//   * ViewerState and its public members are reached directly. ViewerState.CalculateStepZoom is `internal static` and is
//     reached by reflection (NonPublic | Static).
//
// Conventions (shared with W8-01a.cs):
//   * A double input is a JSON number, or one of the strings "NaN", "Infinity", "-Infinity", "-0".
//   * A double output is the same strings for non-finite values and for negative zero, a JSON number otherwise.
//   * Culture is invariant in every command.
//   * Each case object echoes the input case, so the Rust test reads only the golden file.
// No ambient state: nothing reads the clock, the cwd or the file system.

static partial class Commands
{
    private static readonly BindingFlags W806aStaticFlags = BindingFlags.NonPublic | BindingFlags.Static;

    // ---- JSON <-> values ----

    private static double W806aNum(JsonNode? node)
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

    private static JsonNode W806aOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    private static JsonNode? W806aOutOrNull(double? value) => value is { } v ? W806aOut(v) : null;

    private static JsonNode W806aIntNode(int value) => JsonValue.Create(value)!;

    private static JsonNode W806aBoolNode(bool value) => JsonValue.Create(value)!;

    private static JsonNode W806aStrNode(string value) => JsonValue.Create(value)!;

    private static int W806aInt(JsonObject o, string key) => o[key]!.GetValue<int>();

    private static bool W806aBool(JsonObject o, string key) => o[key]!.GetValue<bool>();

    // ---- one step of a trace ----

    /// <summary>
    /// Runs one step on <paramref name="state"/> and returns its "expect" object:
    ///   "changes": sorted, distinct C# PropertyChanged names raised by this step (ordinal sort);
    ///   "zoom_mode_changed": bool (ZoomModeChanged raised at least once);
    ///   "swap_events": [{width, height}] the SourcePixelWidth/Height seen by each SourceSizeSwapping listener (BEFORE the change);
    ///   "applied": bool, only for "apply_initial_view_mode";
    ///   "state": the observable state after the step (see W806aStateOut).
    /// </summary>
    private static JsonObject W806aRunStep(ViewerState state, JsonObject step)
    {
        var changed = new List<string>();
        var zoomModeChanged = false;
        var swaps = new JsonArray();
        bool? applied = null;

        PropertyChangedEventHandler onChanged = (_, e) => changed.Add(e.PropertyName ?? string.Empty);
        EventHandler onZoomMode = (_, _) => zoomModeChanged = true;
        EventHandler onSwap = (sender, _) =>
        {
            var viewer = (ViewerState)sender!;
            swaps.Add(new JsonObject
            {
                ["width"] = W806aIntNode(viewer.SourcePixelWidth),
                ["height"] = W806aIntNode(viewer.SourcePixelHeight),
            });
        };

        state.PropertyChanged += onChanged;
        state.ZoomModeChanged += onZoomMode;
        state.SourceSizeSwapping += onSwap;
        try
        {
            switch (step["op"]!.GetValue<string>())
            {
                case "query":
                    break;
                case "set_source_size":
                    state.SetSourceSize(W806aInt(step, "width"), W806aInt(step, "height"), W806aBool(step, "new_image"));
                    break;
                case "swap_source_size":
                    state.SwapSourceSize(W806aInt(step, "width"), W806aInt(step, "height"));
                    break;
                case "set_zoom":
                    state.SetZoom(W806aNum(step["value"]));
                    break;
                case "set_zoom_raw":
                    state.Zoom = W806aNum(step["value"]);
                    break;
                case "zoom_to_actual_size":
                    state.ZoomToActualSize();
                    break;
                case "zoom_to_fit_width":
                    state.ZoomToFitWidth();
                    break;
                case "zoom_to_fit_height":
                    state.ZoomToFitHeight();
                    break;
                case "zoom_in":
                    state.ZoomIn();
                    break;
                case "zoom_out":
                    state.ZoomOut();
                    break;
                case "wheel_zoom":
                    state.WheelZoom(W806aInt(step, "delta"));
                    break;
                case "reset_fit":
                    state.ResetFit(W806aNum(step["viewport_width"]), W806aNum(step["viewport_height"]));
                    break;
                case "update_viewport":
                    state.UpdateViewport(W806aNum(step["viewport_width"]), W806aNum(step["viewport_height"]), W806aBool(step, "force"));
                    break;
                case "apply_initial_view_mode":
                    applied = state.ApplyInitialViewMode(
                        Enum.Parse<InitialViewMode>(step["mode"]!.GetValue<string>()),
                        W806aNum(step["viewport_width"]),
                        W806aNum(step["viewport_height"]),
                        W806aInt(step, "click_zoom_percent"),
                        W806aBool(step, "keep_zoom_across_images"));
                    break;
                case "toggle_fullscreen":
                    state.ToggleFullscreen();
                    break;
                case "exit_fullscreen":
                    state.ExitFullscreen();
                    break;
                case "set_stretch":
                    state.Stretch = Enum.Parse<ViewerStretchMode>(step["value"]!.GetValue<string>());
                    break;
                case "set_dpi_scale":
                    state.DpiScale = W806aNum(step["value"]);
                    break;
                case "set_max_image_width":
                    state.MaxImageWidth = W806aNum(step["value"]);
                    break;
                case "set_max_image_height":
                    state.MaxImageHeight = W806aNum(step["value"]);
                    break;
                case "set_scaling_quality":
                    state.ScalingQuality = Enum.Parse<ScalingQuality>(step["value"]!.GetValue<string>());
                    break;
                case "set_force_high_quality":
                    state.ForceHighQualityScaling = step["value"]!.GetValue<bool>();
                    break;
                case "set_zoom_step":
                    state.ZoomStep = W806aNum(step["value"]);
                    break;
                default:
                    throw new InvalidOperationException("W8-06a: unknown op " + step["op"]);
            }
        }
        finally
        {
            state.PropertyChanged -= onChanged;
            state.ZoomModeChanged -= onZoomMode;
            state.SourceSizeSwapping -= onSwap;
        }

        var expect = new JsonObject
        {
            ["changes"] = W806aSortedNames(changed),
            ["zoom_mode_changed"] = W806aBoolNode(zoomModeChanged),
            ["swap_events"] = swaps,
            ["state"] = W806aStateOut(state),
        };
        if (applied is { } flag) expect["applied"] = W806aBoolNode(flag);
        return expect;
    }

    private static JsonArray W806aSortedNames(List<string> names)
    {
        var array = new JsonArray();
        foreach (var name in names.Distinct(StringComparer.Ordinal).OrderBy(n => n, StringComparer.Ordinal))
        {
            array.Add(W806aStrNode(name));
        }
        return array;
    }

    /// <summary>The observable state, one field per C# member (computed members included).</summary>
    private static JsonObject W806aStateOut(ViewerState s) => new()
    {
        ["zoom"] = W806aOut(s.Zoom),
        ["stretch"] = W806aStrNode(s.Stretch.ToString()),
        ["is_fit"] = W806aBoolNode(s.IsFit),
        ["image_width"] = W806aOut(s.ImageWidth),
        ["image_height"] = W806aOut(s.ImageHeight),
        ["max_image_width"] = W806aOut(s.MaxImageWidth),
        ["max_image_height"] = W806aOut(s.MaxImageHeight),
        ["source_pixel_width"] = W806aIntNode(s.SourcePixelWidth),
        ["source_pixel_height"] = W806aIntNode(s.SourcePixelHeight),
        ["dpi_scale"] = W806aOut(s.DpiScale),
        ["is_fullscreen"] = W806aBoolNode(s.IsFullscreen),
        ["scaling_quality"] = W806aStrNode(s.ScalingQuality.ToString()),
        ["force_high_quality_scaling"] = W806aBoolNode(s.ForceHighQualityScaling),
        ["effective_scaling_quality"] = W806aStrNode(s.EffectiveScalingQuality.ToString()),
        ["effective_zoom"] = W806aOutOrNull(s.EffectiveZoom),
        ["fit_zoom"] = W806aOut(s.FitZoom),
        ["fit_width_zoom"] = W806aOut(s.FitWidthZoom),
        ["fit_height_zoom"] = W806aOut(s.FitHeightZoom),
        ["display_zoom_percent"] = W806aIntNode(s.DisplayZoomPercent),
        ["zoom_step"] = W806aOut(s.ZoomStep),
    };

    // ---- viewer-state-trace: one case per script; every step gets its "expect" ----

    /// <summary>Case {name, steps:[{op, ...args}]}. Output: the same, each step plus "expect" (see W806aRunStep).</summary>
    public static JsonArray W806aViewerStateTraceCases(JsonObject inputs)
    {
        var output = new JsonArray();
        foreach (var caseNode in inputs["cases"]!.AsArray())
        {
            var caseIn = caseNode!.AsObject();
            var state = new ViewerState();
            var steps = new JsonArray();
            foreach (var stepNode in caseIn["steps"]!.AsArray())
            {
                var step = stepNode!.AsObject();
                var stepOut = (JsonObject)step.DeepClone();
                stepOut["expect"] = W806aRunStep(state, step);
                steps.Add(stepOut);
            }
            output.Add(new JsonObject
            {
                ["name"] = W806aStrNode(caseIn["name"]!.GetValue<string>()),
                ["steps"] = steps,
            });
        }
        return output;
    }

    // ---- viewer-step-zoom: reflection over the internal CalculateStepZoom ----

    /// <summary>Case {name, current, step}. Output adds "expected": a number, "NaN", or null for "no change".</summary>
    public static JsonArray W806aStepZoomCases(JsonObject inputs)
    {
        var method = typeof(ViewerState).GetMethod("CalculateStepZoom", W806aStaticFlags)
            ?? throw new InvalidOperationException("W8-06a: ViewerState.CalculateStepZoom not found");
        var output = new JsonArray();
        foreach (var caseNode in inputs["cases"]!.AsArray())
        {
            var c = caseNode!.AsObject();
            var current = W806aNum(c["current"]);
            var step = W806aNum(c["step"]);
            var result = method.Invoke(null, new object[] { current, step });
            JsonNode? expected = result is double d ? W806aOut(d) : null;
            output.Add(new JsonObject
            {
                ["name"] = W806aStrNode(c["name"]!.GetValue<string>()),
                ["current"] = W806aOut(current),
                ["step"] = W806aOut(step),
                ["expected"] = expected,
            });
        }
        return output;
    }

    // ---- viewer-display-size: the public static CalculateDisplaySize ----

    /// <summary>Case {name, width, height, zoom, dpi_scale}. Output adds "expected_width" and "expected_height".</summary>
    public static JsonArray W806aDisplaySizeCases(JsonObject inputs)
    {
        var output = new JsonArray();
        foreach (var caseNode in inputs["cases"]!.AsArray())
        {
            var c = caseNode!.AsObject();
            var width = W806aInt(c, "width");
            var height = W806aInt(c, "height");
            var zoom = W806aNum(c["zoom"]);
            var dpi = W806aNum(c["dpi_scale"]);
            var (w, h) = ViewerState.CalculateDisplaySize(width, height, zoom, dpi);
            output.Add(new JsonObject
            {
                ["name"] = W806aStrNode(c["name"]!.GetValue<string>()),
                ["width"] = W806aIntNode(width),
                ["height"] = W806aIntNode(height),
                ["zoom"] = W806aOut(zoom),
                ["dpi_scale"] = W806aOut(dpi),
                ["expected_width"] = W806aOut(w),
                ["expected_height"] = W806aOut(h),
            });
        }
        return output;
    }
}
