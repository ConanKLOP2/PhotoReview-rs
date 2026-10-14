using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PhotoReview.Core.Caching;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;
using PhotoReview.Core.Updates;

// W1-08: lru-trace
static partial class Commands
{
    /// <summary>
    /// BoundedLruCache&lt;string,string&gt; (size = value.Length, ordinal keys): one scripted scenario per input.
    /// Ops: ["set",k,v] ["get",k] ["remove",k] ["remove_prefix",p] ["clear"]. After every op the entry count and byte size are recorded.
    /// </summary>
    public static JsonArray LruTrace(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var scenario in inputs["scenarios"]!.AsArray())
        {
            var capacity = scenario!["capacity"]!.GetValue<long>();
            var item = new JsonObject { ["capacity"] = capacity };
            BoundedLruCache<string, string> cache;
            try { cache = new BoundedLruCache<string, string>(capacity, v => v.Length, StringComparer.Ordinal); }
            catch (ArgumentOutOfRangeException)
            {
                item["ctor_error"] = true;
                result.Add(item);
                continue;
            }
            var steps = new JsonArray();
            foreach (var opNode in scenario["ops"]!.AsArray())
            {
                var op = opNode!.AsArray();
                var name = op[0]!.GetValue<string>();
                var step = new JsonObject { ["op"] = op.DeepClone() };
                switch (name)
                {
                    case "set": cache.Set(op[1]!.GetValue<string>(), op[2]!.GetValue<string>()); break;
                    case "get":
                        step["hit"] = cache.TryGet(op[1]!.GetValue<string>(), out var value);
                        step["value"] = value;
                        break;
                    case "remove": step["removed"] = cache.Remove(op[1]!.GetValue<string>()); break;
                    case "remove_prefix":
                        var prefix = op[1]!.GetValue<string>();
                        step["removed"] = cache.RemoveWhere(k => k.StartsWith(prefix, StringComparison.Ordinal));
                        break;
                    case "clear": cache.Clear(); break;
                    default: throw new ArgumentException($"unknown op '{name}'");
                }
                step["count"] = cache.Count;
                step["size"] = cache.CurrentSize;
                steps.Add(step);
            }
            item["steps"] = steps;
            result.Add(item);
        }
        return result;
    }
}
