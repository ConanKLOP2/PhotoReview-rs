using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-16a
//     "full-decode-gate" => Commands.FullDecodeGateScenarios(inputs),
// Needs <Reference Include="PhotoReview.Imaging.LibRaw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.LibRaw.dll</HintPath></Reference> in
// OracleCs.csproj (add it unless another W5 unit already did; tools/oracle.ps1 must copy that DLL into CSharpCoreDir). libraw.dll is NOT
// needed: FullDecodeGate is managed code. FullDecodeGate is `internal` (InternalsVisibleTo covers only the test project), so it is driven
// through reflection; TargetInvocationException is unwrapped. Self-contained: no other oracle file.
//
// W5-16a: deterministic FullDecodeGate scenarios. Every waiter is queued (or refused/cancelled) while the holder owns the slot; then the
// holder is released and every granted lease records its name, marks itself worked when asked, and releases at once, so the grant order
// is fully determined by the gate's lane/aging rules (no timing involved).
// Case = { name, max_queued_preloads, holder: null | { priority: "Viewer"|"Preload", worked, release_twice? }, steps: [...] }
//   step { op: "enter", name, priority, worked }   a thread enters; the oracle waits until it is queued, refused, failed or (free slot) done.
//   step { op: "enter_canceled", name, priority }  Enter with an already-cancelled token, on the main thread.
//   step { op: "cancel", name }                    cancels a queued waiter's token and waits for its thread to end.
//   step { op: "counters" }                        snapshots { slots, viewers, preloads } into `snapshots`.
// Result = { name, outcomes: { <name>: "granted" | "busy" | "canceled" | <other exception type> }, busy_messages: [..distinct..],
//            order: [names in grant order], snapshots: [...], before_release: counters, final: counters }.
static partial class Commands
{
    private static readonly TimeSpan W516aBound = TimeSpan.FromSeconds(30);

    private sealed class W516aGate
    {
        private static readonly Type GateType = Type.GetType("PhotoReview.Imaging.LibRaw.FullDecodeGate, PhotoReview.Imaging.LibRaw", throwOnError: true)!;
        private static readonly Type LeaseType = GateType.GetNestedType("Lease", BindingFlags.NonPublic | BindingFlags.Public)!;
        private const BindingFlags Inst = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;
        private readonly object _gate;

        internal W516aGate(int maxQueuedPreloads, Action<SourceReadPriority> waiterQueued)
        {
            var ctor = GateType.GetConstructor(Inst, [typeof(int), typeof(Action<SourceReadPriority>)])!;
            _gate = ctor.Invoke([maxQueuedPreloads, waiterQueued]);
        }

        private int Get(string property) => (int)GateType.GetProperty(property, Inst)!.GetValue(_gate)!;

        internal JsonObject Counters() => new()
        {
            ["slots"] = Get("SlotsAvailable"),
            ["viewers"] = Get("QueuedViewers"),
            ["preloads"] = Get("QueuedPreloads"),
        };

        internal IDisposable Enter(SourceReadPriority priority, CancellationToken token)
        {
            try
            {
                return (IDisposable)GateType.GetMethod("Enter", Inst)!.Invoke(_gate, [priority, token])!;
            }
            catch (TargetInvocationException ex) when (ex.InnerException is not null)
            {
                System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(ex.InnerException).Throw();
                throw; // unreachable
            }
        }

        internal static void MarkWorked(IDisposable lease) => LeaseType.GetMethod("MarkWorked", Inst)!.Invoke(lease, null);
    }

    private static SourceReadPriority W516aPriority(JsonNode? node) =>
        (string?)node == "Preload" ? SourceReadPriority.Preload : SourceReadPriority.Viewer;

    private static string W516aOutcome(Exception ex) => ex switch
    {
        DecoderBusyException => "busy",
        OperationCanceledException => "canceled",
        _ => ex.GetType().Name,
    };

