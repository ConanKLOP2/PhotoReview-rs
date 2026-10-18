using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.Shell.Rendering;
using PhotoReview.Shell.Win32.Overlay;

// Destination: tools/oracle-cs/Commands/W8-15b.cs (this draft is moved there by the lead). NOT compiled or run by the writer.
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-15b
//     "overlay-animator" => Commands.W815bAnimatorCases(inputs),
//     "overlay-frame-delay" => Commands.W815bFrameDelayCases(inputs),
//     "overlay-palette" => Commands.W815bPaletteCases(inputs),
//
// Wiring (the lead does this, as for W7-01b):
//   * OracleCs.csproj gets a reference to PhotoReview.Shell.Win32 (it pulls PhotoReview.Shell.Rendering and App.Shared).
//   * oracle.ps1 builds PhotoReview.Shell.Win32 into $coreDir next to oracle-cs.exe, as it does for App.Shared.
//   * Animator, DarkPalette, IOpacityTarget and Easing are public. FrameDelayTimer is `internal sealed`: it is reached by
//     reflection (public constructor and methods of an internal class), so no InternalsVisibleTo is needed.
//     Animator.Ease is `internal static`: reached by reflection too. DarkPalette.Table is private: read by reflection.
//
// Conventions (as W8-01a and W8-05f): a double is a JSON number, or one of "NaN", "Infinity", "-Infinity", "-0". Every case
// object is the input case with the expected fields added. Culture is invariant. No clock, cwd or file read: time values are
// inputs. Float fields (C# float) are written as the double they widen to; the Rust test narrows back with `as f32`.
//
// Output fields per kind:
//   overlay-animator, kind script: per op `opacity` (object: watched id -> number or null when absent), `is_animating`,
//     `active_count`, `listening` (clock subscriber count > 0), `request_frame` (this op), `changed` (this op), `threw` (only when thrown).
//   overlay-animator, kind ease: `value`.
//   overlay-frame-delay, kind script: per op `running`, `fired` (cumulative), `fired_now`, `request_frame` (this op), `threw`.
//   overlay-palette, kind table: `entries` (declaration order): key, argb, r, g, b, a. kind get: `found` plus r,g,b,a, or `threw`.
//     kind from_argb: r, g, b, a.

static partial class Commands
{
    private const string W815bAssembly = "PhotoReview";
    private const BindingFlags W815bFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    // ---- JSON <-> double (as W8-01a) ----

    private static double W815bNum(JsonNode? node)
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

    private static JsonNode W815bOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    private static Type W815bType(string fullName) =>
        Assembly.Load(W815bAssembly).GetType(fullName, throwOnError: true)!;

    // Runs a method by reflection; a thrown exception comes back as its type name (TargetInvocationException unwrapped).
    private static object? W815bCall(object target, string method, out string? threw, params object?[] args)
    {
        threw = null;
        try
        {
            return target.GetType().GetMethod(method, W815bFlags)!.Invoke(target, args);
        }
        catch (TargetInvocationException ex)
        {
            threw = ex.InnerException!.GetType().Name;
            return null;
        }
    }

    // ---- fake frame clock: same members as the Shell.Tests FakeFrameClock ----

    private sealed class W815bFakeClock : IFrameClock
    {
        private EventHandler<FrameTickEventArgs>? _frame;

        public int RequestFrameCount { get; private set; }

        public int SubscriberCount => _frame?.GetInvocationList().Length ?? 0;

        public event EventHandler<FrameTickEventArgs>? Frame
        {
            add => _frame += value;
            remove => _frame -= value;
        }

        public void RequestFrame() => RequestFrameCount++;

        public void Tick(long timestamp) =>
            _frame?.Invoke(this, new FrameTickEventArgs(new FrameTick(timestamp, 0, null)));
    }

    private sealed class W815bOpacity : IOpacityTarget
    {
        public float Opacity { get; set; }
    }

    // ---- overlay-animator: Animator (script) and Animator.Ease (ease) ----

