using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;
using PhotoReview.Imaging.Raw.Bmff;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-10a
//     "bmff-navigator" => Commands.BmffNavigatorCases(inputs),
// Needs nothing else from the other oracle files. OracleCs.csproj needs the <Reference Include="PhotoReview.Imaging.Raw"> that W5-02a adds
// (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll). Every type used here is public (BmffBoxNavigator, its nested BmffBox,
// IRawHeaderSource, RawContainerLimits): no InternalsVisibleTo.

// W5-10a: BmffBoxNavigator. One command, one case per entry. Case = { name, op, data, ... } with data = { "hex": "..." } (the whole file).
//   Common optional fields: "fail": "io" | "data" with "fail_after" (int, default 0): the first fail_after Read calls succeed, every later one
//   throws InvalidDataException (with an IOException inner for "io"); "short_at" (long): the Read at exactly that offset returns one byte
//   fewer than asked (a source that breaks the interface contract; BmffBoxNavigator must answer false, not throw).
//   Every result carries "error": null | "io" | "data" | "argument" (the exception that escaped; the result fields are then absent).
//   A box is reported as { type, offset, total_size, payload_offset, payload_size, uuid } (uuid = lower-case hex or null).
//   Numbers that may not fit a double ("container_end", the "parent" fields) are JSON numbers or decimal strings; parsed with long.Parse.
// ops (fields -> reported):
//   try-read-box   offset (long), container_end? (long, absent = the -1 default)      -> { ok, box }          (box null when !ok)
//   child-boxes    parent_at (long: TryReadBox there gives the parent, must succeed)
//                  | parent { type, offset, total_size, payload_offset, payload_size } (built directly, may be hostile),
//                  payload_skip? (int, default 0)                                      -> { count, children:[box] }
//   walk           (the BinaryReaderFuzzTests walker: TryReadBox loop from 0 to source.Length with containerEnd = Length and at most
//                  MaxChildBoxes iterations; boxes of type moov/trak/mdia/minf/stbl/uuid/meta get ReadChildBoxes; the children are
//                  listed but not walked further: the C# walker never recurses past them (its depth < 5 guard is never exceeded))
//                                                                                      -> { any, visited, boxes:[{depth, ...box}] }
//                  `depth` is 0 for the top-level boxes and 1 for the children of a container box. The Rust walker is written in the
//                  integration test and must produce the same list.
static partial class Commands
{
    private sealed class W510aSource(byte[] data, string? fail, int failAfter, long shortAt) : IRawHeaderSource
    {
        private int _reads;

        public long Length => data.Length;

        public ReadOnlySpan<byte> Read(long offset, int count)
        {
            if (fail is not null && _reads++ >= failAfter)
            {
                throw fail == "io"
                    ? new InvalidDataException("read failed", new IOException("disk"))
                    : new InvalidDataException("budget exhausted");
            }

            if (offset < 0 || count < 0) throw new ArgumentOutOfRangeException(nameof(offset));
            if (offset > data.Length - count) throw new InvalidDataException("past end");
            if (count > RawContainerLimits.MaxHeaderBytes) throw new InvalidDataException("over the hard limit");
            return data.AsSpan((int)offset, offset == shortAt ? count - 1 : count);
        }
    }

    private static long W510aLong(JsonNode? node) =>
        node!.GetValueKind() == System.Text.Json.JsonValueKind.String
            ? long.Parse(node.GetValue<string>(), CultureInfo.InvariantCulture)
            : node.GetValue<long>();

    private static JsonNode? W510aBox(BmffBoxNavigator.BmffBox b) => new JsonObject
    {
        ["type"] = b.Type,
        ["offset"] = b.Offset,
        ["total_size"] = b.TotalSize,
        ["payload_offset"] = b.PayloadOffset,
        ["payload_size"] = b.PayloadSize,
        ["uuid"] = b.Uuid is null ? null : Convert.ToHexString(b.Uuid).ToLowerInvariant(),
    };

    private static string W510aError(Exception e) => e switch
    {
        InvalidDataException { InnerException: not null } => "io",
        InvalidDataException => "data",
        ArgumentOutOfRangeException => "argument",
        _ => throw e,
    };

    private static readonly HashSet<string> W510aContainers = ["moov", "trak", "mdia", "minf", "stbl", "uuid", "meta"];

    public static JsonArray BmffNavigatorCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op, ["error"] = null };
            var data = Convert.FromHexString(c["data"]!["hex"]!.GetValue<string>());
            var source = new W510aSource(
                data, c["fail"]?.GetValue<string>(), c["fail_after"]?.GetValue<int>() ?? 0, c["short_at"] is { } sa ? W510aLong(sa) : -1);
            try
            {
                switch (op)
                {
                    case "try-read-box":
                    {
                        long containerEnd = c["container_end"] is { } ce ? W510aLong(ce) : -1;
                        bool ok = BmffBoxNavigator.TryReadBox(source, c["offset"]!.GetValue<long>(), out var box, containerEnd);
                        item["ok"] = ok;
                        item["box"] = ok ? W510aBox(box) : null;
                        break;
                    }
                    case "child-boxes":
                    {
                        BmffBoxNavigator.BmffBox parent;
                        if (c["parent_at"] is { } at)
                        {
                            if (!BmffBoxNavigator.TryReadBox(source, W510aLong(at), out parent))
                                throw new InvalidOperationException("parent_at does not parse in " + item["name"]);
                        }
                        else
                        {
                            var p = c["parent"]!.AsObject();
                            parent = new BmffBoxNavigator.BmffBox(
                                p["type"]!.GetValue<string>(), W510aLong(p["offset"]), W510aLong(p["total_size"]),
                                W510aLong(p["payload_offset"]), W510aLong(p["payload_size"]));
                        }

                        var children = BmffBoxNavigator.ReadChildBoxes(source, parent, c["payload_skip"]?.GetValue<int>() ?? 0);
                        item["count"] = children.Count;
                        item["children"] = new JsonArray(children.Select(W510aBox).ToArray());
                        break;
                    }
                    case "walk":
                    {
                        var boxes = new JsonArray();
                        int visited = 0;
                        bool any = false;
                        long offset = 0, end = source.Length;
                        for (int i = 0; i < BmffBoxNavigator.MaxChildBoxes && offset < end; i++)
                        {
                            if (!BmffBoxNavigator.TryReadBox(source, offset, out var box, end)) break;
                            any = true;
                            boxes.Add(W510aWalkBox(0, box));
                            if (++visited > 4_096 * 8) break;
                            if (W510aContainers.Contains(box.Type))
                            {
                                foreach (var child in BmffBoxNavigator.ReadChildBoxes(source, box))
                                {
                                    boxes.Add(W510aWalkBox(1, child));
                                    visited++;
                                }
                            }

                            offset = box.Offset + box.TotalSize;
                        }

                        item["any"] = any;
                        item["visited"] = visited;
                        item["boxes"] = boxes;
                        break;
                    }
                    default:
                        throw new ArgumentException("unknown op " + op);
                }
            }
            catch (Exception e) when (e is InvalidDataException or ArgumentOutOfRangeException)
            {
                item["error"] = W510aError(e);
            }

            result.Add(item);
        }

        return result;
    }

    private static JsonNode? W510aWalkBox(int depth, BmffBoxNavigator.BmffBox b)
    {
        var o = W510aBox(b)!.AsObject();
        var withDepth = new JsonObject { ["depth"] = depth };
        foreach (var kv in o) withDepth[kv.Key] = kv.Value?.DeepClone();
        return withDepth;
    }
}
