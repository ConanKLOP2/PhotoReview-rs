using System.Text.Json.Nodes;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-16a
//     "pixel-buffer" => Commands.PixelBufferCases(inputs),
//     "pixel-ops" => Commands.PixelOpsCases(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * PixelBuffer / PixelOps live in PhotoReview.Imaging.dll (same reference W4-01a and W4-02a need). Only public API is
//     used (GetRow, TryGetSpan, Address), so no AllowUnsafeBlocks and no InternalsVisibleTo are needed.
//   * Needs no other oracle file. The existing W4-02a golden `orientation-pixels` already proves the pixel PLACEMENT
//     (source ids) for 3x2 .. odd sizes; this file adds layouts, tile-crossing sizes, byte hashes and the buffer/alpha rules.

// W4-16a: PixelBuffer.Allocate / GetRow / TryGetSpan / Dispose, PixelOps.ApplyOrientation / IsFullyOpaque / HasAlpha.
static partial class Commands
{
    private const ulong Fnv64Offset = 14695981039346656037UL;
    private const ulong Fnv64Prime = 1099511628211UL;

    /// <summary>FNV-1a 64 over the bytes, 16 lowercase hex digits (the Rust test re-implements these 5 lines).</summary>
    private static string W416Fnv(ReadOnlySpan<byte> bytes)
    {
        var h = Fnv64Offset;
        foreach (var b in bytes) h = unchecked((h ^ b) * Fnv64Prime);
        return h.ToString("x16");
    }

    /// <summary>Whole buffer as a span through the public API (all golden buffers are far below 2 GiB).</summary>
    private static byte[] W416Bytes(PixelBuffer px)
    {
        if (!px.TryGetSpan(out var span)) throw new InvalidOperationException("buffer too large for the oracle");
        return span.ToArray();
    }

    /// <summary>
    /// Deterministic content: byte i (row-major, no padding) = (i * 131 + seed * 17 + i / 251) mod 256, all in 64-bit
    /// integer arithmetic. The Rust test fills with the same formula (`i as u64`, `seed as u64`; seeds are non-negative).
    /// </summary>
    private static PixelBuffer W416Pattern(int width, int height, PixelLayout layout, int seed)
    {
        var px = PixelBuffer.Allocate(width, height, layout);
        if (!px.TryGetSpan(out var all)) throw new InvalidOperationException("too large");
        for (var i = 0; i < all.Length; i++) all[i] = (byte)((((long)i * 131) + ((long)seed * 17) + (i / 251)) & 0xFF);
        return px;
    }

    private static JsonObject W416Throws(Exception ex) => new() { ["throws"] = ex.GetType().Name };

    private static JsonObject W416Describe(PixelBuffer px, bool withHash)
    {
        var o = new JsonObject
        {
            ["width"] = px.Width,
            ["height"] = px.Height,
            ["layout"] = (int)px.Layout,
            ["stride"] = px.Stride,
            ["byte_count"] = px.ByteCount,
        };
        if (withHash)
        {
            var bytes = W416Bytes(px);
            o["fnv1a64"] = W416Fnv(bytes);
            // Small images also list the raw bytes so a failing Rust case can be read by eye.
            if (bytes.Length <= 64) o["hex"] = Convert.ToHexString(bytes).ToLowerInvariant();
        }

        return o;
    }

    /// <summary>
    /// Cases {name, width, height, layout:int, rows:[int]}: Allocate((PixelLayout)layout). Result:
    /// "throws" (exception type name) or {width, height, stride, byte_count, layout, is_disposed, address_aligned_64,
    /// id_positive, rows:[{y, length | throws}], span_ok, span_length, after_dispose:{...}}.
    /// after_dispose lists is_disposed, the geometry still readable and the exception type of Address, GetRow(0),
    /// TryGetSpan; a second Dispose must not throw ("second_dispose_ok").
    /// </summary>
    public static JsonArray PixelBufferCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            PixelBuffer px;
            try
            {
                px = PixelBuffer.Allocate(c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>(), (PixelLayout)c["layout"]!.GetValue<int>());
            }
            catch (Exception ex)
            {
                o["result"] = W416Throws(ex);
                result.Add(o);
                continue;
            }

            var r = W416Describe(px, withHash: false);
            r["is_disposed"] = px.IsDisposed;
            r["address_aligned_64"] = px.Address % 64 == 0;
            r["id_positive"] = px.Id > 0;
            var rows = new JsonArray();
            foreach (var yNode in c["rows"]!.AsArray())
            {
                var y = yNode!.GetValue<int>();
                var row = new JsonObject { ["y"] = y };
                try { row["length"] = px.GetRow(y).Length; }
                catch (Exception ex) { row["throws"] = ex.GetType().Name; }
                rows.Add(row);
            }

            r["rows"] = rows;
            r["span_ok"] = px.TryGetSpan(out var span);
            r["span_length"] = span.Length;

