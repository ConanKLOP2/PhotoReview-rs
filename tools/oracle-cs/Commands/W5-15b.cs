using System.Reflection;
using System.Text.Json.Nodes;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-15b
//     "libraw-version" => Commands.LibRawVersionCases(inputs),
// OracleCs.csproj needs one more <Reference Include="PhotoReview.Imaging.LibRaw"> (HintPath $(CSharpCoreDir)\PhotoReview.Imaging.LibRaw.dll,
// Private=true so it is copied next to oracle-cs.exe) and tools/oracle.ps1 must build that project next to PhotoReview.Imaging.
// LibRawAvailability is `public static` but every member this unit tests is `internal` (InternalsVisibleTo only names
// PhotoReview.Imaging.Tests), so the oracle calls them by reflection. libraw.dll is NEVER loaded: no op touches Probe/RunProbeCore or
// IsExactPinnedVersion (their results depend on the host). The static initializers used (RequiredExports) only read a string array.

// W5-15b: the pure, host independent half of LibRawAvailability. One command, one case per entry. Case = { name, op, ... }.
// ops (fields -> reported):
//   try-parse        version (string|null)                       -> { ok, major, minor }      (major/minor 0 when !ok, the C# out values)
//   check-exact      version (string|null)                       -> { value }
//   check-version    version (string|null), display (string)     -> { problem }               (string|null)
//   required-exports                                              -> { names:[..], count }      (ordinal sorted)
//   compute-exact    probe: "true"|"false"|"throw"|"oom",
//                    version: string|null|{ "throw": true },     -> { value, version_calls, error }   error: null | "OutOfMemoryException"
//                    (a thrown OutOfMemoryException escapes ComputeExactVersion; "throw" is a DllNotFoundException)
//   guard-probe      core: "ok"|"unavailable"|"throw"|"oom",
//                    message (string, for "throw")               -> { available, reason, error }
static partial class Commands
{
    private static readonly Lazy<Type> W515Availability = new(() =>
        Assembly.Load("PhotoReview.Imaging.LibRaw").GetType("PhotoReview.Imaging.LibRaw.LibRawAvailability", throwOnError: true)!);

    private static MethodInfo W515Method(string name) =>
        W515Availability.Value.GetMethod(name, BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)
        ?? throw new MissingMethodException(name);

    public static JsonArray LibRawVersionCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var name = c["name"]!.GetValue<string>();
            var op = c["op"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = name, ["op"] = op };
            switch (op)
            {
                case "try-parse":
                {
                    var args = new object?[] { W515Str(c["version"]), 0, 0 };
                    var ok = (bool)W515Method("TryParseVersion").Invoke(null, args)!;
                    o["ok"] = ok;
                    o["major"] = (int)args[1]!;
                    o["minor"] = (int)args[2]!;
                    break;
                }
                case "check-exact":
                    o["value"] = (bool)W515Method("CheckExactVersion").Invoke(null, [W515Str(c["version"])])!;
                    break;
                case "check-version":
                    o["problem"] = (string?)W515Method("CheckVersion").Invoke(null, [W515Str(c["version"]), c["display"]!.GetValue<string>()]);
                    break;
                case "required-exports":
                {
                    var names = (IReadOnlyList<string>)W515Availability.Value
                        .GetProperty("RequiredExports", BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)!.GetValue(null)!;
                    var arr = new JsonArray();
                    foreach (var n in names) arr.Add(n);
                    o["names"] = arr;
                    o["count"] = names.Count;
                    break;
                }
                case "compute-exact":
                {
                    var probeMode = c["probe"]!.GetValue<string>();
                    var versionNode = c["version"];
                    var versionThrows = versionNode is JsonObject;
                    var versionCalls = 0;
                    Func<bool> probe = () => probeMode switch
                    {
                        "true" => true,
                        "false" => false,
                        "throw" => throw new DllNotFoundException("libraw.dll"),
                        "oom" => throw new OutOfMemoryException(),
                        _ => throw new ArgumentException("probe " + probeMode),
                    };
                    Func<string?> version = () =>
                    {
                        versionCalls++;
                        if (versionThrows) throw new EntryPointNotFoundException("libraw_version");
                        return W515Str(versionNode);
                    };
                    try
                    {
                        o["value"] = (bool)W515Method("ComputeExactVersion").Invoke(null, [probe, version])!;
                        o["error"] = null;
                    }
                    catch (TargetInvocationException ex) when (ex.InnerException is OutOfMemoryException)
                    {
                        o["value"] = null;
                        o["error"] = "OutOfMemoryException";
                    }
                    o["version_calls"] = versionCalls;
                    break;
                }
                case "guard-probe":
                {
                    var mode = c["core"]!.GetValue<string>();
                    var message = c["message"]?.GetValue<string>() ?? "boom";
                    Func<(bool, string?)> core = () => mode switch
                    {
                        "ok" => (true, null),
                        "unavailable" => (false, message),
                        "throw" => throw new InvalidOperationException(message),
                        "oom" => throw new OutOfMemoryException(),
                        _ => throw new ArgumentException("core " + mode),
                    };
                    try
                    {
                        var (available, reason) = ((bool, string?))W515Method("GuardProbe").Invoke(null, [core])!;
                        o["available"] = available;
                        o["reason"] = reason;
                        o["error"] = null;
                    }
                    catch (TargetInvocationException ex) when (ex.InnerException is OutOfMemoryException)
                    {
                        o["available"] = null;
                        o["reason"] = null;
                        o["error"] = "OutOfMemoryException";
                    }
                    break;
                }
                default:
                    throw new ArgumentException("unknown op " + op);
            }
            result.Add(o);
        }
        return result;
    }

    private static string? W515Str(JsonNode? node) => node is null ? null : node.GetValue<string>();
}
