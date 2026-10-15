using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Pixels;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-16b
//     "native-memory" => Commands.NativeMemory(inputs),
//     "readonly-memory-stream" => Commands.ReadOnlyMemoryStream(inputs),
//     "pixel-codec" => Commands.PixelCodec(inputs),
// Needs a Reference to PhotoReview.Imaging.dll in OracleCs.csproj (the wiring step adds it; today only Core is referenced).
// NativePixelMemory and ReadOnlyMemoryStreamFactory are `internal` and the oracle is not a friend assembly: both are reached by
// reflection (names and signatures are the C# baseline d92d6aae).

// W4-16b: native block lifetime/alignment, the read-only memory stream, the "pixels" platform codec.
static partial class Commands
{
    private const BindingFlags W416bStatic = BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public;
    private const BindingFlags W416bInstance = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;

    private static readonly Type W416bMemoryType =
        typeof(PixelBuffer).Assembly.GetType("PhotoReview.Imaging.Pixels.NativePixelMemory", throwOnError: true)!;

    private static readonly Type W416bFactoryType =
        typeof(PixelBuffer).Assembly.GetType("PhotoReview.Imaging.Decoding.ReadOnlyMemoryStreamFactory", throwOnError: true)!;

    private static long W416bLive() => (long)W416bMemoryType.GetProperty("LiveCount", W416bStatic)!.GetValue(null)!;

    private static string W416bHex(byte[] bytes) => Convert.ToHexString(bytes).ToLowerInvariant();