    public static JsonArray FullDecodeGateScenarios(JsonObject inputs)
    {
        var cases = new JsonArray();
        foreach (var input in inputs["cases"]!.AsArray())
        {
            var c = input!.AsObject();
            ManualResetEventSlim? settled = null; // the event of the step currently being driven
            var gate = new W516aGate((int)c["max_queued_preloads"]!, _ => settled?.Set());
            var order = new List<string>();
            var outcomes = new JsonObject();
            var busyMessages = new SortedSet<string>(StringComparer.Ordinal);
            var snapshots = new JsonArray();
            var threads = new Dictionary<string, (Task Task, CancellationTokenSource Cts)>();

            void Record(string name, string outcome)
            {
                lock (outcomes) outcomes[name] = outcome;
            }

            IDisposable? holder = null;
            var holderSpec = c["holder"] as JsonObject;
            if (holderSpec is not null) holder = gate.Enter(W516aPriority(holderSpec["priority"]), CancellationToken.None);

            foreach (var stepNode in c["steps"]!.AsArray())
            {
                var step = stepNode!.AsObject();
                var op = (string)step["op"]!;
                switch (op)
                {
                    case "enter":
                    {
                        var name = (string)step["name"]!;
                        var priority = W516aPriority(step["priority"]);
                        var worked = (bool?)step["worked"] ?? true;
                        var cts = new CancellationTokenSource();
                        var stepSettled = new ManualResetEventSlim();
                        settled = stepSettled;
                        var task = Task.Factory.StartNew(() =>
                        {
                            try
                            {
                                using var lease = gate.Enter(priority, cts.Token);
                                if (worked) W516aGate.MarkWorked(lease);
                                lock (order) order.Add(name);
                                Record(name, "granted");
                            }
                            catch (Exception ex)
                            {
                                if (ex is DecoderBusyException) lock (busyMessages) busyMessages.Add(ex.Message);
                                Record(name, W516aOutcome(ex));
                            }
                            finally
                            {
                                stepSettled.Set();
                            }
                        }, TaskCreationOptions.LongRunning);
                        if (!stepSettled.Wait(W516aBound)) throw new TimeoutException($"{c["name"]}: step {name} never settled");
                        settled = null;
                        threads[name] = (task, cts);
                        break;
                    }
                    case "enter_canceled":
                    {
                        var name = (string)step["name"]!;
                        using var cts = new CancellationTokenSource();
                        cts.Cancel();
                        try
                        {
                            using var lease = gate.Enter(W516aPriority(step["priority"]), cts.Token);
                            lock (order) order.Add(name);
                            Record(name, "granted");
                        }
                        catch (Exception ex)
                        {
                            Record(name, W516aOutcome(ex));
                        }

                        break;
                    }
                    case "cancel":
                    {
                        var (task, cts) = threads[(string)step["name"]!];
                        cts.Cancel();
                        if (!task.Wait(W516aBound)) throw new TimeoutException($"{c["name"]}: cancelled waiter never ended");
                        break;
                    }
                    case "counters":
                        snapshots.Add(gate.Counters());
                        break;
                    default:
                        throw new InvalidOperationException($"unknown op {op}");
                }
            }

            var beforeRelease = gate.Counters();
            if (holder is not null)
            {
                if ((bool?)holderSpec!["worked"] ?? false) W516aGate.MarkWorked(holder);
                holder.Dispose();
                if ((bool?)holderSpec["release_twice"] ?? false) holder.Dispose();
            }

            if (!Task.WaitAll(threads.Values.Select(t => t.Task).ToArray(), W516aBound)) throw new TimeoutException($"{c["name"]}: waiters never finished");
            foreach (var (_, cts) in threads.Values) cts.Dispose();

            var orderJson = new JsonArray();
            foreach (var n in order) orderJson.Add(n);
            var busyJson = new JsonArray();
            foreach (var m in busyMessages) busyJson.Add(m);
            cases.Add(new JsonObject
            {
                ["name"] = (string)c["name"]!,
                ["outcomes"] = outcomes,
                ["busy_messages"] = busyJson,
                ["order"] = orderJson,
                ["snapshots"] = snapshots,
                ["before_release"] = beforeRelease,
                ["final"] = gate.Counters(),
            });
        }

        return cases;
    }
}
