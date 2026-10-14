using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Core.Instance;

// W2-19 oracle: ForwardedPathProtocol.Encode / TryDecode / IsComplete (including the private IsAcceptablePath, reached through them).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `command switch`):
//     "forwarded-path" => Commands.ForwardedPathCases(inputs),
//
// Case shapes ("op" selects):
//   {"op":"encode","paths":[<string | {"prefix":"C:\\","fill":"a","length":N}>]}
//        -> {"ok":true,"hex":"..."}  |  {"ok":false,"exception":"ArgumentException","param":"paths"}
//   {"op":"decode","text":"<utf8 text>" | "hex":"<bytes>", "exists":"all"|"none"|["paths that exist"]}
//        -> {"ok":bool,"paths":[...],"exists_calls":[...paths pathExists was called with, in order...]}
//   {"op":"complete","text":"..." | "hex":"..."}  -> {"complete":bool}
//   {"op":"constants"} -> Header, MaxMessageBytes, MaxPaths, MaxPathChars
static partial class Commands
{
    private static string FwPath(JsonNode? node)
    {
        if (node is JsonValue) return node.GetValue<string>();
        var o = node!.AsObject();
        var prefix = o["prefix"]?.GetValue<string>() ?? "";
        var fill = o["fill"]!.GetValue<string>();
        var length = o["length"]!.GetValue<int>();
        return prefix + new string(fill[0], length - prefix.Length);
    }

    private static byte[] FwBytes(JsonObject c) =>
        c["hex"] is { } hex ? Convert.FromHexString(hex.GetValue<string>()) : new UTF8Encoding(false).GetBytes(c["text"]!.GetValue<string>());

    public static JsonArray ForwardedPathCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["op"]!.GetValue<string>())
            {
                case "constants":
                    item["header"] = ForwardedPathProtocol.Header;
                    item["max_message_bytes"] = ForwardedPathProtocol.MaxMessageBytes;
                    item["max_paths"] = ForwardedPathProtocol.MaxPaths;
                    item["max_path_chars"] = ForwardedPathProtocol.MaxPathChars;
                    break;
                case "encode":
                {
                    var paths = c["paths"]!.AsArray().Select(FwPath).ToList();
                    item["path_lengths"] = new JsonArray(paths.Select(p => (JsonNode?)p.Length).ToArray());
                    try
                    {
                        var bytes = ForwardedPathProtocol.Encode(paths);
                        item["ok"] = true;
                        item["length"] = bytes.Length;
                        item["hex"] = bytes.Length <= 512 ? Convert.ToHexString(bytes) : null;
                    }
                    catch (ArgumentException ex)
                    {
                        item["ok"] = false;
                        item["exception"] = "ArgumentException";
                        item["param"] = ex.ParamName;
                    }
                    catch (Exception ex)
                    {
                        item["ok"] = false;
                        item["exception"] = ex.GetType().Name;
                    }
                    break;
                }
                case "decode":
                {
                    var bytes = FwBytes(c);
                    var calls = new List<string>();
                    var exists = c["exists"];
                    bool Exists(string p)
                    {
                        calls.Add(p);
                        return exists is JsonValue v ? v.GetValue<string>() == "all" : exists!.AsArray().Any(e => e!.GetValue<string>() == p);
                    }

                    var ok = ForwardedPathProtocol.TryDecode(bytes, Exists, out var paths);
                    item["ok"] = ok;
                    item["paths"] = new JsonArray(paths.Select(p => (JsonNode?)p).ToArray());
                    item["exists_calls"] = new JsonArray(calls.Select(p => (JsonNode?)p).ToArray());
                    break;
                }
                case "complete":
                    item["complete"] = ForwardedPathProtocol.IsComplete(FwBytes(c));
                    break;
                default:
                    throw new ArgumentException("unknown op");
            }

            result.Add(item);
        }

        return result;
    }
}
