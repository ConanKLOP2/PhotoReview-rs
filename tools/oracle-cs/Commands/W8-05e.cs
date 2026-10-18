using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.Core.Model;

// Destination: tools/oracle-cs/Commands/W8-05e.cs (this draft is moved there by the lead).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-05e
//     "mouse-gestures" => Commands.W805eMouseGestures(inputs),
//     "touchpad-gestures" => Commands.W805eTouchpadGestures(inputs),
//
// The command names are the golden input file names: the runner reads tests/golden-inputs/<command>.json and writes
// tests/golden/<command>.json (the golden-inputs of this draft are copied there by the lead).
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: Reference to PhotoReview.App.Shared.dll (see W8-01a for the same wiring).
//   * The internal classes are reached by reflection (WheelGestureInterpreter, PointerGestures, MiddleClickResolver,
//     TouchpadWheelClassifier, TouchpadSwipeNavigator, TouchpadGestureRules, WheelDeviceHint, WheelInput). The public enums
//     and PointD are referenced directly. The C# repo is not touched.
//
// Conventions (shared with W8-01a.cs):
//   * Inputs: a double is a JSON number, or one of "NaN", "Infinity", "-Infinity", "-0". A null pointer is JSON null.
//   * Outputs: the same strings for non-finite values and negative zero; JSON numbers otherwise. Enum outputs are C# names.
//   * Every case object is the input case with the expected fields added (see the field names per kind below).
//   * Culture pinned to Invariant. No clock, no cwd, no file system: the output is deterministic.
//
// Output fields per kind:
//   wheel_sequence       -> "outcomes": [WheelOutcomeKind name per step]
//   classify_release     -> "action": PointerReleaseAction name
//   fit_double_click     -> "result": bool
//   click_zoom_target    -> "target": ClickZoomTarget name
//   click_zoom_factor    -> "factor": double
//   keyboard_zoom_anchor -> "anchor": {"x", "y"}
//   middle_click         -> "command": ReviewCommandType name, or null
//   classifier_sequence  -> "touchpad": [bool per handle step]
//   within_gap           -> "within": bool
//   navigator_sequence   -> "outcomes": [name per handle step], "gesture_total": [int], "gesture_images": [int]
//   route                -> "action": TouchpadWheelAction name
//   pan_offset           -> "horizontal": double, "vertical": double
// Not generated (not representable in Rust, see contract "Traps"): an undefined MiddleClickAction value.

static partial class Commands
{
    private const BindingFlags W805eFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private const string W805eWheelGestureName = "PhotoReview.App.Input.WheelGestureInterpreter";
    private const string W805ePointerName = "PhotoReview.App.Input.PointerGestures";
    private const string W805eMiddleName = "PhotoReview.App.Input.MiddleClickResolver";
    private const string W805eClassifierName = "PhotoReview.App.Input.TouchpadWheelClassifier";
    private const string W805eNavigatorName = "PhotoReview.App.Input.TouchpadSwipeNavigator";
    private const string W805eRulesName = "PhotoReview.App.Input.TouchpadGestureRules";
    private const string W805eWheelInputName = "PhotoReview.App.Input.WheelInput";
    private const string W805eHintName = "PhotoReview.App.Input.WheelDeviceHint";

    // ---- JSON <-> values ----

    private static double W805eNum(JsonNode? node)
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

    private static JsonNode W805eOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    private static Type W805eType(string name) =>
        typeof(WheelOutcomeKind).Assembly.GetType(name, throwOnError: true)!;

    private static object? W805eStatic(Type type, string method, params object?[] args) =>
        type.GetMethod(method, W805eFlags)!.Invoke(null, args);

    private static object? W805eInstance(object target, string method, params object?[] args) =>
        target.GetType().GetMethod(method, W805eFlags)!.Invoke(target, args);

    private static object W805eHint(string name) =>
        Enum.Parse(W805eType(W805eHintName), name);

