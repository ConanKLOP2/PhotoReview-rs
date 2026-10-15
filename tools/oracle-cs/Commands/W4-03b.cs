using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Metadata;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-03b
//     "exif-parser" => Commands.ExifParserCases(inputs),
//
// Wiring needs (do these in the wiring step, not here):
//   * OracleCs.csproj: add <Reference Include="PhotoReview.Imaging"> with a HintPath to the baseline build of
//     PhotoReview.Imaging.dll (tools/oracle.ps1 must copy it next to PhotoReview.Core.dll). Only PUBLIC members are
//     used below (TryParseJpeg, TryParseTiffBlock x2, TryReadOrientationFromJpeg, TryReadOrientation), so no
//     InternalsVisibleTo is needed. PhotoReview.Imaging targets net10.0-windows (WPF-free for Metadata), same as the oracle.
//   * Output goes to tests/golden/exif-parser.json; inputs are golden-inputs/exif-parser.json.

// W4-03b: ExifParser (JPEG marker walk, TIFF IFD0 + Exif IFD reader, orientation reader)
static partial class Commands
{
    private static JsonNode? ExifRationalNode(ExifRational? value) =>
        value is { } r ? new JsonObject { ["numerator"] = (long)r.Numerator, ["denominator"] = (long)r.Denominator } : null;

    private static JsonNode? ExifSummaryNode(ExifSummary? s) =>
        s is null
            ? null
            : new JsonObject
            {
                // DateTime.Kind is Unspecified; the text is the wall-clock value, invariant, no zone.
                ["date_taken"] = s.DateTaken?.ToString("yyyy-MM-dd'T'HH:mm:ss", CultureInfo.InvariantCulture),
                ["camera_make"] = s.CameraMake,
                ["camera_model"] = s.CameraModel,
                ["lens_model"] = s.LensModel,
                ["iso"] = s.Iso,
                ["focal_length"] = ExifRationalNode(s.FocalLength),
                ["f_number"] = ExifRationalNode(s.FNumber),
                ["exposure_time"] = ExifRationalNode(s.ExposureTime),
            };

    /// <summary>
    /// For each case {name, op, hex}: op is one of
    ///   "jpeg"             ExifParser.TryParseJpeg(bytes)
    ///   "tiff"             ExifParser.TryParseTiffBlock(bytes)
    ///   "tiff-exif"        ExifParser.TryParseTiffBlock(bytes, ifdIsExif: true)
    ///   "orientation-jpeg" ExifParser.TryReadOrientationFromJpeg(bytes)
    ///   "orientation-tiff" ExifParser.TryReadOrientation(bytes)
    /// Result adds "summary" (object or null; ops jpeg/tiff/tiff-exif) or "orientation" (int or null; orientation ops),
    /// and "threw": false (the parser must never throw; a throw is reported as true plus "exception").
    /// </summary>
    public static JsonArray ExifParserCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var bytes = Convert.FromHexString(c["hex"]!.GetValue<string>());
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op, ["hex"] = c["hex"]!.GetValue<string>() };
            try
            {
                switch (op)
                {
                    case "jpeg": o["summary"] = ExifSummaryNode(ExifParser.TryParseJpeg(bytes)); break;
                    case "tiff": o["summary"] = ExifSummaryNode(ExifParser.TryParseTiffBlock(bytes)); break;
                    case "tiff-exif": o["summary"] = ExifSummaryNode(ExifParser.TryParseTiffBlock(bytes, ifdIsExif: true)); break;
                    case "orientation-jpeg": o["orientation"] = ExifParser.TryReadOrientationFromJpeg(bytes); break;
                    case "orientation-tiff": o["orientation"] = ExifParser.TryReadOrientation(bytes); break;
                    default: throw new ArgumentException("unknown op " + op);
                }
                o["threw"] = false;
            }
            catch (Exception ex) when (op is "jpeg" or "tiff" or "tiff-exif" or "orientation-jpeg" or "orientation-tiff")
            {
                o["threw"] = true;
                o["exception"] = ex.GetType().Name;
            }
            result.Add(o);
        }
        return result;
    }
}