    public static JsonArray W815bAnimatorCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            if (c["kind"]!.GetValue<string>() == "ease")
            {
                W815bEaseCase(c);
            }
            else
            {
                W815bAnimatorScript(c);
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static void W815bEaseCase(JsonObject c)
    {
        Type easingType = W815bType("PhotoReview.Shell.Win32.Overlay.Easing");
        object easing = Enum.Parse(easingType, c["easing"]!.GetValue<string>());
        MethodInfo ease = W815bType("PhotoReview.Shell.Win32.Overlay.Animator").GetMethod("Ease", BindingFlags.NonPublic | BindingFlags.Static)!;
        var values = new JsonArray();
        foreach (var t in c["t"]!.AsArray())
        {
            values.Add(W815bOut((double)ease.Invoke(null, new object[] { easing, W815bNum(t) })!));
        }

        c["values"] = values;
    }

    private static void W815bAnimatorScript(JsonObject c)
    {
        long ticksPerSecond = c["ticks_per_second"]!.GetValue<long>();
        var clock = new W815bFakeClock();
        var store = new Dictionary<string, W815bOpacity?>(StringComparer.Ordinal);
        foreach (var kv in c["elements"]!.AsObject())
        {
            store[kv.Key] = new W815bOpacity { Opacity = (float)W815bNum(kv.Value) };
        }

        var animator = new Animator(clock, id => store.GetValueOrDefault(id), ticksPerSecond);
        int changedCount = 0;
        animator.Changed += (_, _) => changedCount++;
        var watch = c["watch"]!.AsArray().Select(n => n!.GetValue<string>()).ToArray();

        var results = new JsonArray();
        foreach (var opNode in c["ops"]!.AsArray())
        {
            var op = opNode!.AsObject();
            string name = op["op"]!.GetValue<string>();
            int requests0 = clock.RequestFrameCount;
            int changed0 = changedCount;
            string? threw = null;
            switch (name)
            {
                case "fade":
                    Easing easing = Enum.Parse<Easing>(op["easing"]!.GetValue<string>());
                    try
                    {
                        animator.FadeTo(op["id"]!.GetValue<string>(), (float)W815bNum(op["target"]),
                            TimeSpan.FromTicks(op["duration_ticks"]!.GetValue<long>()), easing);
                    }
                    catch (ObjectDisposedException ex)
                    {
                        threw = ex.GetType().Name;
                    }

                    break;
                case "tick":
                    clock.Tick(op["timestamp"]!.GetValue<long>());
                    break;
                case "remove":
                    store[op["id"]!.GetValue<string>()] = null;
                    break;
                case "restore":
                    store[op["id"]!.GetValue<string>()] = new W815bOpacity { Opacity = (float)W815bNum(op["opacity"]) };
                    break;
                case "set":
                    store[op["id"]!.GetValue<string>()]!.Opacity = (float)W815bNum(op["opacity"]);
                    break;
                case "cancel":
                    animator.Cancel(op["id"]!.GetValue<string>());
                    break;
                case "dispose":
                    animator.Dispose();
                    break;
                default:
                    throw new InvalidOperationException("unknown animator op " + name);
            }

            var opacity = new JsonObject();
            foreach (string id in watch)
            {
                opacity[id] = store.TryGetValue(id, out W815bOpacity? target) && target is not null
                    ? W815bOut(target.Opacity)
                    : null;
            }

            var res = new JsonObject
            {
                ["op"] = name,
                ["opacity"] = opacity,
                ["is_animating"] = animator.IsAnimating,
                ["active_count"] = animator.ActiveCount,
                ["listening"] = clock.SubscriberCount > 0,
                ["request_frame"] = clock.RequestFrameCount > requests0,
                ["changed"] = changedCount > changed0,
            };
            if (threw is not null)
            {
                res["threw"] = threw;
            }

            results.Add(res);
        }

        c["results"] = results;
    }

    // ---- overlay-frame-delay: FrameDelayTimer (internal, reflection) ----

    public static JsonArray W815bFrameDelayCases(JsonObject inputs)
    {
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            long ticksPerSecond = c["ticks_per_second"]!.GetValue<long>();
            var clock = new W815bFakeClock();
            long now = 0;
            int fired = 0;
            object? timer = null;
            JsonObject? restart = c["restart"]?.AsObject();
            Action elapsed = () =>
            {
                fired++;
                if (restart is not null && fired < restart["until_fired"]!.GetValue<int>())
                {
                    W815bCall(timer!, "Start", out _, TimeSpan.FromMilliseconds(restart["delay_ms"]!.GetValue<double>()));
                }
            };
            Type timerType = W815bType("PhotoReview.Shell.Win32.Overlay.FrameDelayTimer");
            timer = Activator.CreateInstance(timerType, new object?[] { clock, elapsed, (Func<long>)(() => now), (long?)ticksPerSecond })!;

            var results = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                string name = op["op"]!.GetValue<string>();
                if (op["now"] is not null)
                {
                    now = op["now"]!.GetValue<long>();
                }

                int requests0 = clock.RequestFrameCount;
                int fired0 = fired;
                string? threw = null;
                switch (name)
                {
                    case "start":
                        W815bCall(timer, "Start", out threw, TimeSpan.FromTicks(op["delay_ticks"]!.GetValue<long>()));
                        break;
                    case "stop":
                        W815bCall(timer, "Stop", out threw);
                        break;
                    case "tick":
                        clock.Tick(op["timestamp"]!.GetValue<long>());
                        break;
                    case "dispose":
                        ((IDisposable)timer).Dispose();
                        break;
                    default:
                        throw new InvalidOperationException("unknown frame-delay op " + name);
                }

                var res = new JsonObject
                {
                    ["op"] = name,
                    ["running"] = (bool)timerType.GetProperty("IsRunning", W815bFlags)!.GetValue(timer)!,
                    ["fired"] = fired,
                    ["fired_now"] = fired > fired0,
                    ["request_frame"] = clock.RequestFrameCount > requests0,
                };
                if (threw is not null)
                {
                    res["threw"] = threw;
                }

                results.Add(res);
            }

            c["results"] = results;
            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    // ---- overlay-palette: DarkPalette ----

    private static void W815bRgba(JsonObject target, object color)
    {
        Type colorType = color.GetType();
        target["r"] = W815bOut((float)colorType.GetProperty("R")!.GetValue(color)!);
        target["g"] = W815bOut((float)colorType.GetProperty("G")!.GetValue(color)!);
        target["b"] = W815bOut((float)colorType.GetProperty("B")!.GetValue(color)!);
        target["a"] = W815bOut((float)colorType.GetProperty("A")!.GetValue(color)!);
    }

    public static JsonArray W815bPaletteCases(JsonObject inputs)
    {
        Type palette = W815bType("PhotoReview.Shell.Win32.Overlay.DarkPalette");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "table":
                    var entries = new JsonArray();
                    var table = (Array)palette.GetField("Table", BindingFlags.NonPublic | BindingFlags.Static)!.GetValue(null)!;
                    foreach (object entry in table)
                    {
                        string key = (string)entry.GetType().GetField("Item1")!.GetValue(entry)!;
                        uint argb = (uint)entry.GetType().GetField("Item2")!.GetValue(entry)!;
                        object color = palette.GetMethod("Get", W815bFlags)!.Invoke(null, new object[] { key })!;
                        var e = new JsonObject { ["key"] = key, ["argb"] = argb };
                        W815bRgba(e, color);
                        entries.Add(e);
                    }

                    c["entries"] = entries;
                    break;
                case "get":
                    // DarkPalette.Get throws KeyNotFoundException for an unknown key: recorded as threw, found = false.
                    try
                    {
                        object found = palette.GetMethod("Get", W815bFlags)!.Invoke(null, new object[] { c["key"]!.GetValue<string>() })!;
                        c["found"] = true;
                        W815bRgba(c, found);
                    }
                    catch (TargetInvocationException ex)
                    {
                        c["found"] = false;
                        c["threw"] = ex.InnerException!.GetType().Name;
                    }

                    break;
                case "from_argb":
                    object rgba = palette.GetMethod("FromArgb", W815bFlags)!.Invoke(null, new object[] { c["argb"]!.GetValue<uint>() })!;
                    W815bRgba(c, rgba);
                    break;
                default:
                    throw new InvalidOperationException("unknown palette kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
