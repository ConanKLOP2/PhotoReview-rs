using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.App.Viewport;
using PhotoReview.App.ViewModels;

// Destination: tools/oracle-cs/Commands/W8-01a.cs (this draft is moved there by the lead).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-01a
//     "viewport-state" => Commands.W801aViewportStateCases(inputs),
//     "arrow-pan" => Commands.W801aArrowPanCases(inputs),
//     "viewport-snapshot-text" => Commands.W801aSnapshotTextCases(inputs),
//     "viewport-values" => Commands.W801aViewportValuesCases(inputs),
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: add a Reference to PhotoReview.App.Shared.dll (HintPath $(CSharpCoreDir)).
//     App.Shared targets net10.0-windows and has PackageReferences (CommunityToolkit.Mvvm, Microsoft.Extensions.DependencyInjection):
//     their DLLs must also sit next to oracle-cs.exe (see the open questions of the contract).
//   * tools/oracle.ps1: add 'PhotoReview.App.Shared.dll' to $requiredDlls and, after the Imaging builds, build
//         src\PhotoReview.App.Shared\PhotoReview.App.Shared.csproj -c Release -o $coreDir
//     (ViewportState and ArrowPanRule are `internal`: they are reached by reflection, the C# repo is not touched).
//
// Conventions (shared with Perf.cs and W1-15.cs):
//   * Inputs: a double is a JSON number, or one of the strings "NaN", "Infinity", "-Infinity", "-0".
//   * Outputs: the same strings for non-finite values and for negative zero, JSON numbers otherwise.
//   * Every case object is the input case with the expected fields added, so the Rust side reads only the golden file.
//   * Culture is pinned to Invariant in every command (".", no group separator, "Infinity" symbols).
// No ambient state: nothing reads the clock, the cwd or the file system.

static partial class Commands
{
    private const BindingFlags W801aFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private const string W801aViewportStateName = "PhotoReview.App.Viewport.ViewportState";
    private const string W801aArrowPanName = "PhotoReview.App.Viewport.ArrowPanRule";

    // ---- JSON <-> double ----

    private static double W801aNum(JsonNode? node)
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

    private static JsonNode W801aOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    private static JsonNode W801aBool(bool value) => JsonValue.Create(value)!;

    // ---- records ----

    private static ViewportInput W801aInput(JsonObject o) => new(
        W801aNum(o["client_width"]), W801aNum(o["client_height"]), W801aNum(o["scroll_bar_thickness"]),
        Enum.Parse<ScrollBarPolicy>(o["scroll_bars"]!.GetValue<string>()),
        Enum.Parse<ViewerStretchMode>(o["stretch"]!.GetValue<string>()),
        W801aNum(o["image_width"]), W801aNum(o["image_height"]),
        W801aNum(o["max_image_width"]), W801aNum(o["max_image_height"]),
        W801aNum(o["bitmap_width"]), W801aNum(o["bitmap_height"]));

    private static RectD W801aRect(JsonObject o) =>
        new(W801aNum(o["x"]), W801aNum(o["y"]), W801aNum(o["width"]), W801aNum(o["height"]));

    private static JsonObject W801aRectOut(RectD r) => new()
    {
        ["x"] = W801aOut(r.X),
        ["y"] = W801aOut(r.Y),
        ["width"] = W801aOut(r.Width),
        ["height"] = W801aOut(r.Height),
    };

    private static ViewportLayout W801aLayoutIn(JsonObject o) => new(
        W801aNum(o["viewport_width"]), W801aNum(o["viewport_height"]),
        W801aNum(o["extent_width"]), W801aNum(o["extent_height"]),
        o["horizontal_bar_visible"]!.GetValue<bool>(), o["vertical_bar_visible"]!.GetValue<bool>(),
        W801aRect(o["image_rect"]!.AsObject()),
        W801aNum(o["max_horizontal_offset"]), W801aNum(o["max_vertical_offset"]));

    private static JsonObject W801aLayoutOut(ViewportLayout l) => new()
    {
        ["viewport_width"] = W801aOut(l.ViewportWidth),
        ["viewport_height"] = W801aOut(l.ViewportHeight),
        ["extent_width"] = W801aOut(l.ExtentWidth),
        ["extent_height"] = W801aOut(l.ExtentHeight),
        ["horizontal_bar_visible"] = W801aBool(l.HorizontalBarVisible),
        ["vertical_bar_visible"] = W801aBool(l.VerticalBarVisible),
        ["image_rect"] = W801aRectOut(l.ImageRect),
        ["max_horizontal_offset"] = W801aOut(l.MaxHorizontalOffset),
        ["max_vertical_offset"] = W801aOut(l.MaxVerticalOffset),
    };

