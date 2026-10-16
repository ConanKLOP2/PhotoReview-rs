using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Pixels;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-14a
//     "pixel-area-resample" => Commands.PixelAreaResample(inputs),
// Wiring needs (the wiring step, not this draft): OracleCs.csproj gets a <Reference Include="PhotoReview.Imaging.TurboJpeg"> to the
// baseline PhotoReview.Imaging.TurboJpeg.dll (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.TurboJpeg.dll), copied next to the other
// baseline DLLs by oracle.ps1. PixelAreaResampler is `internal` (InternalsVisibleTo covers only PhotoReview.Imaging.Tests), so it is
// reached by reflection below; PixelBuffer/PixelLayout are public (PhotoReview.Imaging, already referenced since W4-02a).
// The class is pure managed code: loading the assembly does NOT need libturbojpeg.

// W5-14a: PixelAreaResampler.Resize. One command; one case per entry:
//   { name, sw, sh, tw, th,
//     layout?: "Bgr32" (default) | "Pbgra32",
//     src: { kind: "formula", a, b, k, m, seed }   byte(x,y,c) = (x*a + y*b + c*k + x*y*m + seed) mod 256 (c: 0=B 1=G 2=R), long arithmetic
//        | { kind: "const", b, g, r }
//        | { kind: "lcg", seed }                    uint state = seed; per byte in order y, x, B, G, R: state = state*1664525 + 1013904223 (wraps), byte = state >> 24
//        | { kind: "checker", lo?, hi? }            byte = ((x + y) & 1) == 0 ? lo (0) : hi (255) in all three channels
//        | { kind: "bytes", values: [b,g,r, b,g,r, ...] }   row-major, sw*sh*3 values
//     src_alpha?: int (default 255, the fourth byte of every source pixel),
//     dispose_src?: bool (the source is disposed before Resize), null_src?: bool (Resize(null, ..)), slow?: bool (only a hint for the Rust side) }
// Reported: { name, ok, error (exception type name | null), width, height, layout, fnv1a64 (hex of all output bytes, row after row, stride = width*4),
//             first_pixel (hex of bytes 0..4), last_pixel (hex of the last 4 bytes), alpha_all_ff (every 4th output byte is 0xFF),
//             src_unchanged (FNV of the source before == after), new_buffer (output is another object with another Id than the source) }
// A failed case reports only name/ok/error (no output fields).
static partial class Commands
{
    private static readonly MethodInfo W514aResize = Type
        .GetType("PhotoReview.Imaging.TurboJpeg.PixelAreaResampler, PhotoReview.Imaging.TurboJpeg", throwOnError: true)!
        .GetMethod("Resize", BindingFlags.NonPublic | BindingFlags.Static)!;

    private static ulong W514aFnv(PixelBuffer buffer)
    {
        var h = 0xcbf29ce484222325UL;
        for (var y = 0; y < buffer.Height; y++)
        {
            foreach (var b in buffer.GetRow(y))
            {
                h ^= b;
                h = unchecked(h * 0x100000001b3UL);
            }
        }

        return h;
    }

    private static PixelBuffer W514aBuild(JsonObject c)
    {
        var sw = c["sw"]!.GetValue<int>();
        var sh = c["sh"]!.GetValue<int>();
        var layout = (c["layout"]?.GetValue<string>() ?? "Bgr32") == "Pbgra32" ? PixelLayout.Pbgra32 : PixelLayout.Bgr32;
        var alpha = (byte)(c["src_alpha"]?.GetValue<int>() ?? 255);
        var spec = c["src"]!.AsObject();
        var kind = spec["kind"]!.GetValue<string>();
        var buffer = PixelBuffer.Allocate(sw, sh, layout);
        uint state = kind == "lcg" ? (uint)spec["seed"]!.GetValue<long>() : 0;
        var values = kind == "bytes" ? spec["values"]!.AsArray().Select(v => (byte)v!.GetValue<int>()).ToArray() : [];
        for (var y = 0; y < sh; y++)
        {
            var row = buffer.GetRow(y);
            for (var x = 0; x < sw; x++)
            {
                for (var ch = 0; ch < 3; ch++)
                {
                    byte value;
                    switch (kind)
                    {
                        case "formula":
                            value = (byte)(((long)x * spec["a"]!.GetValue<int>() + (long)y * spec["b"]!.GetValue<int>()
                                + ch * spec["k"]!.GetValue<int>() + (long)x * y * spec["m"]!.GetValue<int>()
                                + spec["seed"]!.GetValue<int>()) & 255);
                            break;
                        case "const":
                            value = (byte)spec[ch == 0 ? "b" : ch == 1 ? "g" : "r"]!.GetValue<int>();
                            break;
                        case "lcg":
                            state = unchecked(state * 1664525u + 1013904223u);
                            value = (byte)(state >> 24);
                            break;
                        case "checker":
                            value = (byte)(((x + y) & 1) == 0 ? spec["lo"]?.GetValue<int>() ?? 0 : spec["hi"]?.GetValue<int>() ?? 255);
                            break;
                        case "bytes":
                            value = values[((y * sw) + x) * 3 + ch];
                            break;
                        default:
                            throw new ArgumentException("unknown src kind " + kind);
                    }

                    row[x * 4 + ch] = value;
                }

                row[x * 4 + 3] = alpha;
            }
        }

        return buffer;
    }

    public static JsonArray PixelAreaResample(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var name = c["name"]!.GetValue<string>();
            var tw = c["tw"]!.GetValue<int>();
            var th = c["th"]!.GetValue<int>();
            var nullSrc = c["null_src"]?.GetValue<bool>() == true;
            using var src = nullSrc ? null : W514aBuild(c);
            var before = src is null ? 0UL : W514aFnv(src);
            if (src is not null && c["dispose_src"]?.GetValue<bool>() == true) src.Dispose();
            PixelBuffer? dst = null;
            string? error = null;
            try
            {
                dst = (PixelBuffer)W514aResize.Invoke(null, [src, tw, th])!;
            }
            catch (TargetInvocationException e)
            {
                error = e.InnerException!.GetType().Name;
            }

            var entry = new JsonObject { ["name"] = name, ["ok"] = error is null, ["error"] = error };
            if (dst is not null)
            {
                using (dst)
                {
                    var alphaOk = true;
                    for (var y = 0; y < dst.Height && alphaOk; y++)
                    {
                        var row = dst.GetRow(y);
                        for (var x = 3; x < row.Length; x += 4)
                        {
                            if (row[x] != 0xFF)
                            {
                                alphaOk = false;
                                break;
                            }
                        }
                    }

                    var first = dst.GetRow(0).Slice(0, 4).ToArray();
                    var lastRow = dst.GetRow(dst.Height - 1);
                    var last = lastRow.Slice(lastRow.Length - 4, 4).ToArray();
                    entry["width"] = dst.Width;
                    entry["height"] = dst.Height;
                    entry["layout"] = dst.Layout.ToString();
                    entry["fnv1a64"] = W514aFnv(dst).ToString("x16");
                    entry["first_pixel"] = Convert.ToHexString(first);
                    entry["last_pixel"] = Convert.ToHexString(last);
                    entry["alpha_all_ff"] = alphaOk;
                    entry["src_unchanged"] = W514aFnv(src!) == before;
                    entry["new_buffer"] = !ReferenceEquals(src, dst) && src!.Id != dst.Id;
                }
            }

            result.Add(entry);
        }

        return result;
    }
}