            px.Dispose();
            var after = new JsonObject
            {
                ["is_disposed"] = px.IsDisposed,
                ["width"] = px.Width,
                ["height"] = px.Height,
                ["byte_count"] = px.ByteCount,
                ["address"] = W416Try(() => { _ = px.Address; }),
                ["get_row"] = W416Try(() => { _ = px.GetRow(0); }),
                ["try_get_span"] = W416Try(() => { _ = px.TryGetSpan(out _); }),
            };
            try { px.Dispose(); after["second_dispose_ok"] = true; }
            catch { after["second_dispose_ok"] = false; }
            r["after_dispose"] = after;
            o["result"] = r;
            result.Add(o);
        }

        return result;
    }

    /// <summary>Exception type name, or null (JSON null) when the action did not throw.</summary>
    private static JsonNode? W416Try(Action action)
    {
        try { action(); return null; }
        catch (Exception ex) { return ex.GetType().Name; }
    }

    /// <summary>
    /// Cases {name, kind, ...}:
    ///  * "orient": {width, height, layout, orientation, seed}: pattern -> ApplyOrientation. Result: same_instance,
    ///    source_disposed, id_changed, then the describe block of the returned buffer (width, height, layout, stride,
    ///    byte_count, fnv1a64, hex when at most 16 pixels).
    ///  * "compose": {width, height, layout, seed, steps:[int]}: apply the steps in order (each consumes the previous
    ///    buffer). Result: describe block + equals_original (bytes and size equal the starting pattern).
    ///  * "alpha": {width, height, layout, seed, alpha_fill, alpha_set:[{x,y,a}], colour_fill?}: pattern, then every
    ///    byte 3 is set to alpha_fill, then the listed pixels' byte 3; colour_fill (when present) sets bytes 0..2 of all
    ///    pixels. Result: is_fully_opaque, has_alpha.
    ///  * "disposed": {width, height, layout, op: "orient-N"|"is_fully_opaque"|"has_alpha"}: operation on a disposed buffer.
    ///    Result: throws (exception type name).
    /// </summary>
    public static JsonArray PixelOpsCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            int width = c["width"]!.GetValue<int>(), height = c["height"]!.GetValue<int>();
            var layout = (PixelLayout)c["layout"]!.GetValue<int>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            switch (kind)
            {
                case "orient":
                {
                    var src = W416Pattern(width, height, layout, c["seed"]!.GetValue<int>());
                    var srcId = src.Id;
                    var dst = PixelOps.ApplyOrientation(src, c["orientation"]!.GetValue<int>());
                    var r = W416Describe(dst, withHash: true);
                    r["same_instance"] = ReferenceEquals(src, dst);
                    r["source_disposed"] = src.IsDisposed;
                    r["id_changed"] = dst.Id != srcId;
                    o["result"] = r;
                    dst.Dispose();
                    break;
                }
                case "compose":
                {
                    var start = W416Pattern(width, height, layout, c["seed"]!.GetValue<int>());
                    var startBytes = W416Bytes(start);
                    var cur = start;
                    foreach (var step in c["steps"]!.AsArray()) cur = PixelOps.ApplyOrientation(cur, step!.GetValue<int>());
                    var r = W416Describe(cur, withHash: true);
                    r["equals_original"] = cur.Width == width && cur.Height == height && W416Bytes(cur).AsSpan().SequenceEqual(startBytes);
                    o["result"] = r;
                    cur.Dispose();
                    break;
                }
                case "alpha":
                {
                    var px = W416Pattern(width, height, layout, c["seed"]!.GetValue<int>());
                    var fill = (byte)c["alpha_fill"]!.GetValue<int>();
                    var colour = c["colour_fill"]?.GetValue<int>();
                    for (var y = 0; y < height; y++)
                    {
                        var row = px.GetRow(y);
                        for (var x = 0; x < width; x++)
                        {
                            if (colour is { } cf)
                            {
                                row[(x * 4) + 0] = row[(x * 4) + 1] = row[(x * 4) + 2] = (byte)cf;
                            }

                            row[(x * 4) + 3] = fill;
                        }
                    }

                    foreach (var s in c["alpha_set"]!.AsArray())
                    {
                        var so = s!.AsObject();
                        px.GetRow(so["y"]!.GetValue<int>())[(so["x"]!.GetValue<int>() * 4) + 3] = (byte)so["a"]!.GetValue<int>();
                    }

                    o["result"] = new JsonObject
                    {
                        ["is_fully_opaque"] = PixelOps.IsFullyOpaque(px),
                        ["has_alpha"] = PixelOps.HasAlpha(px),
                    };
                    px.Dispose();
                    break;
                }
                case "disposed":
                {
                    var px = PixelBuffer.Allocate(width, height, layout);
                    px.Dispose();
                    var op = c["op"]!.GetValue<string>();
                    o["result"] = new JsonObject
                    {
                        ["throws"] = W416Try(() =>
                        {
                            if (op == "is_fully_opaque") PixelOps.IsFullyOpaque(px);
                            else if (op == "has_alpha") PixelOps.HasAlpha(px);
                            else PixelOps.ApplyOrientation(px, int.Parse(op["orient-".Length..]));
                        }),
                    };
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + kind);
            }

            result.Add(o);
        }

        return result;
    }
}
