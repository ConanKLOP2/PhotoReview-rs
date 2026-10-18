using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Model;

// Destination: tools/oracle-cs/Commands/W8-05f.cs (this draft is moved there by the lead). NOT compiled or run by the writer.
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-05f
//     "kinetic-velocity" => Commands.W805fVelocityCases(inputs),
//     "kinetic-scroller" => Commands.W805fScrollerCases(inputs),
//     "keyboard-pan" => Commands.W805fKeyboardCases(inputs),
//     "glide-clock" => Commands.W805fGlideCases(inputs),
//
// Wiring (the lead does this, as for W8-01a, open question O6 of that contract):
//   * PhotoReview.App.Shared is built into $coreDir and its package DLLs sit next to oracle-cs.exe.
//   * PanVelocityTracker, ScrollBounds, KineticScroller, KeyboardPanResult, KeyboardPan and GlideFrameClock are `internal`:
//     they are reached by reflection on the PhotoReview.App.Shared assembly (the C# repo is not touched).
//   * DisplayTiming and KineticGlideSmoothing are public and used directly.
//
// Conventions (as W8-01a): a double is a JSON number, or one of "NaN", "Infinity", "-Infinity", "-0"; outputs use the same
// strings for non-finite values and for negative zero. Every case object is the input case with the expected fields added.
// Culture is invariant. No clock, cwd or file read: `now` values are inputs.
//
// Output fields per kind:
//   kinetic-velocity: `count`, `vx`, `vy` (kinds velocity, reset, reset_then_velocity).
//   kinetic-scroller: kind start -> `ret`, `vx`, `vy`, `active`; kind script -> `results`: one object per op with `ret` (start,
//     advance-like bool), `h`, `v` (step only), and the state after the op: `vx`, `vy`, `active`.
//   keyboard-pan: step_pixels -> `value`; step -> `result` (enum name), `h`, `v`; is_zoomed -> `value`; impulse -> `vx`, `vy`;
//     consumes -> `value`.
//   glide-clock: kind script -> `results`: one object per op with `needs` (NeedsDisplayTiming after the op) and, for advance,
//     `ret`.

static partial class Commands
{
    private const string W805fAssembly = "PhotoReview.App.Shared";
    private const BindingFlags W805fFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    // ---- JSON <-> double (as W8-01a) ----

    private static double W805fNum(JsonNode? node)
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

    private static JsonNode W805fOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    // ---- reflection helpers ----

    private static Type W805fType(string name) =>
        Assembly.Load(W805fAssembly).GetType("PhotoReview.App.Input." + name, throwOnError: true)!;

    private static object? W805fCall(object? target, Type type, string method, params object?[] args) =>
        type.GetMethod(method, W805fFlags)!.Invoke(target, args);

    private static object? W805fGet(object target, string property) =>
        target.GetType().GetProperty(property, W805fFlags)!.GetValue(target);

    private static object? W805fItem(object tuple, string field) =>
        tuple.GetType().GetField(field, W805fFlags)!.GetValue(tuple);

    private static object W805fBounds(JsonNode node)
    {
        var a = node.AsArray();
        return Activator.CreateInstance(W805fType("ScrollBounds"),
            new object[] { W805fNum(a[0]), W805fNum(a[1]), W805fNum(a[2]), W805fNum(a[3]) })!;
    }

    private static DisplayTiming? W805fTiming(JsonNode? node) =>
        node is null ? null : new DisplayTiming(node["last_vblank"]!.GetValue<long>(), node["refresh_period"]!.GetValue<long>());

    private static object W805fEnum(string typeName, string value) => Enum.Parse(W805fType(typeName), value);
    // ---- kinetic-velocity: PanVelocityTracker ----

    public static JsonArray W805fVelocityCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var type = W805fType("PanVelocityTracker");
            var tracker = Activator.CreateInstance(type)!;
            void Feed(JsonArray samples)
            {
                foreach (var s in samples)
                {
                    var a = s!.AsArray();
                    W805fCall(tracker, type, "Add", W805fNum(a[0]), W805fNum(a[1]), W805fNum(a[2]));
                }
            }

            Feed(c["samples"]!.AsArray());
            if (kind is "reset" or "reset_then_velocity") W805fCall(tracker, type, "Reset");
            if (kind == "reset_then_velocity") Feed(c["after"]!.AsArray());

