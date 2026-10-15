using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Metadata;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-04a
//     "exif-summary-create" => Commands.ExifSummaryCreate(inputs),
//     "exif-summary-text" => Commands.ExifSummaryText(inputs),
//     "exif-summary-date" => Commands.ExifSummaryDate(inputs),
//     "exif-summary-encode" => Commands.ExifSummaryEncode(inputs),
//     "exif-summary-decode" => Commands.ExifSummaryDecode(inputs),
// Needs W2-00.cs (class W2Util).
// Wiring: tools/oracle-cs/oracle-cs.csproj must also reference the baseline build of PhotoReview.Imaging.dll (and
// whatever it needs to load); tools/oracle.ps1 copies it next to PhotoReview.Core.dll. ExifSummaryCodec, ParseDate and
// CleanText are internal, so they are reached through reflection (a ref-struct parameter needs a custom delegate).

// W4-04a: ExifSummary.Create / ParseDate / CleanText and ExifSummaryCodec.Encode / Decode
static partial class Commands
{
    private delegate ExifSummary? W404DecodeFn(ReadOnlySpan<byte> data);

    private static readonly Type W404CodecType =
        typeof(ExifSummary).Assembly.GetType("PhotoReview.Imaging.Metadata.ExifSummaryCodec", throwOnError: true)!;

    private static readonly Func<ExifSummary?, byte[]> W404Encode = (Func<ExifSummary?, byte[]>)Delegate.CreateDelegate(
        typeof(Func<ExifSummary?, byte[]>), W404CodecType.GetMethod("Encode", BindingFlags.Public | BindingFlags.Static)!);

    private static readonly W404DecodeFn W404Decode = (W404DecodeFn)Delegate.CreateDelegate(
        typeof(W404DecodeFn), W404CodecType.GetMethod("Decode", BindingFlags.Public | BindingFlags.Static)!);

    private static readonly Func<string?, DateTime?> W404ParseDate = (Func<string?, DateTime?>)Delegate.CreateDelegate(
        typeof(Func<string?, DateTime?>), typeof(ExifSummary).GetMethod("ParseDate", BindingFlags.NonPublic | BindingFlags.Static)!);

    private static readonly Func<string?, string?> W404CleanText = (Func<string?, string?>)Delegate.CreateDelegate(
        typeof(Func<string?, string?>), typeof(ExifSummary).GetMethod("CleanText", BindingFlags.NonPublic | BindingFlags.Static)!);

    private static ExifRational? W404Rational(JsonNode? node) =>
        node is JsonArray a ? new ExifRational(a[0]!.GetValue<uint>(), a[1]!.GetValue<uint>()) : null;

    private static JsonNode? W404RationalJson(ExifRational? r) =>
        r is { } v ? new JsonArray(JsonValue.Create(v.Numerator), JsonValue.Create(v.Denominator)) : null;

    private static string W404DateText(DateTime d) => d.ToString("yyyy-MM-ddTHH:mm:ss.fffffff", CultureInfo.InvariantCulture);

    /// <summary>"key" or "key_repeat": {"text", "count"} with optional case-level "prefix"/"suffix" (long texts without writing them out).</summary>
    private static string? W404Text(JsonObject c, string key) =>
        c[key + "_repeat"] is JsonObject r
            ? (W2Util.Str(c["prefix"]) ?? "")
                + string.Concat(Enumerable.Repeat(r["text"]!.GetValue<string>(), r["count"]!.GetValue<int>()))
                + (W2Util.Str(c["suffix"]) ?? "")
            : W2Util.Str(c[key]);

    private static JsonObject W404Describe(ExifSummary? s)
    {
        if (s is null) return new JsonObject { ["is_null"] = true };
        return new JsonObject
        {
            ["is_null"] = false,
            ["date_ticks"] = s.DateTaken is { } d ? JsonValue.Create(d.Ticks) : null,
            ["date_text"] = s.DateTaken is { } d2 ? W404DateText(d2) : null,
            ["make"] = s.CameraMake,
            ["model"] = s.CameraModel,
            ["lens"] = s.LensModel,
            ["iso"] = s.Iso is { } iso ? JsonValue.Create(iso) : null,
            ["focal"] = W404RationalJson(s.FocalLength),
            ["f_number"] = W404RationalJson(s.FNumber),
            ["exposure"] = W404RationalJson(s.ExposureTime),
            ["is_empty"] = s.IsEmpty,
            ["encoded_hex"] = Convert.ToHexString(W404Encode(s)),
        };
    }

