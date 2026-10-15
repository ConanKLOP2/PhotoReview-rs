using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Metadata;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-11d
//     "wic-exif-values" => Commands.WicExifValues(inputs),
//     "wic-exif-variant" => Commands.WicExifVariant(inputs),
// Wiring notes (the wiring step does these, this file does not run anything):
//   * Needs W4-04b.cs (helpers W404bValue / W404bRational) and the same <Reference Include="PhotoReview.Imaging"> as W4-04b.
//   * WicExifReader and WicGuids are `internal`: reached by reflection below (no InternalsVisibleTo). The oracle runs only
//     on Windows (WicExifReader.ReadVariant uses Marshal on real unmanaged memory; no WIC call is made: no COM, no codec).
//   * The native half (IWICMetadataQueryReader, PropVariantClear, GetContainerFormat) is NOT differential; it is covered
//     by the `#[ignore = "native: needs WIC"]` tests listed in the contract.

// W4-11d: WicExifReader.ReadMetadataValues and WicExifReader.ReadVariant, plus the two container GUID constants.
static partial class Commands
{
    private static readonly Type W411dReader = typeof(ExifSummary).Assembly
        .GetType("PhotoReview.Imaging.Decoding.Wic.WicExifReader", throwOnError: true)!;

    private static readonly MethodInfo W411dReadValues = W411dReader.GetMethod("ReadMetadataValues",
        BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public)!;

    private static readonly MethodInfo W411dReadVariant = W411dReader.GetMethod("ReadVariant",
        BindingFlags.Static | BindingFlags.NonPublic)!;

    private static Guid W411dGuid(string field) =>
        (Guid)typeof(ExifSummary).Assembly.GetType("PhotoReview.Imaging.Decoding.Wic.WicGuids", throwOnError: true)!
            .GetField(field, BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)!.GetValue(null)!;

