using System.Text.Json.Nodes;
using PhotoReview.Imaging.Metadata;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-03a
//     "tiff-structure" => Commands.TiffStructureCases(inputs),
// Needs W2-00.cs (class W2Util). OracleCs.csproj needs a second <Reference Include="PhotoReview.Imaging"> (HintPath
// $(CSharpCoreDir)\PhotoReview.Imaging.dll): TiffStructure and ExifRational are in PhotoReview.Imaging, not Core;
// tools/oracle.ps1 must copy that dll next to PhotoReview.Core.dll. Both types are public, no InternalsVisibleTo needed.

// W4-03a: TiffStructure. One command, one case per entry. Case = { name, op, ... }; "data" nodes are
//   { "hex": "49492a00" }                                       exact bytes, or
//   { "len": N, "patches": [ { "at": offset, "hex": ".." } ] }  N zero bytes with the patches written over them.
// ops (fields -> reported):
//   header       data                                            -> { ok, little, magic, ifd0 } (the last three null when !ok)
//   type-size    type                                            -> { size }
//   u16 | u32    data, offset (int), little                      -> { value }
//   value-span   data, entry_offset (int), type, count (uint), little -> { ok, length, hex } (null when !ok)
//   ascii        data, type                                      -> { text } (null | string)
//   unsigned     data, type, little                              -> { value } (null | long)
//   rational     data, type, little                              -> { value } (null | { n, d })
static partial class Commands
{
    private static byte[] W403Data(JsonNode? node)
    {
        var o = node!.AsObject();
        if (o["hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        var bytes = new byte[o["len"]!.GetValue<int>()];
        if (o["patches"] is { } patches)
        {
            foreach (var p in patches.AsArray())
            {
                var po = p!.AsObject();
                Convert.FromHexString(po["hex"]!.GetValue<string>()).CopyTo(bytes, po["at"]!.GetValue<int>());
            }
        }
        return bytes;
    }

    public static JsonArray TiffStructureCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op };
            bool little = c["little"]?.GetValue<bool>() ?? true;
            switch (op)
            {
                case "header":
                {
                    var ok = TiffStructure.TryReadHeader(W403Data(c["data"]), out var le, out var magic, out var ifd0);
                    item["ok"] = ok;
                    item["little"] = ok ? le : null;
                    item["magic"] = ok ? magic : null;
                    item["ifd0"] = ok ? ifd0 : null;
                    break;
                }
                case "type-size":
                    item["size"] = TiffStructure.TypeSize((ushort)c["type"]!.GetValue<int>());
                    break;
                case "u16":
                    item["value"] = TiffStructure.ReadU16(W403Data(c["data"]), c["offset"]!.GetValue<int>(), little);
                    break;
                case "u32":
                    item["value"] = TiffStructure.ReadU32(W403Data(c["data"]), c["offset"]!.GetValue<int>(), little);
                    break;
                case "value-span":
                {
                    var ok = TiffStructure.TryGetValueSpan(W403Data(c["data"]), c["entry_offset"]!.GetValue<int>(),
                        (ushort)c["type"]!.GetValue<int>(), c["count"]!.GetValue<uint>(), little, out var span);
                    item["ok"] = ok;
                    item["length"] = ok ? span.Length : null;
                    item["hex"] = ok ? Convert.ToHexString(span) : null;
                    break;
                }
                case "ascii":
                    item["text"] = TiffStructure.ReadAscii(W403Data(c["data"]), (ushort)c["type"]!.GetValue<int>());
                    break;
                case "unsigned":
                    item["value"] = TiffStructure.ReadUnsigned(W403Data(c["data"]), (ushort)c["type"]!.GetValue<int>(), little);
                    break;
                case "rational":
                {
                    var r = TiffStructure.ReadRational(W403Data(c["data"]), (ushort)c["type"]!.GetValue<int>(), little);
                    item["value"] = r is { } v ? new JsonObject { ["n"] = v.Numerator, ["d"] = v.Denominator } : null;
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + op);
            }
            result.Add(item);
        }
        return result;
    }
}
