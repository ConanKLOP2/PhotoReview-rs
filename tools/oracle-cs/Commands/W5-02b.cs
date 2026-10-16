using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-02b
//     "jpeg-marker-probe" => Commands.JpegMarkerProbeCases(inputs),
// Wiring: tools/oracle-cs/OracleCs.csproj needs one more reference next to PhotoReview.Imaging:
//     <Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>
// and tools/oracle.ps1 must copy PhotoReview.Imaging.Raw.dll into the baseline build folder (the same step that copies the others).
// JpegMarkerProbe is `internal` (InternalsVisibleTo only lists test projects), so it is called by reflection.

// W5-02b: JpegMarkerProbe.TryReadLossyFrame over synthetic byte ranges.
static partial class Commands
{
    private const int W502bBlockSize = 64 * 1024;
    private const int W502bMaxHeaderBytes = 8 << 20;

    /// <summary>Same budget semantics as SourceRawHeaderSource / InMemoryRawHeaderSource: 64 KB blocks charged once each.</summary>
    private sealed class W502bMemorySource(byte[] bytes) : IRawHeaderSource
    {
        private readonly HashSet<long> _touched = [];
        private long _total;
        public long Length => bytes.Length;

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            if (offset < 0 || count < 0) throw new ArgumentOutOfRangeException(nameof(offset));
            if (offset > bytes.Length - count) throw new InvalidDataException("past end");
            if (count > W502bMaxHeaderBytes) throw new InvalidDataException("too large");
            if (count == 0) return ReadOnlySpan<byte>.Empty;
            for (long block = offset / W502bBlockSize; block <= (offset + count - 1) / W502bBlockSize; block++)
            {
                if (_touched.Contains(block)) continue;
                long blockBytes = Math.Min(W502bBlockSize, bytes.Length - (block * W502bBlockSize));
                if (blockBytes > W502bMaxHeaderBytes - _total) throw new InvalidDataException("budget exhausted");
                _touched.Add(block);
                _total += blockBytes;
            }
            return bytes.AsSpan((int)offset, count);
        }
    }

    private sealed class W502bThrowingSource(Exception exception) : IRawHeaderSource
    {
        public long Length => 64;
        public ReadOnlySpan<byte> Read(long offset, int count) => throw exception;
    }

    private static byte[] W502bBuild(JsonArray parts)
    {
        var all = new List<byte>();
        foreach (var node in parts)
        {
            var p = node!.AsObject();
            var unit = Convert.FromHexString(p["hex"]!.GetValue<string>()).ToList();
            if (p["pad_to"] is { } pad) while (unit.Count < pad.GetValue<int>()) unit.Add(0);
            int repeat = p["repeat"]?.GetValue<int>() ?? 1;
            for (int i = 0; i < repeat; i++) all.AddRange(unit);
        }
        return [.. all];
    }

    private static long W502bLong(JsonNode node) =>
        node is JsonValue v && v.TryGetValue<string>(out var s) ? long.Parse(s, System.Globalization.CultureInfo.InvariantCulture) : node.GetValue<long>();

    /// <summary>
    /// Per case {name, source: memory|throw-invalid|throw-io, parts:[{hex, pad_to?, repeat?}], offset, length?, length_delta?}:
    /// length defaults to (total - offset + length_delta); a string length is parsed as int64.
    /// Result adds total_length, ok, width/height (only when ok), io_failure (the call threw InvalidDataException with an inner IOException).
    /// </summary>
    public static JsonArray JpegMarkerProbeCases(JsonObject inputs)
    {
        var probe = typeof(IRawHeaderSource).Assembly.GetType("PhotoReview.Imaging.Raw.Tiff.JpegMarkerProbe", throwOnError: true)!;
        var method = probe.GetMethod("TryReadLossyFrame", BindingFlags.Public | BindingFlags.Static)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var bytes = W502bBuild(c["parts"]!.AsArray());
            long offset = W502bLong(c["offset"]!);
            long length = c["length"] is { } l ? W502bLong(l) : bytes.Length - offset + (c["length_delta"]?.GetValue<long>() ?? 0);
            IRawHeaderSource source = c["source"]!.GetValue<string>() switch
            {
                "memory" => new W502bMemorySource(bytes),
                "throw-invalid" => new W502bThrowingSource(new InvalidDataException("budget exhausted")),
                "throw-io" => new W502bThrowingSource(new InvalidDataException("read failed", new IOException("disk"))),
                var other => throw new ArgumentException("unknown source " + other),
            };

            bool ok = false;
            bool ioFailure = false;
            int width = 0, height = 0;
            try
            {
                var args = new object?[] { source, offset, length, 0, 0 };
                ok = (bool)method.Invoke(null, args)!;
                width = (int)args[3]!;
                height = (int)args[4]!;
            }
            catch (TargetInvocationException ex) when (ex.InnerException is InvalidDataException { InnerException: IOException })
            {
                ioFailure = true;
            }

            var row = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["source"] = c["source"]!.GetValue<string>(),
                ["parts"] = c["parts"]!.DeepClone(),
                ["total_length"] = bytes.Length,
                ["offset"] = offset,
                ["length"] = length.ToString(System.Globalization.CultureInfo.InvariantCulture),
                ["ok"] = ok,
                ["io_failure"] = ioFailure,
            };
            if (ok)
            {
                row["width"] = width;
                row["height"] = height;
            }
            result.Add(row);
        }
        return result;
    }
}
