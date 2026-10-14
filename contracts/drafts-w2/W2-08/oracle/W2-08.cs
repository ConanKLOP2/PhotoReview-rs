using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W2-08
//     "destination-policy" => Commands.DestinationPolicy(inputs),
// Needs W2-00.cs (class W2Util).

// W2-08: ActionDestinationPolicy.Validate / ValidatePickedFolder / ValidateNoEscapeViaReparsePoint
public class W208FsProxy : DispatchProxy
{
    /// <summary>Normalized (full path, no trailing separator) reparse point -> the real target it "points to" (SEC-01 simulation).</summary>
    public Dictionary<string, string> Reparse { get; } = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>Paths starting with this text make ResolveRealPath throw ThrowKind ("io" or "unauthorized").</summary>
    public string? ThrowPrefix { get; set; }

    public string? ThrowKind { get; set; }

    public static string Key(string path) => Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));

    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
    {
        if (targetMethod!.Name != nameof(IFileSystem.ResolveRealPath)) throw new NotSupportedException(targetMethod.Name);
        var path = (string)args![0]!;
        if (ThrowPrefix is not null && path.StartsWith(ThrowPrefix, StringComparison.OrdinalIgnoreCase))
        {
            throw ThrowKind == "unauthorized"
                ? new UnauthorizedAccessException("denied")
                : new IOException("The name of the file cannot be resolved by the system.");
        }

        // Same walk as the test InMemoryFileSystem.ResolveRealPath (segment by segment, substituting registered links).
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        var full = Path.GetFullPath(path);
        var root = Path.GetPathRoot(full) ?? string.Empty;
        if (string.IsNullOrEmpty(root)) return full;
        var relative = full[root.Length..];
        var segments = relative.Split([Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar], StringSplitOptions.RemoveEmptyEntries);
        if (segments.Length == 0) return root;
        var current = root.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        foreach (var segment in segments)
        {
            var candidate = current.Length == 0 ? segment : current + Path.DirectorySeparatorChar + segment;
            current = Reparse.TryGetValue(Key(candidate), out var target) ? target : candidate;
        }
        return current;
    }
}

static partial class Commands
{
    /// <summary>
    /// Three lists: "validate":[destination|null] => Validate; "picked":[{destination, photo_folder, existing:[...]}] =>
    /// ValidatePickedFolder (with the strings the existence callback was asked about); "no_escape":[{name, source, destination,
    /// reparse:{link:target}, throw_prefix?, throw_kind?}] => ValidateNoEscapeViaReparsePoint on a fake IFileSystem whose only working
    /// member is ResolveRealPath.
    /// </summary>
    public static JsonArray DestinationPolicy(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var d in W2Util.NullableStrings(inputs["validate"]))
        {
            result.Add(new JsonObject
            {
                ["kind"] = "validate",
                ["destination"] = d,
                ["result"] = W2Util.Capture(() => ActionDestinationPolicy.Validate(d).ToString()),
            });
        }

        foreach (var node in inputs["picked"]!.AsArray())
        {
            var c = node!.AsObject();
            var destination = W2Util.Str(c["destination"]);
            var photo = c["photo_folder"]!.GetValue<string>();
            var existing = new HashSet<string>(W2Util.Strings(c["existing"]), StringComparer.OrdinalIgnoreCase);
            var asked = new List<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "picked",
                ["destination"] = destination,
                ["photo_folder"] = photo,
                ["existing"] = c["existing"]!.DeepClone(),
                ["result"] = W2Util.Capture(() => ActionDestinationPolicy.ValidatePickedFolder(destination, photo, p => { asked.Add(p); return existing.Contains(p); }).ToString()),
                ["asked"] = W2Util.StrArray(asked),
            });
        }

        foreach (var node in inputs["no_escape"]!.AsArray())
        {
            var c = node!.AsObject();
            var fs = DispatchProxy.Create<IFileSystem, W208FsProxy>();
            var proxy = (W208FsProxy)(object)fs;
            if (c["reparse"] is JsonObject links)
            {
                foreach (var (link, target) in links) proxy.Reparse[W208FsProxy.Key(link)] = target!.GetValue<string>();
            }
            proxy.ThrowPrefix = W2Util.Str(c["throw_prefix"]);
            proxy.ThrowKind = W2Util.Str(c["throw_kind"]);
            var source = c["source"]!.GetValue<string>();
            var destination = c["destination"]!.GetValue<string>();
            result.Add(new JsonObject
            {
                ["kind"] = "no_escape",
                ["name"] = c["name"]!.GetValue<string>(),
                ["source"] = source,
                ["destination"] = destination,
                ["reparse"] = c["reparse"]?.DeepClone(),
                ["throw_prefix"] = proxy.ThrowPrefix,
                ["throw_kind"] = proxy.ThrowKind,
                ["result"] = W2Util.Capture(() => ActionDestinationPolicy.ValidateNoEscapeViaReparsePoint(source, destination, fs).ToString()),
            });
        }
        return result;
    }
}
