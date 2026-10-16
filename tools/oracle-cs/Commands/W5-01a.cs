using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Raw;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-01a
//     "raw-formats" => Commands.RawFormats(inputs),
// Wiring (the lead does it, NOT this draft):
//   * tools/oracle.ps1 must also build src/PhotoReview.Imaging.Raw/PhotoReview.Imaging.Raw.csproj into $coreDir
//     (it references Core + Imaging only; add it to the "any DLL missing" check too);
//   * tools/oracle-cs/OracleCs.csproj needs  <Reference Include="PhotoReview.Imaging.Raw"><HintPath>$(CSharpCoreDir)\PhotoReview.Imaging.Raw.dll</HintPath></Reference>.
// The internal members (RawContainerLimits.MaxJpeg*, InitialProbeLength, RawHeaderErrors.IsIoFailure) are reached through
// reflection, so no InternalsVisibleTo is needed.

// W5-01a: RawFileTypes.IsRawExtension, RawFormat values, RawContainerLimits constants + InitialProbeLength,
// RawHeaderErrors.IsIoFailure.
static partial class Commands
{
    /// <summary>
    /// Input: "paths" (string|null list), "enum_members" (bool, optional), "probe_lengths" (long list),
    /// "inner_kinds" (list of "none"|"IOException"|"FileNotFoundException"|"EndOfStreamException"|"ObjectDisposedException"|
    /// "InvalidDataException"|"ArgumentException"|"UnauthorizedAccessException"|"InvalidOperationException").
    /// Output cases, each with a "kind":
    ///   extension {path, is_raw}; enum {name, value}; limit {name, value}; probe {length, result}; io-failure {inner, result}.
    /// </summary>
    public static JsonArray RawFormats(JsonObject inputs)
    {
        var result = new JsonArray();

        foreach (var node in inputs["paths"]!.AsArray())
        {
            string? path = node?.GetValue<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "extension",
                ["path"] = path,
                ["is_raw"] = RawFileTypes.IsRawExtension(path),
            });
        }

        // Every RawFormat member in declaration order (names + numeric values; the Rust discriminants must match).
        foreach (var field in typeof(RawFormat).GetFields(BindingFlags.Public | BindingFlags.Static))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "enum",
                ["name"] = field.Name,
                ["value"] = Convert.ToInt32(field.GetValue(null), System.Globalization.CultureInfo.InvariantCulture),
            });
        }

        // Every const of RawContainerLimits, public and internal, sorted by name for a stable file.
        var limitsType = typeof(RawContainerLimits);
        foreach (var field in limitsType
                     .GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
                     .Where(f => f.IsLiteral)
                     .OrderBy(f => f.Name, StringComparer.Ordinal))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "limit",
                ["name"] = field.Name,
                ["value"] = Convert.ToInt64(field.GetRawConstantValue(), System.Globalization.CultureInfo.InvariantCulture),
            });
        }

        var probe = limitsType.GetMethod("InitialProbeLength", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
        foreach (var node in inputs["probe_lengths"]!.AsArray())
        {
            long length = node!.GetValue<long>();
            result.Add(new JsonObject
            {
                ["kind"] = "probe",
                ["length"] = length,
                ["result"] = (int)probe.Invoke(null, [length])!,
            });
        }

        var errorsType = typeof(RawFormat).Assembly.GetType("PhotoReview.Imaging.Raw.RawHeaderErrors", throwOnError: true)!;
        var isIo = errorsType.GetMethod("IsIoFailure", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
        foreach (var node in inputs["inner_kinds"]!.AsArray())
        {
            string kind = node!.GetValue<string>();
            Exception? inner = kind switch
            {
                "none" => null,
                "IOException" => new IOException("io"),
                "FileNotFoundException" => new FileNotFoundException("fnf"),
                "EndOfStreamException" => new EndOfStreamException("eos"),
                "ObjectDisposedException" => new ObjectDisposedException("src"),
                "InvalidDataException" => new InvalidDataException("nested"),
                "ArgumentException" => new ArgumentException("arg"),
                "UnauthorizedAccessException" => new UnauthorizedAccessException("denied"),
                "InvalidOperationException" => new InvalidOperationException("op"),
                _ => throw new ArgumentException("unknown inner kind " + kind),
            };
            var ex = inner is null ? new InvalidDataException("bad") : new InvalidDataException("bad", inner);
            result.Add(new JsonObject
            {
                ["kind"] = "io-failure",
                ["inner"] = kind,
                ["result"] = (bool)isIo.Invoke(null, [ex])!,
            });
        }

        return result;
    }
}
