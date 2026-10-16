using System.IO;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-11a
//     "raw-registry-find" => Commands.RawRegistryFind(inputs),
//     "raw-icc-ensure" => Commands.RawIccEnsure(inputs),
// Needs a Reference to PhotoReview.Imaging.Raw.dll in OracleCs.csproj (the wiring step adds it; same note as W5-01b).
// RawJpegIccProfile is internal and the registry list is private: both are reached through reflection, so the oracle needs no
// InternalsVisibleTo entry in the C# repo (which must not be edited).

// W5-11a: RawContainerReaderRegistry.FindReader routing and RawJpegIccProfile.EnsureAdobeRgbProfile.
static partial class Commands
{
    private static IRawContainerReader W511aReader(string name) => name switch
    {
        "Cr2" => new PhotoReview.Imaging.Raw.Tiff.Cr2ContainerReader(),
        "Nef" => new PhotoReview.Imaging.Raw.Tiff.NefContainerReader(),
        "Arw" => new PhotoReview.Imaging.Raw.Tiff.ArwContainerReader(),
        "Dng" => new PhotoReview.Imaging.Raw.Tiff.DngContainerReader(),
        "Orf" => new PhotoReview.Imaging.Raw.Tiff.OrfContainerReader(),
        "Rw2" => new PhotoReview.Imaging.Raw.Tiff.Rw2ContainerReader(),
        "Cr3" => new PhotoReview.Imaging.Raw.Bmff.Cr3ContainerReader(),
        "Raf" => new PhotoReview.Imaging.Raw.Raf.RafContainerReader(),
        _ => throw new ArgumentException("unknown reader " + name),
    };

    private static byte[] W511aHex(string hex) => Convert.FromHexString(hex);

    /// <summary>
    /// Cases {name, header_hex, ext, registry?: [names]}: registry omitted = <c>new RawContainerReaderRegistry()</c>, an array = the
    /// <c>IEnumerable</c> constructor in that order. Reports the routed reader ("Cr2".."Raf" = type name without "ContainerReader", or null)
    /// and its zero-based position in the registry's own reader list (read through reflection, so the default order is observed, not assumed),
    /// Case {name, op:"default-order"}: the default registry's reader names in registration order.
    /// </summary>
    public static JsonArray RawRegistryFind(JsonObject inputs)
    {
        var listField = typeof(RawContainerReaderRegistry).GetField("_readers", BindingFlags.NonPublic | BindingFlags.Instance)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            if (c["op"]?.GetValue<string>() == "default-order")
            {
                var order = new JsonArray();
                foreach (var r in (List<IRawContainerReader>)listField.GetValue(new RawContainerReaderRegistry())!)
                    order.Add(r.GetType().Name.Replace("ContainerReader", ""));
                result.Add(new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["order"] = order });
                continue;
            }
            var registry = c["registry"] is JsonArray names
                ? new RawContainerReaderRegistry(names.Select(n => W511aReader(n!.GetValue<string>())).ToList())
                : new RawContainerReaderRegistry();
            var list = (List<IRawContainerReader>)listField.GetValue(registry)!;
            var found = registry.FindReader(W511aHex(c["header_hex"]!.GetValue<string>()), c["ext"]!.GetValue<string>());
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            if (found is null)
            {
                item["reader"] = null;
                item["index"] = null;
            }
            else
            {
                item["reader"] = found.GetType().Name.Replace("ContainerReader", "");
                item["index"] = list.IndexOf(found);
            }
            result.Add(item);
        }
        return result;
    }

    private static byte[] W511aBundledProfile()
    {
        using var stream = typeof(RawContainerReaderRegistry).Assembly.GetManifestResourceStream("PhotoReview.Imaging.Raw.AdobeCompat-v2.icc")!;
        using var buffer = new MemoryStream();
        stream.CopyTo(buffer);
        return buffer.ToArray();
    }

    /// <summary>
    /// Cases {name, jpeg_hex} (or {name, op:"profile"}): <c>RawJpegIccProfile.EnsureAdobeRgbProfile(jpeg)</c> via reflection. Reports ok, the
    /// exception type and message, <c>same</c> (reference equality with the input: "returned as is"), the result length, the first
    /// min(20, len) bytes as hex, whether bytes [20, 20+profileLen) equal the bundled profile and whether the rest equals input[2..].
    /// Case op "profile": the bundled profile's length and FNV-1a 64 hash (the Rust asset must be byte-identical).
    /// </summary>
    public static JsonArray RawIccEnsure(JsonObject inputs)
    {
        var type = typeof(RawContainerReaderRegistry).Assembly.GetType("PhotoReview.Imaging.Raw.RawJpegIccProfile")!;
        var method = type.GetMethod("EnsureAdobeRgbProfile", BindingFlags.NonPublic | BindingFlags.Static)!;
        var profile = W511aBundledProfile();
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            if (c["op"]?.GetValue<string>() == "profile")
            {
                ulong h = 14695981039346656037UL;
                foreach (var b in profile) { h ^= b; h = unchecked(h * 1099511628211UL); }
                item["profile_len"] = profile.Length;
                item["profile_fnv1a64"] = h.ToString("x16");
                result.Add(item);
                continue;
            }

            var jpeg = W511aHex(c["jpeg_hex"]!.GetValue<string>());
            try
            {
                var output = (byte[])method.Invoke(null, [jpeg])!;
                item["ok"] = true;
                item["same"] = ReferenceEquals(output, jpeg);
                item["len"] = output.Length;
                item["head_hex"] = Convert.ToHexString(output.AsSpan(0, Math.Min(20, output.Length))).ToLowerInvariant();
                if (!ReferenceEquals(output, jpeg))
                {
                    item["profile_equal"] = output.AsSpan(20, profile.Length).SequenceEqual(profile);
                    item["tail_equals_input_after_soi"] = output.AsSpan(20 + profile.Length).SequenceEqual(jpeg.AsSpan(2));
                }
            }
            catch (TargetInvocationException ex)
            {
                item["ok"] = false;
                item["error"] = ex.InnerException!.GetType().Name;
                item["message"] = ex.InnerException.Message;
            }
            result.Add(item);
        }
        return result;
    }
}
