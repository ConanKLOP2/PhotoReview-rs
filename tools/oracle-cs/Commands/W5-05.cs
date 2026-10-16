using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Tiff;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-05
//     "orf-container" => Commands.OrfContainerCases(inputs),
// Wiring needed (the lead does it with the other W5 oracles):
//   * OracleCs.csproj: a third <Reference Include="PhotoReview.Imaging.Raw"> (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll).
//     OrfContainerReader, SourceRawHeaderSource, RawContainerInfo, EmbeddedPreview, ExifBlock are all public: no InternalsVisibleTo.
//   * tools/oracle.ps1: also `dotnet build src\PhotoReview.Imaging.Raw\PhotoReview.Imaging.Raw.csproj -c Release -o $coreDir` and add
//     PhotoReview.Imaging.Raw.dll to the "rebuild if missing" check.
// Needs nothing from other oracle files. The corpus is read from tests/fixtures/raw-corpus (PerfCommands.FixturesDir + "raw-corpus").

// W5-05: OrfContainerReader. One command, one case per entry. Case = { name, op, ... }; "data" nodes are
//   { "hex": "49492a00..." }                                              exact bytes, or
//   { "len": N, "patches": [ { "at": offset, "hex": "..", "times": k? } ] }  N zero bytes, patches written in order, "times" repeats the hex.
// ops (fields -> reported):
//   can-read   data, ext                                -> { can_read }                       CanRead(first min(64, len) bytes, ext)
//   read       data                                     -> { can_read, ok, error, message, format, sensor_width, sensor_height,
//                                                            orientation, previews[], exif_blocks[] }
//   corpus     file (name inside tests/fixtures/raw-corpus) -> { present } and, when present, the same fields as read plus file_length
// The "read" and "corpus" ops read through SourceRawHeaderSource over a FileStream, the production header source (64 KiB blocks and the
// 8 MiB header budget are part of what is pinned: see the case "preview-jpeg-exhausting-the-header-budget-keeps-the-declared-size").
// "error" is the exception type name (expected: InvalidDataException only); "message" is reported only for the two fixed ORF texts.
static partial class Commands
{
    private static byte[] W505Data(JsonNode? node)
    {
        var o = node!.AsObject();
        if (o["hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        var bytes = new byte[o["len"]!.GetValue<int>()];
        if (o["patches"] is { } patches)
        {
            foreach (var p in patches.AsArray())
            {
                var po = p!.AsObject();
                var unit = Convert.FromHexString(po["hex"]!.GetValue<string>());
                int times = po["times"]?.GetValue<int>() ?? 1;
                int at = po["at"]!.GetValue<int>();
                for (int k = 0; k < times; k++) unit.CopyTo(bytes, at + (k * unit.Length));
            }
        }
        return bytes;
    }

    private static bool W505CanRead(byte[] data, string ext) =>
        new OrfContainerReader().CanRead(data.AsSpan(0, Math.Min(64, data.Length)), ext);

    private static void W505Read(string path, byte[] data, JsonObject item)
    {
        item["can_read"] = W505CanRead(data, ".orf");
        try
        {
            using var fs = File.OpenRead(path);
            using var source = new SourceRawHeaderSource(fs);
            var info = new OrfContainerReader().Read(source, CancellationToken.None);
            item["ok"] = true;
            item["error"] = null;
            item["message"] = null;
            item["format"] = info.Format.ToString();
            item["sensor_width"] = info.SensorWidth;
            item["sensor_height"] = info.SensorHeight;
            item["orientation"] = info.Orientation;
            item["previews"] = new JsonArray(info.Previews.Select(p => (JsonNode?)new JsonObject
            {
                ["index"] = p.Index,
                ["offset"] = p.Offset,
                ["length"] = p.Length,
                ["kind"] = p.Kind.ToString(),
                ["width"] = p.Width,
                ["height"] = p.Height,
                ["color_space"] = p.ColorSpace.ToString(),
                ["header_resolved"] = p.HeaderResolved,
            }).ToArray());
            item["exif_blocks"] = new JsonArray(info.ExifBlocks.Select(b => (JsonNode?)new JsonObject
            {
                ["offset"] = b.Offset,
                ["length"] = b.Length,
                ["is_tiff_header"] = b.IsTiffHeader,
                ["ifd_is_exif"] = b.IfdIsExif,
            }).ToArray());
        }
        catch (Exception ex)
        {
            item["ok"] = false;
            item["error"] = ex.GetType().Name;
            item["message"] = ex.Message is "File too short for ORF container." or "Invalid ORF header." ? ex.Message : null;
        }
    }

    public static JsonArray OrfContainerCases(JsonObject inputs)
    {
        var result = new JsonArray();
        var root = Path.Combine(Path.GetTempPath(), "pr-oracle-orf-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var op = c["op"]!.GetValue<string>();
                var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op };
                switch (op)
                {
                    case "can-read":
                        item["ext"] = c["ext"]!.GetValue<string>();
                        item["can_read"] = W505CanRead(W505Data(c["data"]), c["ext"]!.GetValue<string>());
                        break;
                    case "read":
                    {
                        var data = W505Data(c["data"]);
                        var path = Path.Combine(root, "case.orf");
                        File.WriteAllBytes(path, data);
                        W505Read(path, data, item);
                        break;
                    }
                    case "corpus":
                    {
                        var file = c["file"]!.GetValue<string>();
                        var path = Path.GetFullPath(Path.Combine(PerfCommands.FixturesDir, "raw-corpus", file));
                        item["file"] = file;
                        item["present"] = File.Exists(path);
                        if (File.Exists(path))
                        {
                            item["file_length"] = new FileInfo(path).Length;
                            // The reader is run on the corpus file itself (not a copy) so the extension logic of the registry is not involved.
                            using var head = File.OpenRead(path);
                            var first = new byte[64];
                            int n = head.Read(first, 0, first.Length);
                            W505Read(path, first.AsSpan(0, n).ToArray(), item);
                        }
                        break;
                    }
                    default:
                        throw new ArgumentException("unknown op " + op);
                }
                result.Add(item);
            }
        }
        finally
        {
            try { Directory.Delete(root, recursive: true); } catch { }
        }
        return result;
    }
}
