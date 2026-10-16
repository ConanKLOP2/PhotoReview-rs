using System.Text.Json.Nodes;
using PhotoReview.Core.Localization;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Raw;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-12c
//     "raw-routing" => Commands.RawRouting(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * Needs PhotoReview.Imaging.Raw.dll AND PhotoReview.Imaging.dll referenced by OracleCs.csproj (same step as W5-01a / W4-01a).
//   * No other oracle file is needed (own tiny helpers, prefix W512c).
//   * `describe` goes through Tr/Localizer. Run the oracle with the English language active (or the default fallback);
//     if Localizer needs initialisation in a console host, the wiring step does it once in Program.cs (same as W4-12c).
//     The Rust test compares `describe` against the en.json template `image.error.rawDisabled` with {fileName} replaced.

// W5-12c: FormatRoutingDecoder.Decode / ReadInfo over two recording decoders and a setting-read counter.
static partial class Commands
{
    private sealed class W512cRecordingDecoder : IImageDecoder
    {
        public List<string> Paths { get; } = [];

        public IDecodedImage Decode(DecodeRequest request)
        {
            Paths.Add(request.Path);
            return null!;
        }

        public ImageInfo ReadInfo(string path)
        {
            Paths.Add(path);
            return new ImageInfo(1, 1, 1);
        }
    }

    /// <summary>
    /// Per case {name, raw_enabled, ops:[{op: decode|read_info|set_enabled, path?, value?}]}.
    /// Reports per op: routed ("standard"|"raw"|null), throws (exception type name), is_localized, message, describe,
    /// enabled_calls (running count of isRawEnabled invocations AFTER the op). For set_enabled: value.
    /// </summary>
    public static JsonArray RawRouting(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var enabled = c["raw_enabled"]!.GetValue<bool>();
            var enabledCalls = 0;
            var standard = new W512cRecordingDecoder();
            var raw = new W512cRecordingDecoder();
            var router = new FormatRoutingDecoder(standard, raw, () => { enabledCalls++; return enabled; });

            var ops = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                var name = op["op"]!.GetValue<string>();
                var path = op["path"]?.GetValue<string>();
                var entry = new JsonObject { ["op"] = name, ["path"] = path };
                if (name == "set_enabled")
                {
                    enabled = op["value"]!.GetValue<bool>();
                    entry["value"] = enabled;
                }
                else
                {
                    var standardBefore = standard.Paths.Count;
                    var rawBefore = raw.Paths.Count;
                    try
                    {
                        if (name == "decode") router.Decode(new DecodeRequest(path!, TargetWidth: 0));
                        else router.ReadInfo(path!);
                        entry["routed"] = standard.Paths.Count > standardBefore ? "standard" : raw.Paths.Count > rawBefore ? "raw" : null;
                    }
                    catch (Exception ex)
                    {
                        entry["throws"] = ex.GetType().Name;
                        entry["is_localized"] = UserFacingError.IsLocalized(ex);
                        entry["message"] = ex.Message;
                        entry["describe"] = UserFacingError.Describe(ex);
                    }
                }
                entry["enabled_calls"] = enabledCalls;
                ops.Add(entry);
            }

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["ops"] = ops,
                ["standard_paths"] = new JsonArray(standard.Paths.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
                ["raw_paths"] = new JsonArray(raw.Paths.Select(p => (JsonNode?)JsonValue.Create(p)).ToArray()),
            });
        }
        return result;
    }
}
