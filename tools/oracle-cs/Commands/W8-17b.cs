using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;

// W8-17b oracle (the F half of the unit): the state machine of StartupCloak and the table of ShellPlacement.ShowCommandFor of the baseline. NOT compiled or run by the writer
// (the lead does). ShellPlacement.ApplyBeforeShow / Save take the `internal` interface IWindowPlacementNative, which the oracle cannot implement or proxy: their expectations
// are hand-written from ShellPlacementTests (see contracts/W8-17b.md).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-17b
//     "startup-cloak" => Commands.W817bStartupCloakCases(inputs),
//     "shell-show-command" => Commands.W817bShowCommandCases(inputs),
// Wiring (the lead does this): none for the csproj. OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" = PhotoReview.Shell.Win32), PhotoReview.Core and
// PhotoReview.App.Shared. `StartupCloak` and `ShellPlacement` are `internal`, so they are reached by reflection (as W8-18o does): the constructor and `Hide`, `Reveal`,
// `NoteFirstPresent`, `Stop`, `Dispose`, `IsHidden`, `IsRevealed` are public members of the internal class. The seams are public types: `IUiDispatcher` (Core),
// `Action`, `Func<nint,bool,bool>`, `Func<TimeSpan,Action,IDisposable>`. Nothing here creates a window, a DWM call or a real timer.
//
// Inputs (tests/golden-inputs/startup-cloak.json):
//   "scripts": [ { "id", "accept": bool (DWM accepts the cloak), "timeout_ms": int|null (the safetyTimeout argument; null = the default), "ops": [string] } ]
//   op strings: "hide:<hwnd>", "note_first_present", "reveal", "stop", "dispose", "fire" (the manual scheduler runs the scheduled job if it was not cancelled).
//   After EVERY op the case records the state, so "states" has one entry per op: { "op", "is_hidden", "is_revealed", "revealed_count", "fire_ran" (only for "fire") }.
//   Output = the input case plus "kind": "script", "states": [...], "calls": ["set:<hwnd>:<true|false>", ...] (the setCloaked seam, in order), "delays_ms": [int] (the delays
//   handed to the scheduler, in order), "timer_disposed": bool (the last scheduled timer was disposed).
// Inputs (tests/golden-inputs/shell-show-command.json):
//   "show_command": [ { "id", "state": "Normal" | "Minimized" | "Maximized" | null, "activate": bool } ] -> "result": int (ShellPlacement.ShowCommandFor)
//   Output also carries { "kind": "constants", "safety_timeout_ms": StartupCloak.SafetyTimeout in ms } as the last element of the startup-cloak command.
static partial class Commands
{
    private const string W817bAssembly = "PhotoReview";
    private const BindingFlags W817bFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W817bType(string fullName) => Assembly.Load(W817bAssembly).GetType(fullName, throwOnError: true)!;

    /// <summary>Runs every posted action inline (the C# test double <c>InlineDispatcher</c>).</summary>
    private sealed class W817bInlineDispatcher : IUiDispatcher
    {
        public bool CheckAccess() => true;

        public void Post(Action action) => action();

        public void Post(Action action, UiPriority priority) => action();

        public Task InvokeAsync(Action action)
        {
            action();
            return Task.CompletedTask;
        }

        public Task InvokeAsync(Action action, UiPriority priority) => InvokeAsync(action);

        public ValueTask YieldAsync(CancellationToken cancellationToken = default) => ValueTask.CompletedTask;

        public ValueTask YieldAsync(UiPriority priority, CancellationToken cancellationToken = default) => ValueTask.CompletedTask;
    }

    /// <summary>The injectable timer of the C# tests: no clock, no thread pool; <c>Fire</c> runs the scheduled job unless the handle was disposed.</summary>
    private sealed class W817bManualScheduler
    {
        private Action? _callback;

        public List<int> DelaysMs { get; } = [];

        public bool Disposed { get; private set; }

        public IDisposable Schedule(TimeSpan delay, Action callback)
        {
            DelaysMs.Add((int)delay.TotalMilliseconds);
            _callback = callback;
            Disposed = false;
            return new Handle(this);
        }

        public bool Fire()
        {
            if (Disposed || _callback is null)
            {
                return false;
            }

            _callback();
            return true;
        }

        private sealed class Handle(W817bManualScheduler owner) : IDisposable
        {
            public void Dispose() => owner.Disposed = true;
        }
    }

