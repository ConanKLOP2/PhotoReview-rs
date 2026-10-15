using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Localization;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Decoding.Wic;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-12c
//     "webp-heic-routing" => Commands.WebpHeicRouting(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * Needs PhotoReview.Imaging.dll referenced by OracleCs.csproj (same step as W4-01a/W4-01c).
//   * No other oracle file is needed (no W2Util: this file has its own tiny helpers).
//   * `describe` goes through Tr/Localizer. Run the oracle with the English language active (or the default fallback);
//     if Localizer needs initialisation in a console host, the wiring step does it once in Program.cs. The Rust test
//     compares `describe` against the en.json templates with {fileName} replaced.

// W4-12c: WebpHeicRoutingDecoder.Decode / ReadInfo / WillRefuse over recording decoders and a probe counter.
static partial class Commands
{
    private sealed class W412cRecordingDecoder : IImageDecoder
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

    private sealed class W412cLog : ILog
    {
        public List<string> Warnings { get; } = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) => Warnings.Add(message);
        public void Error(string message, Exception? ex = null) { }
    }

    private static string? W412cStr(JsonNode? n) => n is null ? null : n.GetValue<string>();

    /// <summary>
    /// Per case {name, enabled, codecs:{webp, heif_container, hevc, detail}, ops:[{op: decode|read_info|will_refuse|set_enabled, path?, value?}]}.
    /// Reports per op: routed ("inner"|"wic"|null), throws (type name), is_missing_codec, is_localized, message, describe,
    /// value (will_refuse), probe_calls (running count AFTER the op); and the case's final warnings list.
    /// </summary>
    public static JsonArray WebpHeicRouting(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var enabled = c["enabled"]!.GetValue<bool>();
            var k = c["codecs"]!.AsObject();
            var codecs = new WicCodecSupport(
                k["webp"]!.GetValue<bool>(), k["heif_container"]!.GetValue<bool>(), k["hevc"]!.GetValue<bool>(), k["detail"]!.GetValue<string>());
            var probeCalls = 0;
            var inner = new W412cRecordingDecoder();
            var wic = new W412cRecordingDecoder();
            var log = new W412cLog();
            var router = new WebpHeicRoutingDecoder(inner, wic, () => enabled, () => { probeCalls++; return codecs; }, log);

            var ops = new JsonArray();
            foreach (var opNode in c["ops"]!.AsArray())
            {
                var op = opNode!.AsObject();
                var name = op["op"]!.GetValue<string>();
                var path = W412cStr(op["path"]);
                var entry = new JsonObject { ["op"] = name, ["path"] = path };
                if (name == "set_enabled")
                {
                    enabled = op["value"]!.GetValue<bool>();
                    entry["value"] = enabled;
                }
                else if (name == "will_refuse")
                {
                    entry["value"] = WebpHeicRoutingDecoder.WillRefuse(path, () => enabled, () => { probeCalls++; return codecs; });
                }
                else
                {
                    var innerBefore = inner.Paths.Count;
                    var wicBefore = wic.Paths.Count;
                    try
                    {
                        if (name == "decode") router.Decode(new DecodeRequest(path!, TargetWidth: 0));
                        else router.ReadInfo(path!);
                        entry["routed"] = inner.Paths.Count > innerBefore ? "inner" : wic.Paths.Count > wicBefore ? "wic" : null;
                    }
                    catch (Exception ex)
                    {
                        entry["throws"] = ex.GetType().Name;
                        entry["is_missing_codec"] = ex is MissingImageCodecException;
                        entry["is_localized"] = UserFacingError.IsLocalized(ex);
                        entry["message"] = ex.Message;
                        entry["describe"] = UserFacingError.Describe(ex);
                    }
                }
                entry["probe_calls"] = probeCalls;
                ops.Add(entry);
            }

            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["ops"] = ops,
                ["warnings"] = new JsonArray(log.Warnings.Select(w => (JsonNode?)JsonValue.Create(w)).ToArray()),
            });
        }
        return result;
    }
}
