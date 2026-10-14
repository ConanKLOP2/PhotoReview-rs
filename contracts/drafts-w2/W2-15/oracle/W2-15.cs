using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using PhotoReview.Core.IO;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-15
//     "navigation-stat-worker" => Commands.NavigationStatWorkerCases(inputs),
// Needs W2-00.cs (class W2Util).

// W2-15: NavigationStatWorker.RunAsync. Only deterministic observables are reported (no thread ids, no timings):
// completion status, value / exception, which items ran and in which order, IsCanceled at the moment RunAsync returned.
static partial class Commands
{
    /// <summary>
    /// Case: { name, busy_first: bool, items: [ { id, kind: "value"|"throw", value: int, token: "none"|"cancelled-before"|"cancel-while-queued" } ] }.
    /// With busy_first the worker is held on a gate item while every case item is queued (and the cancel-while-queued ones
    /// are cancelled), then the gate is released. Items are queued in the listed order.
    /// </summary>
    public static JsonArray NavigationStatWorkerCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var worker = new NavigationStatWorker();
            var timeout = TimeSpan.FromSeconds(20);
            var ran = new ConcurrentQueue<string>();
            var threads = new ConcurrentBag<int>();
            using var gate = new ManualResetEventSlim(false);
            using var started = new ManualResetEventSlim(false);
            Task<int>? busy = null;
            var busyFirst = c["busy_first"]!.GetValue<bool>();
            if (busyFirst)
            {
                busy = worker.RunAsync(() =>
                {
                    started.Set();
                    gate.Wait(timeout);
                    return -1;
                });
                if (!started.Wait(timeout)) throw new TimeoutException("worker never started");
            }

            var items = c["items"]!.AsArray();
            var ctsList = new List<CancellationTokenSource>();
            var queued = new List<(string Id, Task<int> Task, bool CanceledAtReturn)>();
            foreach (var itemNode in items)
            {
                var item = itemNode!.AsObject();
                var id = item["id"]!.GetValue<string>();
                var kind = item["kind"]!.GetValue<string>();
                var value = item["value"]?.GetValue<int>() ?? 0;
                var cts = new CancellationTokenSource();
                ctsList.Add(cts);
                if (item["token"]!.GetValue<string>() == "cancelled-before") cts.Cancel();
                var task = worker.RunAsync(() =>
                {
                    ran.Enqueue(id);
                    threads.Add(Environment.CurrentManagedThreadId);
                    if (kind == "throw") throw new InvalidOperationException("boom-" + id);
                    return value;
                }, cts.Token);
                queued.Add((id, task, task.IsCanceled));
            }
            for (var i = 0; i < items.Count; i++)
            {
                if (items[i]!["token"]!.GetValue<string>() == "cancel-while-queued") ctsList[i].Cancel();
            }
            gate.Set();

            var outcomes = new JsonArray();
            foreach (var (id, task, canceledAtReturn) in queued)
            {
                string status;
                int? value = null;
                string? exceptionType = null, message = null;
                try
                {
                    value = task.WaitAsync(timeout).GetAwaiter().GetResult();
                    status = "value";
                }
                catch (OperationCanceledException)
                {
                    status = "canceled";
                }
                catch (Exception ex)
                {
                    status = "faulted";
                    exceptionType = ex.GetType().Name;
                    message = ex.Message;
                }
                outcomes.Add(new JsonObject
                {
                    ["id"] = id,
                    ["status"] = status,
                    ["value"] = value,
                    ["exception_type"] = exceptionType,
                    ["message"] = message,
                    ["canceled_at_return"] = canceledAtReturn,
                });
            }
            busy?.WaitAsync(timeout).GetAwaiter().GetResult();
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["busy_first"] = busyFirst,
                ["outcomes"] = outcomes,
                ["ran_order"] = W2Util.StrArray(ran.ToArray()),
                ["single_worker_thread"] = threads.Distinct().Count() <= 1 && !threads.Contains(Environment.CurrentManagedThreadId),
            });
        }
        return result;
    }
}