            var velocity = W805fCall(tracker, type, "GetVelocity")!;
            c["count"] = (int)W805fGet(tracker, "Count")!;
            c["vx"] = W805fOut((double)W805fItem(velocity, "Item1")!);
            c["vy"] = W805fOut((double)W805fItem(velocity, "Item2")!);
            outCases.Add(c.DeepClone());
        }
        return outCases;
    }

    // ---- kinetic-scroller: KineticScroller ----

    private static void W805fScrollerState(JsonObject target, object scroller)
    {
        target["vx"] = W805fOut((double)W805fGet(scroller, "VelocityX")!);
        target["vy"] = W805fOut((double)W805fGet(scroller, "VelocityY")!);
        target["active"] = (bool)W805fGet(scroller, "IsActive")!;
    }

    public static JsonArray W805fScrollerCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var type = W805fType("KineticScroller");
            var scroller = Activator.CreateInstance(type)!; // boxed: Invoke mutates the box in place
            if (c["kind"]!.GetValue<string>() == "start")
            {
                c["ret"] = (bool)W805fCall(scroller, type, "Start", W805fNum(c["vx"]), W805fNum(c["vy"]))!;
                W805fScrollerState(c, scroller);
            }
            else
            {
                var results = new JsonArray();
                foreach (var opNode in c["ops"]!.AsArray())
                {
                    var op = opNode!.AsObject();
                    var name = op["op"]!.GetValue<string>();
                    var result = new JsonObject();
                    switch (name)
                    {
                        case "start":
                            result["ret"] = (bool)W805fCall(scroller, type, "Start", W805fNum(op["vx"]), W805fNum(op["vy"]))!;
                            break;
                        case "stop":
                            W805fCall(scroller, type, "Stop");
                            break;
                        case "impulse":
                            W805fCall(scroller, type, "AddImpulse", W805fNum(op["vx"]), W805fNum(op["vy"]));
                            break;
                        case "step":
                            var moved = W805fCall(scroller, type, "Step", W805fNum(op["elapsed"]),
                                W805fNum(op["h"]), W805fNum(op["v"]), W805fBounds(op["bounds"]!))!;
                            result["h"] = W805fOut((double)W805fItem(moved, "Item1")!);
                            result["v"] = W805fOut((double)W805fItem(moved, "Item2")!);
                            break;
                        default:
                            throw new InvalidOperationException("unknown op " + name);
                    }
                    W805fScrollerState(result, scroller);
                    results.Add(result);
                }
                c["results"] = results;
            }
            outCases.Add(c.DeepClone());
        }
        return outCases;
    }

    // ---- keyboard-pan: KeyboardPan (static) ----

    public static JsonArray W805fKeyboardCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        var pan = W805fType("KeyboardPan");
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "step_pixels":
                    c["value"] = W805fOut((double)W805fCall(null, pan, "StepPixels",
                        W805fNum(c["viewport_width"]), W805fNum(c["viewport_height"]), W805fNum(c["step_fraction"]))!);
                    break;
                case "step":
                    var moved = W805fCall(null, pan, "Step", (int)c["dx"]!.GetValue<int>(), (int)c["dy"]!.GetValue<int>(),
                        W805fNum(c["h"]), W805fNum(c["v"]), W805fBounds(c["bounds"]!), W805fNum(c["step_fraction"]))!;
                    c["result"] = W805fItem(moved, "Item1")!.ToString();
                    c["h"] = W805fOut((double)W805fItem(moved, "Item2")!);
                    c["v"] = W805fOut((double)W805fItem(moved, "Item3")!);
                    break;
                case "is_zoomed":
                    c["value"] = (bool)W805fCall(null, pan, "IsZoomed", W805fBounds(c["bounds"]!))!;
                    break;
                case "impulse":
                    var impulse = W805fCall(null, pan, "ImpulseVelocity", W805fNum(c["step_x"]), W805fNum(c["step_y"]))!;
                    c["vx"] = W805fOut((double)W805fItem(impulse, "Item1")!);
                    c["vy"] = W805fOut((double)W805fItem(impulse, "Item2")!);
                    break;
                case "consumes":
                    c["value"] = (bool)W805fCall(null, pan, "ConsumesKey",
                        W805fEnum("KeyboardPanResult", c["result"]!.GetValue<string>()),
                        c["is_repeat"]!.GetValue<bool>(), c["navigates_at_edge"]!.GetValue<bool>(),
                        W805fBounds(c["bounds"]!))!;
                    break;
                default:
                    throw new InvalidOperationException("unknown kind");
            }
            outCases.Add(c.DeepClone());
        }
        return outCases;
    }

    // ---- glide-clock: GlideFrameClock ----

    public static JsonArray W805fGlideCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        var type = W805fType("GlideFrameClock");
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var clock = Activator.CreateInstance(type)!; // boxed: Start/Advance mutate the box in place
            var results = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                var result = new JsonObject();
                if (op["op"]!.GetValue<string>() == "start")
                {
                    var mode = Enum.Parse<KineticGlideSmoothing>(op["mode"]!.GetValue<string>());
                    W805fCall(clock, type, "Start", mode, W805fNum(op["ticks_per_ms"]));
                }
                else
                {
                    result["ret"] = W805fOut((double)W805fCall(clock, type, "Advance", W805fNum(op["rendering_ms"]),
                        op["now"]!.GetValue<long>(), W805fTiming(op["timing"]))!);
                }
                result["needs"] = (bool)W805fGet(clock, "NeedsDisplayTiming")!;
                results.Add(result);
            }
            c["results"] = results;
            outCases.Add(c.DeepClone());
        }
        return outCases;
    }
}