    /// <summary>
    /// Cases {name, kind: "allocate"|"unallocated", byte_count?: long}. "allocate" calls the internal
    /// <c>NativePixelMemory.Allocate(byteCount)</c>; reports either the exception type name or: is_invalid, byte_count,
    /// address_mod_alignment (address % 64), live_delta_after_allocate, is_closed_before_dispose, live_delta_after_dispose,
    /// is_closed_after_dispose, live_delta_after_second_dispose, and the Alignment constant.
    /// "unallocated" is <c>new NativePixelMemory()</c> (reports the same fields; live deltas must all be 0).
    /// Sizes are small (no case allocates more than 1 MiB) except the two refusal cases (1 &lt;&lt; 62, long.MaxValue).
    /// </summary>
    public static JsonArray NativeMemory(JsonObject inputs)
    {
        var result = new JsonArray();
        var allocate = W416bMemoryType.GetMethod("Allocate", W416bStatic)!;
        var byteCountProp = W416bMemoryType.GetProperty("ByteCount", W416bInstance)!;
        var alignment = (int)W416bMemoryType.GetField("Alignment", W416bStatic)!.GetValue(null)!;
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind, ["alignment"] = alignment };
            var before = W416bLive();
            try
            {
                SafeHandle block;
                if (kind == "unallocated")
                {
                    block = (SafeHandle)Activator.CreateInstance(W416bMemoryType, nonPublic: true)!;
                }
                else
                {
                    row["byte_count_in"] = c["byte_count"]!.GetValue<long>();
                    try
                    {
                        block = (SafeHandle)allocate.Invoke(null, [c["byte_count"]!.GetValue<long>()])!;
                    }
                    catch (TargetInvocationException tie)
                    {
                        row["exception"] = tie.InnerException!.GetType().Name;
                        row["live_delta_after_failure"] = W416bLive() - before;
                        result.Add(row);
                        continue;
                    }
                }

                row["exception"] = null;
                row["is_invalid"] = block.IsInvalid;
                row["byte_count"] = (long)byteCountProp.GetValue(block)!;
                row["address_mod_alignment"] = (long)block.DangerousGetHandle() % alignment;
                row["live_delta_after_allocate"] = W416bLive() - before;
                row["is_closed_before_dispose"] = block.IsClosed;
                block.Dispose();
                row["live_delta_after_dispose"] = W416bLive() - before;
                row["is_closed_after_dispose"] = block.IsClosed;
                block.Dispose();
                row["live_delta_after_second_dispose"] = W416bLive() - before;
            }
            catch (Exception ex)
            {
                row["exception"] = "UNEXPECTED:" + ex.GetType().Name;
            }
            result.Add(row);
        }
        return result;
    }

    /// <summary>
    /// Cases {name, hex, shape: "whole"|"slice"|"empty", offset?, count?}. Builds a <c>byte[]</c> from <c>hex</c>; "whole" passes
    /// the array as <c>ReadOnlyMemory</c>, "slice" passes <c>array[offset..offset+count]</c>, "empty" passes
    /// <c>ReadOnlyMemory&lt;byte&gt;.Empty</c>. Reports can_read/can_write/can_seek, length, position, the whole content,
    /// tail_hex (after seeking to max(0, length - 2) from the start), write_exception (type of the exception writing one byte,
    /// null when it did not throw) and is_memory_stream.
    /// </summary>
    public static JsonArray ReadOnlyMemoryStream(JsonObject inputs)
    {
        var result = new JsonArray();
        var create = W416bFactoryType.GetMethod("Create", W416bStatic)!;
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var shape = c["shape"]!.GetValue<string>();
            var bytes = Convert.FromHexString(c["hex"]!.GetValue<string>());
            ReadOnlyMemory<byte> memory = shape switch
            {
                "whole" => new ReadOnlyMemory<byte>(bytes),
                "slice" => new ReadOnlyMemory<byte>(bytes, c["offset"]!.GetValue<int>(), c["count"]!.GetValue<int>()),
                "empty" => ReadOnlyMemory<byte>.Empty,
                _ => throw new ArgumentException($"unknown shape {shape}"),
            };
            using var stream = (Stream)create.Invoke(null, [memory])!;
            var row = new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["shape"] = shape,
                ["is_memory_stream"] = stream is MemoryStream,
                ["can_read"] = stream.CanRead,
                ["can_write"] = stream.CanWrite,
                ["can_seek"] = stream.CanSeek,
                ["length"] = stream.Length,
                ["position"] = stream.Position,
            };
            using (var copy = new MemoryStream())
            {
                stream.CopyTo(copy);
                row["content_hex"] = W416bHex(copy.ToArray());
            }

            stream.Position = Math.Max(0, stream.Length - 2);
            using (var copy = new MemoryStream())
            {
                stream.CopyTo(copy);
                row["tail_hex"] = W416bHex(copy.ToArray());
            }

            try
            {
                stream.WriteByte(0xFF);
                row["write_exception"] = null;
            }
            catch (Exception ex)
            {
                row["write_exception"] = ex.GetType().Name;
            }
            result.Add(row);
        }
        return result;
    }

    /// <summary>
    /// Cases {name, op}: "name" (codec.Name); "from_live" (FromPixels of a live buffer: same_ref, is_disposed_after);
    /// "from_disposed" (exception type); "to_live" (ToPixels: same_ref, owned, is_disposed_after_lease_dispose);
    /// "to_disposed" (exception type); "to_foreign_string" / "to_foreign_object" (exception type);
    /// "roundtrip" (ToPixels(FromPixels(p)): same_ref, owned). Null arguments are not representable in Rust and are not cases.
    /// </summary>
    public static JsonArray PixelCodec(JsonObject inputs)
    {
        var result = new JsonArray();
        var codec = PixelBufferImageCodec.Instance;
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op };
            try
            {
                switch (op)
                {
                    case "name":
                        row["codec_name"] = codec.Name;
                        break;
                    case "from_live":
                    {
                        using var pixels = PixelBuffer.Allocate(3, 3, PixelLayout.Bgr32);
                        var platform = codec.FromPixels(pixels);
                        row["same_ref"] = ReferenceEquals(platform, pixels);
                        row["is_disposed_after"] = pixels.IsDisposed;
                        break;
                    }
                    case "from_disposed":
                    {
                        var pixels = PixelBuffer.Allocate(1, 1, PixelLayout.Bgr32);
                        pixels.Dispose();
                        codec.FromPixels(pixels);
                        break;
                    }
                    case "to_live":
                    {
                        using var pixels = PixelBuffer.Allocate(4, 2, PixelLayout.Pbgra32);
                        var lease = codec.ToPixels(pixels);
                        row["same_ref"] = ReferenceEquals(lease.Pixels, pixels);
                        row["owned"] = lease.Owned;
                        lease.Dispose();
                        row["is_disposed_after_lease_dispose"] = pixels.IsDisposed;
                        break;
                    }
                    case "to_disposed":
                    {
                        var pixels = PixelBuffer.Allocate(2, 2, PixelLayout.Bgr32);
                        pixels.Dispose();
                        codec.ToPixels(pixels);
                        break;
                    }
                    case "to_foreign_string":
                        codec.ToPixels("not an image");
                        break;
                    case "to_foreign_object":
                        codec.ToPixels(new object());
                        break;
                    case "roundtrip":
                    {
                        using var pixels = PixelBuffer.Allocate(5, 4, PixelLayout.Pbgra32);
                        using var lease = codec.ToPixels(codec.FromPixels(pixels));
                        row["same_ref"] = ReferenceEquals(lease.Pixels, pixels);
                        row["owned"] = lease.Owned;
                        break;
                    }
                    default:
                        throw new ArgumentException($"unknown op {op}");
                }
                row["exception"] = null;
            }
            catch (Exception ex)
            {
                row["exception"] = ex.GetType().Name;
            }
            result.Add(row);
        }
        return result;
    }
}