    public static JsonArray W817bStartupCloakCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var cloakType = W817bType("PhotoReview.Shell.Win32.Startup.StartupCloak");
        var ctor = cloakType.GetConstructors(W817bFlags).Single(k => !k.IsStatic);
        var result = new JsonArray();
        foreach (var node in inputs["scripts"]!.AsArray())
        {
            var input = node!.AsObject();
            var accept = input["accept"]!.GetValue<bool>();
            var calls = new List<string>();
            var revealed = 0;
            var scheduler = new W817bManualScheduler();
            Func<nint, bool, bool> setCloaked = (hwnd, cloaked) =>
            {
                calls.Add("set:" + (long)hwnd + ":" + (cloaked ? "true" : "false"));
                return accept;
            };
            TimeSpan? timeout = input["timeout_ms"] is { } ms ? TimeSpan.FromMilliseconds(ms.GetValue<int>()) : null;
            // Parameter order of the baseline constructor: (dispatcher, onRevealed, setCloaked, safetyTimeout, scheduleTimeout).
            var cloak = ctor.Invoke([new W817bInlineDispatcher(), (Action)(() => revealed++), setCloaked, timeout,
                (Func<TimeSpan, Action, IDisposable>)scheduler.Schedule]);
            var states = new JsonArray();
            foreach (var opNode in input["ops"]!.AsArray())
            {
                var op = opNode!.GetValue<string>();
                bool? fireRan = null;
                if (op.StartsWith("hide:", StringComparison.Ordinal))
                {
                    cloakType.GetMethod("Hide", W817bFlags)!.Invoke(cloak, [(nint)long.Parse(op["hide:".Length..], CultureInfo.InvariantCulture)]);
                }
                else if (op == "note_first_present")
                {
                    cloakType.GetMethod("NoteFirstPresent", W817bFlags)!.Invoke(cloak, null);
                }
                else if (op == "reveal")
                {
                    cloakType.GetMethod("Reveal", W817bFlags)!.Invoke(cloak, null);
                }
                else if (op == "stop")
                {
                    cloakType.GetMethod("Stop", W817bFlags)!.Invoke(cloak, null);
                }
                else if (op == "dispose")
                {
                    ((IDisposable)cloak).Dispose();
                }
                else if (op == "fire")
                {
                    fireRan = scheduler.Fire();
                }
                else
                {
                    throw new InvalidOperationException("Unknown op in the input: " + op);
                }

                var state = new JsonObject
                {
                    ["op"] = op,
                    ["is_hidden"] = (bool)cloakType.GetProperty("IsHidden", W817bFlags)!.GetValue(cloak)!,
                    ["is_revealed"] = (bool)cloakType.GetProperty("IsRevealed", W817bFlags)!.GetValue(cloak)!,
                    ["revealed_count"] = revealed,
                };
                if (fireRan is { } ran)
                {
                    state["fire_ran"] = ran;
                }

                states.Add(state);
            }

            var entry = (JsonObject)input.DeepClone();
            entry["kind"] = "script";
            entry["states"] = states;
            entry["calls"] = new JsonArray(calls.Select(c => (JsonNode?)JsonValue.Create(c)).ToArray());
            entry["delays_ms"] = new JsonArray(scheduler.DelaysMs.Select(d => (JsonNode?)JsonValue.Create(d)).ToArray());
            entry["timer_disposed"] = scheduler.Disposed;
            result.Add(entry);
        }

        var safety = (TimeSpan)cloakType.GetField("SafetyTimeout", W817bFlags)!.GetValue(null)!;
        result.Add(new JsonObject { ["kind"] = "constants", ["safety_timeout_ms"] = (int)safety.TotalMilliseconds });
        return result;
    }

    public static JsonArray W817bShowCommandCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var placement = W817bType("PhotoReview.Shell.Win32.Startup.ShellPlacement");
        var method = placement.GetMethod("ShowCommandFor", W817bFlags)!;
        var stateType = method.GetParameters()[0].ParameterType; // Nullable<WindowShowState>
        var enumType = Nullable.GetUnderlyingType(stateType)!;
        var result = new JsonArray();
        foreach (var node in inputs["show_command"]!.AsArray())
        {
            var input = node!.AsObject();
            object? state = input["state"]?.GetValue<string>() is { } name
                ? Activator.CreateInstance(stateType, Enum.Parse(enumType, name))
                : null;
            var entry = (JsonObject)input.DeepClone();
            entry["kind"] = "show_command";
            entry["result"] = (int)method.Invoke(null, [state, input["activate"]!.GetValue<bool>()])!;
            result.Add(entry);
        }

        return result;
    }
}