    // ---- viewport-state: reflection over the internal ViewportState ----

    private static object? W801aInvoke(object target, string method, params object?[] args) =>
        target.GetType().GetMethod(method, W801aFlags)!.Invoke(target, args);

    private static object? W801aProp(object target, string name) =>
        target.GetType().GetProperty(name, W801aFlags)!.GetValue(target);

    private static bool W801aDirty(object target) =>
        (bool)target.GetType().GetField("_inputDirty", W801aFlags)!.GetValue(target)!;

    private static JsonObject W801aStateOut(object state, JsonObject? probe)
    {
        var origin = (PointD)W801aProp(state, "ImageOrigin")!;
        var o = new JsonObject
        {
            ["is_layout_pending"] = W801aBool((bool)W801aProp(state, "IsLayoutPending")!),
            ["layout"] = W801aLayoutOut((ViewportLayout)W801aProp(state, "Layout")!),
            ["horizontal_offset"] = W801aOut((double)W801aProp(state, "HorizontalOffset")!),
            ["vertical_offset"] = W801aOut((double)W801aProp(state, "VerticalOffset")!),
            ["image_origin"] = new JsonObject { ["x"] = W801aOut(origin.X), ["y"] = W801aOut(origin.Y) },
            ["image_actual_width"] = W801aOut((double)W801aProp(state, "ImageActualWidth")!),
            ["image_actual_height"] = W801aOut((double)W801aProp(state, "ImageActualHeight")!),
        };
        if (probe is not null)
        {
            var mapped = (PointD)W801aInvoke(state, "ToImageElement", new PointD(W801aNum(probe["x"]), W801aNum(probe["y"])))!;
            o["to_image"] = new JsonObject { ["x"] = W801aOut(mapped.X), ["y"] = W801aOut(mapped.Y) };
        }
        return o;
    }