    private static object W805eWheelInput(JsonObject c, string hint = "Unknown")
    {
        var type = W805eType(W805eWheelInputName);
        // Positional record struct: (int Delta, bool Horizontal, bool Ctrl, int Timestamp, WheelDeviceHint Hint).
        return Activator.CreateInstance(type, new object?[]
        {
            c["delta"]!.GetValue<int>(),
            c["horizontal"]?.GetValue<bool>() ?? false,
            c["ctrl"]?.GetValue<bool>() ?? false,
            c["timestamp"]?.GetValue<int>() ?? 0,
            W805eHint(hint),
        })!;
    }

    // ---- mouse-gestures ----

    public static JsonArray W805eMouseGestures(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var wheelType = W805eType(W805eWheelGestureName);
        var pointerType = W805eType(W805ePointerName);
        var middleType = W805eType(W805eMiddleName);
        var result = new JsonArray();

        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["kind"]!.GetValue<string>())
            {
                case "wheel_sequence":
                {
                    // A fresh interpreter per case; steps run in order on the same instance (C# state carries over).
                    var wheel = Activator.CreateInstance(wheelType)!;
                    var mode = Enum.Parse<MouseWheelAction>(c["mode"]!.GetValue<string>());
                    var outcomes = new JsonArray();
                    foreach (var step in c["steps"]!.AsArray())
                    {
                        var s = step!.AsObject();
                        var outcome = W805eInstance(wheel, "Handle", s["delta"]!.GetValue<int>(),
                            s["ctrl"]!.GetValue<bool>(), mode);
                        outcomes.Add(JsonValue.Create(outcome!.ToString()));
                    }
                    item["outcomes"] = outcomes;
                    break;
                }
                case "classify_release":
                {
                    var action = W805eStatic(pointerType, "ClassifyRelease",
                        c["dragged"]!.GetValue<bool>(), c["panned"]!.GetValue<bool>(),
                        c["press_was_consumed"]!.GetValue<bool>(), c["click_to_zoom_enabled"]!.GetValue<bool>(),
                        c["kinetic_pan_enabled"]!.GetValue<bool>());
                    item["action"] = JsonValue.Create(action!.ToString());
                    break;
                }
                case "fit_double_click":
                {
                    var r = W805eStatic(pointerType, "IsFitDoubleClick", c["click_count"]!.GetValue<int>());
                    item["result"] = JsonValue.Create((bool)r!);
                    break;
                }
                case "click_zoom_target":
                {
                    var target = W805eStatic(pointerType, "DecideClickZoom", c["is_fit"]!.GetValue<bool>(),
                        W805eNum(c["current_zoom"]), W805eNum(c["click_zoom"]));
                    item["target"] = JsonValue.Create(target!.ToString());
                    break;
                }
                case "click_zoom_factor":
                {
                    var f = W805eStatic(pointerType, "ClickZoomFactor", c["percent"]!.GetValue<int>());
                    item["factor"] = W805eOut((double)f!);
                    break;
                }
                case "keyboard_zoom_anchor":
                {
                    var mode = Enum.Parse<KeyboardZoomAnchor>(c["mode"]!.GetValue<string>());
                    PointD? pointer = null;
                    if (c["pointer"] is JsonObject p)
                        pointer = new PointD(W805eNum(p["x"]), W805eNum(p["y"]));
                    var anchor = (PointD)W805eStatic(pointerType, "ResolveKeyboardZoomAnchor", mode,
                        pointer, W805eNum(c["viewport_width"]), W805eNum(c["viewport_height"]))!;
                    item["anchor"] = new JsonObject { ["x"] = W805eOut(anchor.X), ["y"] = W805eOut(anchor.Y) };
                    break;
                }
                case "middle_click":
                {
                    var action = Enum.Parse<MiddleClickAction>(c["action"]!.GetValue<string>());
                    var command = W805eStatic(middleType, "Resolve", action);
                    // ReviewCommand? is a boxed struct or null; its Type property is the ReviewCommandType name.
                    item["command"] = command is null
                        ? null
                        : JsonValue.Create(command.GetType().GetProperty("Type", W805eFlags)!.GetValue(command)!.ToString());
                    break;
                }
                default:
                    throw new InvalidOperationException($"W8-05e mouse-gestures: unknown kind {c["kind"]}");
            }
            result.Add(item);
        }
        return result;
    }

    // ---- touchpad-gestures ----

    public static JsonArray W805eTouchpadGestures(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var classifierType = W805eType(W805eClassifierName);
        var navigatorType = W805eType(W805eNavigatorName);
        var rulesType = W805eType(W805eRulesName);
        var result = new JsonArray();

        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["kind"]!.GetValue<string>())
            {
                case "classifier_sequence":
                {
                    var classifier = Activator.CreateInstance(classifierType)!;
                    var verdicts = new JsonArray();
                    foreach (var step in c["steps"]!.AsArray())
                    {
                        var s = step!.AsObject();
                        if (s["op"]!.GetValue<string>() == "reset")
                        {
                            W805eInstance(classifier, "Reset");
                            continue;
                        }
                        var touchpad = W805eInstance(classifier, "IsTouchpad", s["delta"]!.GetValue<int>(),
                            s["timestamp"]!.GetValue<int>(), W805eHint(s["hint"]!.GetValue<string>()));
                        verdicts.Add(JsonValue.Create((bool)touchpad!));
                    }
                    item["touchpad"] = verdicts;
                    break;
                }
                case "within_gap":
                {
                    var within = W805eStatic(classifierType, "IsWithinGap", c["last"]!.GetValue<int>(), c["now"]!.GetValue<int>());
                    item["within"] = JsonValue.Create((bool)within!);
                    break;
                }
                case "navigator_sequence":
                {
                    var navigator = Activator.CreateInstance(navigatorType)!;
                    var outcomes = new JsonArray();
                    var totals = new JsonArray();
                    var images = new JsonArray();
                    foreach (var step in c["steps"]!.AsArray())
                    {
                        var s = step!.AsObject();
                        if (s["op"]!.GetValue<string>() == "reset")
                        {
                            W805eInstance(navigator, "Reset");
                            continue;
                        }
                        var outcome = W805eInstance(navigator, "Handle", s["delta"]!.GetValue<int>(),
                            s["timestamp"]!.GetValue<int>(), s["distance"]!.GetValue<int>());
                        outcomes.Add(JsonValue.Create(outcome!.ToString()));
                        // GestureTotal / GestureImages are public getters of the internal class (read after every call).
                        totals.Add(JsonValue.Create((int)navigator.GetType().GetProperty("GestureTotal", W805eFlags)!.GetValue(navigator)!));
                        images.Add(JsonValue.Create((int)navigator.GetType().GetProperty("GestureImages", W805eFlags)!.GetValue(navigator)!));
                    }
                    item["outcomes"] = outcomes;
                    item["gesture_total"] = totals;
                    item["gesture_images"] = images;
                    break;
                }
                case "route":
                {
                    var input = W805eWheelInput(c);
                    var action = W805eStatic(rulesType, "Route", input, c["enabled"]!.GetValue<bool>(),
                        c["is_touchpad"]!.GetValue<bool>(), c["can_pan"]!.GetValue<bool>());
                    item["action"] = JsonValue.Create(action!.ToString());
                    break;
                }
                case "pan_offset":
                {
                    var input = W805eWheelInput(c);
                    var offset = (ValueTuple<double, double>)W805eStatic(rulesType, "PanOffsetDelta", input)!;
                    item["horizontal"] = W805eOut(offset.Item1);
                    item["vertical"] = W805eOut(offset.Item2);
                    break;
                }
                default:
                    throw new InvalidOperationException($"W8-05e touchpad-gestures: unknown kind {c["kind"]}");
            }
            result.Add(item);
        }
        return result;
    }
}