    /// <summary>
    /// ReadMetadataValues for each case {name, read_orientation, root (string|null), answers: {"query": {type, value} | {type: "throw", message}}}.
    /// Output: name, queried (every query name in call order, the order and the abort after a throw are part of the contract),
    /// orientation, summary (null | the W4-04b field object). A query that is not in `answers` answers null. The value types are
    /// those of W404bValue (W4-04b.cs). Last row: {name: "constants", jpeg, tiff} (the two container GUIDs, lower-case "D" form).
    /// </summary>
    public static JsonArray WicExifValues(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var answers = c["answers"]!.AsObject();
            var queried = new JsonArray();
            object? Query(string name)
            {
                queried.Add(name);
                if (!answers.TryGetPropertyValue(name, out var spec) || spec is null) return null;
                if (spec.AsObject()["type"]!.GetValue<string>() == "throw")
                    throw new FormatException(spec.AsObject()["message"]?.GetValue<string>() ?? "query failed");
                return W404bValue(spec);
            }

            var args = new object?[]
            {
                (Func<string, object?>)Query,
                c["read_orientation"]!.GetValue<bool>(),
                c["root"]?.GetValue<string>(),
                null,
            };
            var orientation = (int)W411dReadValues.Invoke(null, args)!;
            var summary = args[3] as ExifSummary;
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["queried"] = queried,
                ["orientation"] = orientation,
                ["summary"] = summary is null ? null : new JsonObject
                {
                    ["date_taken"] = summary.DateTaken?.ToString("yyyy-MM-ddTHH:mm:ss", CultureInfo.InvariantCulture),
                    ["camera_make"] = summary.CameraMake,
                    ["camera_model"] = summary.CameraModel,
                    ["lens_model"] = summary.LensModel,
                    ["iso"] = summary.Iso,
                    ["focal_length"] = W404bRational(summary.FocalLength),
                    ["f_number"] = W404bRational(summary.FNumber),
                    ["exposure_time"] = W404bRational(summary.ExposureTime),
                },
            });
        }

        result.Add(new JsonObject
        {
            ["name"] = "constants",
            ["jpeg"] = W411dGuid("GUID_ContainerFormatJpeg").ToString("D"),
            ["tiff"] = W411dGuid("GUID_ContainerFormatTiff").ToString("D"),
        });
        return result;
    }

    private static string? W411dTypeName(object? value) => value switch
    {
        null => null,
        string => "string",
        short => "i16",
        int => "i32",
        byte => "u8",
        ushort => "u16",
        uint => "u32",
        long => "i64",
        ulong => "u64",
        ushort[] => "u16[]",
        uint[] => "u32[]",
        long[] => "i64[]",
        ulong[] => "u64[]",
        _ => value.GetType().Name,
    };

    private static JsonNode? W411dValueNode(object? value) => value switch
    {
        null => null,
        string s => s,
        Array a => new JsonArray(a.Cast<object>().Select(e => (JsonNode?)Convert.ToString(e, CultureInfo.InvariantCulture)).ToArray()),
        _ => Convert.ToString(value, CultureInfo.InvariantCulture),
    };

    /// <summary>
    /// ReadVariant for each case {name, vt, reserved_hex?, scalar? (decimal int64 written at byte 8), count? (int32 written at byte 8:
    /// vector layout), payload? {kind: utf8|wide|u16|u32|i64|u64, text? | bytes_hex? | elements?: [decimal]}, pointer? (default true:
    /// write the payload address at byte 8 for strings, byte 16 (8 + IntPtr.Size) for vectors)}. A real 24-byte unmanaged buffer is
    /// built and passed to the private method. Output: name, vt, type (see W411dTypeName; null = ReadVariant returned null), value
    /// (decimal strings; array of decimal strings for vectors).
    /// </summary>
    public static JsonArray WicExifVariant(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var vt = c["vt"]!.GetValue<int>();
            var pvar = Marshal.AllocHGlobal(24);
            var payload = IntPtr.Zero;
            try
            {
                for (var i = 0; i < 24; i += 8) Marshal.WriteInt64(pvar, i, 0);
                Marshal.WriteInt16(pvar, 0, unchecked((short)vt));
                if (c["reserved_hex"] is { } reserved)
                {
                    var bytes = Convert.FromHexString(reserved.GetValue<string>());
                    for (var i = 0; i < bytes.Length; i++) Marshal.WriteByte(pvar, 2 + i, bytes[i]);
                }

                if (c["payload"] is { } spec) payload = W411dAllocPayload(spec.AsObject());
                var withPointer = c["pointer"]?.GetValue<bool>() ?? true;

                if (c["count"] is { } count)
                {
                    Marshal.WriteInt32(pvar, 8, count.GetValue<int>());
                    if (withPointer && payload != IntPtr.Zero) Marshal.WriteIntPtr(pvar, 8 + IntPtr.Size, payload);
                }
                else if (c["scalar"] is { } scalar)
                {
                    Marshal.WriteInt64(pvar, 8, long.Parse(scalar.GetValue<string>(), CultureInfo.InvariantCulture));
                }
                else if (withPointer && payload != IntPtr.Zero)
                {
                    Marshal.WriteIntPtr(pvar, 8, payload);
                }

                var value = W411dReadVariant.Invoke(null, new object[] { pvar });
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["vt"] = vt,
                    ["type"] = W411dTypeName(value),
                    ["value"] = W411dValueNode(value),
                });
            }
            finally
            {
                if (payload != IntPtr.Zero) Marshal.FreeHGlobal(payload);
                Marshal.FreeHGlobal(pvar);
            }
        }

        return result;
    }

    /// <summary>Unmanaged copy of a payload (always freed with FreeHGlobal; text payloads get a NUL terminator).</summary>
    private static IntPtr W411dAllocPayload(JsonObject spec)
    {
        var kind = spec["kind"]!.GetValue<string>();
        byte[] bytes;
        switch (kind)
        {
            case "utf8":
                bytes = spec["bytes_hex"] is { } hex
                    ? Convert.FromHexString(hex.GetValue<string>())
                    : Encoding.UTF8.GetBytes(spec["text"]!.GetValue<string>());
                bytes = [.. bytes, 0];
                break;
            case "wide":
                bytes = [.. Encoding.Unicode.GetBytes(spec["text"]!.GetValue<string>()), 0, 0];
                break;
            default:
            {
                var elements = spec["elements"]!.AsArray().Select(e => e!.GetValue<string>()).ToArray();
                var size = kind switch { "u16" => 2, "u32" => 4, "i64" or "u64" => 8, _ => throw new ArgumentException("kind " + kind) };
                bytes = new byte[Math.Max(8, elements.Length * size)];
                for (var i = 0; i < elements.Length; i++)
                {
                    var raw = kind == "u64"
                        ? unchecked((long)ulong.Parse(elements[i], CultureInfo.InvariantCulture))
                        : long.Parse(elements[i], CultureInfo.InvariantCulture);
                    for (var b = 0; b < size; b++) bytes[(i * size) + b] = (byte)((raw >> (8 * b)) & 0xFF);
                }

                break;
            }
        }

        var memory = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, memory, bytes.Length);
        return memory;
    }
}