    /// <summary>
    /// Case {name, steps:[...]}. Each step is the input step plus "expect":
    ///   op "set_input" (input = ViewportInput fields), "scroll_to" (horizontal, vertical), "scroll_home", "update_layout", "query"
    ///   (optional "probe": {x, y} maps a viewport point to image coordinates).
    /// "expect" holds: "returned" (update_layout only), "compute_called" and "clamp_called" (update_layout only: the input was dirty,
    /// and something was pending), "compute" (the layout produced by the engine on this step, only when compute_called),
    /// "clamp" ([horizontal, vertical] the engine returned, only when clamp_called) and "state" (after the step).
    /// </summary>
    public static JsonArray W801aViewportStateCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            var state = Activator.CreateInstance(typeof(ViewportInput).Assembly.GetType(W801aViewportStateName, throwOnError: true)!)!;
            var steps = new JsonArray();
            foreach (var stepNode in c["steps"]!.AsArray())
            {
                var step = stepNode!.AsObject();
                var op = step["op"]!.GetValue<string>();
                var expect = new JsonObject();
                switch (op)
                {
                    case "set_input":
                        W801aInvoke(state, "SetInput", W801aInput(step["input"]!.AsObject()));
                        break;
                    case "scroll_to":
                        W801aInvoke(state, "ScrollTo", W801aNum(step["horizontal"]), W801aNum(step["vertical"]));
                        break;
                    case "scroll_home":
                        W801aInvoke(state, "ScrollHome");
                        break;
                    case "update_layout":
                    {
                        var pending = (bool)W801aProp(state, "IsLayoutPending")!;
                        var dirty = W801aDirty(state);
                        var returned = (bool)W801aInvoke(state, "UpdateLayout")!;
                        expect["returned"] = W801aBool(returned);
                        expect["compute_called"] = W801aBool(dirty);
                        expect["clamp_called"] = W801aBool(pending);
                        if (dirty)
                        {
                            expect["compute"] = W801aLayoutOut((ViewportLayout)W801aProp(state, "Layout")!);
                        }
                        if (pending)
                        {
                            expect["clamp"] = new JsonArray(
                                W801aOut((double)W801aProp(state, "HorizontalOffset")!),
                                W801aOut((double)W801aProp(state, "VerticalOffset")!));
                        }
                        break;
                    }
                    case "query":
                        break;
                    default:
                        throw new InvalidOperationException($"W8-01a viewport-state: unknown op '{op}'");
                }
                expect["state"] = W801aStateOut(state, step["probe"]?.AsObject());
                var stepOut = (JsonObject)step.DeepClone();
                stepOut["expect"] = expect;
                steps.Add(stepOut);
            }
            item["steps"] = steps;
            result.Add(item);
        }
        return result;
    }

    // ---- arrow-pan: reflection over the internal static ArrowPanRule ----

    /// <summary>
    /// Cases by "kind": "constant" -> "value" (DefaultStepFraction); "step_length" (viewport_width, viewport_height,
    /// step_fraction) -> "value"; "step" (dx, dy, horizontal, vertical, layout, step_fraction) -> "outcome" (enum name),
    /// "horizontal", "vertical".
    /// </summary>
    public static JsonArray W801aArrowPanCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var rule = typeof(ViewportInput).Assembly.GetType(W801aArrowPanName, throwOnError: true)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["kind"]!.GetValue<string>())
            {
                case "constant":
                    item["value"] = W801aOut((double)rule.GetField("DefaultStepFraction", W801aFlags)!.GetRawConstantValue()!);
                    break;
                case "step_length":
                    item["value"] = W801aOut((double)rule.GetMethod("StepLength", W801aFlags)!.Invoke(null,
                        new object?[] { W801aNum(c["viewport_width"]), W801aNum(c["viewport_height"]), W801aNum(c["step_fraction"]) })!);
                    break;
                case "step":
                {
                    var layout = W801aLayoutIn(c["layout"]!.AsObject());
                    var r = (ITuple)rule.GetMethod("Step", W801aFlags)!.Invoke(null, new object?[]
                    {
                        c["dx"]!.GetValue<int>(), c["dy"]!.GetValue<int>(),
                        W801aNum(c["horizontal"]), W801aNum(c["vertical"]),
                        layout, W801aNum(c["step_fraction"]),
                    })!;
                    item["outcome"] = JsonValue.Create(r[0]!.ToString());
                    item["horizontal"] = W801aOut((double)r[1]!);
                    item["vertical"] = W801aOut((double)r[2]!);
                    break;
                }
                default:
                    throw new InvalidOperationException("W8-01a arrow-pan: unknown kind");
            }
            result.Add(item);
        }
        return result;
    }

    // ---- viewport-snapshot-text: ViewportSnapshot.ToString (public record) ----

    /// <summary>Cases: the 14 snapshot fields (doubles as conventions above, stretch by name, bars as booleans) -> "text".</summary>
    public static JsonArray W801aSnapshotTextCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        CultureInfo.CurrentUICulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            var snapshot = new ViewportSnapshot(
                W801aNum(c["zoom"]),
                Enum.Parse<ViewerStretchMode>(c["stretch"]!.GetValue<string>()),
                W801aNum(c["max_image_width"]), W801aNum(c["max_image_height"]),
                W801aNum(c["actual_image_width"]), W801aNum(c["actual_image_height"]),
                W801aNum(c["extent_width"]), W801aNum(c["extent_height"]),
                W801aNum(c["viewport_width"]), W801aNum(c["viewport_height"]),
                W801aNum(c["horizontal_offset"]), W801aNum(c["vertical_offset"]),
                c["horizontal_scroll_bar_visible"]!.GetValue<bool>(),
                c["vertical_scroll_bar_visible"]!.GetValue<bool>());
            item["text"] = JsonValue.Create(snapshot.ToString());
            result.Add(item);
        }
        return result;
    }

    // ---- viewport-values: ViewportInput / ViewportLayout equality, RectD.Contains and edges ----

    /// <summary>
    /// Cases by "kind": "input_equals" (a, b: ViewportInput) -> "equal" (record struct Equals, the test SetInput uses);
    /// "layout_equals" (a, b: ViewportLayout) -> "equal" (EqualityComparer, the test UpdateLayout uses);
    /// "rect_contains" (rect, point) -> "contains"; "rect_edges" (rect) -> "right", "bottom".
    /// </summary>
    public static JsonArray W801aViewportValuesCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["kind"]!.GetValue<string>())
            {
                case "input_equals":
                    item["equal"] = W801aBool(W801aInput(c["a"]!.AsObject()).Equals(W801aInput(c["b"]!.AsObject())));
                    break;
                case "layout_equals":
                    item["equal"] = W801aBool(EqualityComparer<ViewportLayout>.Default.Equals(
                        W801aLayoutIn(c["a"]!.AsObject()), W801aLayoutIn(c["b"]!.AsObject())));
                    break;
                case "rect_contains":
                {
                    var p = c["point"]!.AsObject();
                    item["contains"] = W801aBool(W801aRect(c["rect"]!.AsObject())
                        .Contains(new PointD(W801aNum(p["x"]), W801aNum(p["y"]))));
                    break;
                }
                case "rect_edges":
                {
                    var r = W801aRect(c["rect"]!.AsObject());
                    item["right"] = W801aOut(r.Right);
                    item["bottom"] = W801aOut(r.Bottom);
                    break;
                }
                default:
                    throw new InvalidOperationException("W8-01a viewport-values: unknown kind");
            }
            result.Add(item);
        }
        return result;
    }
}
