using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Catalog;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Preload;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-06b
//     "preload-backoff-script" => Commands.PreloadBackoffScript(inputs),
//     "preview-size-sampler-script" => Commands.PreviewSizeSamplerScript(inputs),
// WIRING: needs the PhotoReview.Imaging.dll reference (see the W4-05a header). PreloadBusyBackoff is `internal` (ctor and
// methods too) and the C# repo is never edited, so this file reaches it by REFLECTION (BindingFlags.NonPublic); no
// InternalsVisibleTo is needed. PreviewSizeSampler is public. Both are pure (no I/O, clock or culture): deterministic (D19/D27).

// W4-06b: PreloadBusyBackoff / PreviewSizeSampler driven by an operation script.
static partial class Commands
{
    private static readonly DateTime W406bDefaultWritten = new(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc);
    private const BindingFlags W406bFlags = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    private static ImageCacheKey W406bKey(JsonObject op)
    {
        var written = op["written"] is { } w
            ? DateTime.Parse(w.GetValue<string>(), CultureInfo.InvariantCulture, DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal)
            : W406bDefaultWritten;
        var box = new DecodeBox(op["box_width"]?.GetValue<int>() ?? 100, op["box_height"]?.GetValue<int>() ?? 100);
        var path = op["path"]!.GetValue<string>();
        return ImageCacheKey.Create(new CatalogEntry(path).WithMetadata(op["length"]!.GetValue<long>(), written), false, box);
    }

    /// <summary>
    /// Per case {name, ops:[{op, path?, length?, written?, box_width?, box_height?, times?}]}; ops are begin_pass (x times),
    /// note_busy, should_skip, record_success, clear. Result {name, results:[...], pass}: one entry per op (null for
    /// begin_pass/record_success/clear, the count for note_busy, a bool for should_skip) and the final private pass counter.
    /// </summary>
    public static JsonArray PreloadBackoffScript(JsonObject inputs)
    {
        var type = typeof(PreviewSizeSampler).Assembly.GetType("PhotoReview.Imaging.Preload.PreloadBusyBackoff", throwOnError: true)!;
        var beginPass = type.GetMethod("BeginPass", W406bFlags)!;
        var noteBusy = type.GetMethod("NoteBusy", W406bFlags)!;
        var shouldSkip = type.GetMethod("ShouldSkip", W406bFlags)!;
        var recordSuccess = type.GetMethod("RecordSuccess", W406bFlags)!;
        var clear = type.GetMethod("Clear", W406bFlags)!;
        var passField = type.GetField("_pass", W406bFlags)!;

        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var backoff = Activator.CreateInstance(type, nonPublic: true)!;
            var results = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                switch (op["op"]!.GetValue<string>())
                {
                    case "begin_pass":
                        for (var i = 0; i < (op["times"]?.GetValue<int>() ?? 1); i++) beginPass.Invoke(backoff, null);
                        results.Add(null);
                        break;
                    case "note_busy":
                        results.Add((int)noteBusy.Invoke(backoff, [op["path"]!.GetValue<string>(), W406bKey(op)])!);
                        break;
                    case "should_skip":
                        results.Add((bool)shouldSkip.Invoke(backoff, [op["path"]!.GetValue<string>(), W406bKey(op)])!);
                        break;
                    case "record_success":
                        recordSuccess.Invoke(backoff, [op["path"]!.GetValue<string>()]);
                        results.Add(null);
                        break;
                    case "clear":
                        clear.Invoke(backoff, null);
                        results.Add(null);
                        break;
                    default:
                        throw new ArgumentException("unknown op " + op["op"]);
                }
            }

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["results"] = results,
                ["pass"] = (int)passField.GetValue(backoff)!,
            });
        }

        return result;
    }

    /// <summary>
    /// Per case {name, ops:[{op: record|mean, box_width, box_height, bytes?, times?}]}. Result {name, results:[...]}:
    /// null for record, the mean (double) or null for mean.
    /// </summary>
    public static JsonArray PreviewSizeSamplerScript(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var sampler = new PreviewSizeSampler();
            var results = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                var box = new DecodeBox(op["box_width"]!.GetValue<int>(), op["box_height"]!.GetValue<int>());
                switch (op["op"]!.GetValue<string>())
                {
                    case "record":
                        for (var i = 0; i < (op["times"]?.GetValue<int>() ?? 1); i++) sampler.Record(box, op["bytes"]!.GetValue<long>());
                        results.Add(null);
                        break;
                    case "mean":
                        results.Add(sampler.MeanBytes(box) is { } mean ? JsonValue.Create(mean) : null);
                        break;
                    default:
                        throw new ArgumentException("unknown op " + op["op"]);
                }
            }

            result.Add(new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["results"] = results });
        }

        return result;
    }
}