    private static JsonObject W404Case(JsonObject c, JsonObject body)
    {
        var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
        foreach (var (k, v) in body) o[k] = v?.DeepClone();
        return o;
    }

    /// <summary>ExifSummary.Create for each case {name, date, make, model, lens, iso (long|null), focal, f_number, exposure ([n, d]|null)}.</summary>
    public static JsonArray ExifSummaryCreate(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            long? iso = c["iso"] is { } i ? i.GetValue<long>() : null;
            var summary = ExifSummary.Create(
                W404Text(c, "date"), W404Text(c, "make"), W404Text(c, "model"), W404Text(c, "lens"), iso,
                W404Rational(c["focal"]), W404Rational(c["f_number"]), W404Rational(c["exposure"]));
            result.Add(W404Case(c, new JsonObject { ["input"] = c.DeepClone(), ["summary"] = W404Describe(summary) }));
        }
        return result;
    }

    /// <summary>ExifSummary.CleanText for each case {name, text|text_repeat}: the text (null when none) and its UTF-16 length.</summary>
    public static JsonArray ExifSummaryText(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var cleaned = W404CleanText(W404Text(c, "text"));
            result.Add(W404Case(c, new JsonObject
            {
                ["cleaned"] = cleaned,
                ["utf16_length"] = cleaned is null ? null : JsonValue.Create(cleaned.Length),
            }));
        }
        return result;
    }

    /// <summary>ExifSummary.ParseDate for each case {name, text}: ticks and ISO text, or null.</summary>
    public static JsonArray ExifSummaryDate(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var date = W404ParseDate(W2Util.Str(c["text"]));
            result.Add(W404Case(c, new JsonObject
            {
                ["text"] = W2Util.Str(c["text"]),
                ["ticks"] = date is { } d ? JsonValue.Create(d.Ticks) : null,
                ["date_text"] = date is { } d2 ? W404DateText(d2) : null,
            }));
        }
        return result;
    }

    /// <summary>
    /// Encode of a summary built WITHOUT Create (like an object initialiser in the C# tests) for each case
    /// {name, date_ticks?, make|make_repeat, model, lens, iso?, focal, f_number, exposure}: the block as hex and what Decode makes of it.
    /// </summary>
    public static JsonArray ExifSummaryEncode(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var summary = new ExifSummary
            {
                DateTaken = c["date_ticks"] is { } t ? new DateTime(t.GetValue<long>(), DateTimeKind.Unspecified) : null,
                CameraMake = W404Text(c, "make"),
                CameraModel = W404Text(c, "model"),
                LensModel = W404Text(c, "lens"),
                Iso = c["iso"] is { } i ? i.GetValue<int>() : null,
                FocalLength = W404Rational(c["focal"]),
                FNumber = W404Rational(c["f_number"]),
                ExposureTime = W404Rational(c["exposure"]),
            };
            var encoded = W404Encode(summary);
            result.Add(W404Case(c, new JsonObject
            {
                ["is_empty"] = summary.IsEmpty,
                ["encoded_hex"] = Convert.ToHexString(encoded),
                ["encoded_length"] = encoded.Length,
                ["decoded"] = W404Describe(W404Decode(encoded)),
            }));
        }
        return result;
    }

    /// <summary>ExifSummaryCodec.Decode for each case {name, hex}: the decoded summary (or is_null) and its re-encoded hex.</summary>
    public static JsonArray ExifSummaryDecode(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var bytes = Convert.FromHexString(c["hex"]!.GetValue<string>());
            result.Add(W404Case(c, new JsonObject
            {
                ["hex"] = c["hex"]!.GetValue<string>(),
                ["decoded"] = W404Describe(W404Decode(bytes)),
            }));
        }
        return result;
    }
}
