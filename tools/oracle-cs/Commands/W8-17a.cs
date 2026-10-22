using System.Globalization;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Shell.Win32.Startup;

// W8-17a oracle (the F half of the unit): ShellLaunch.Parse (+ ShellInstanceNames.Prefix) and ShellErrorLog (Format, Append, Configure/Report/CurrentPath, constants) of the
// baseline. NOT compiled or run by the writer (the lead does). DpiAwarenessBootstrap and ForwardedOpenHandler cannot be reached from the oracle (their seams
// IDpiAwarenessNative / IForwardedOpenTarget are `internal` interfaces of the Shell.Win32 assembly, which the oracle can neither implement nor proxy): their expectations
// are hand-written from DpiAwarenessBootstrapTests / ForwardedOpenHandlerTests (see contracts/W8-17a.md).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-17a
//     "shell-launch" => Commands.W817aShellLaunchCases(inputs),
//     "shell-error-log" => Commands.W817aShellErrorLogCases(inputs),
// Wiring (the lead does this, the writer does not run the oracle): OracleCs.csproj, inside the existing `<ItemGroup Condition="'$(CSharpSrcDir)' != ''">` (the W7-03d / W8-18b
// pattern), add three baseline files compiled into the oracle as they are (nothing copied by hand):
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\LaunchArguments.cs" Link="cs-baseline\LaunchArguments.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.Shell.Win32\Startup\ShellLaunch.cs" Link="cs-baseline\ShellLaunch.cs" />
//     <Compile Include="$(CSharpSrcDir)\src\PhotoReview.Shell.Win32\Startup\ShellErrorLog.cs" Link="cs-baseline\ShellErrorLog.cs" />
// LaunchArguments is `internal static` in App.Shared and ShellLaunch/ShellErrorLog are `internal` in Shell.Win32: compiled here they are visible to the oracle assembly.
// ShellLaunch.cs uses only System.IO and `LaunchArguments`; ShellErrorLog.cs only System.IO/Text/Globalization.
//
// Determinism: CurrentCulture is Invariant. The tokens "$ROOT" (a fresh temp root with the folder "photos", the folder "photos\sub", the files "a.jpg" and "photos\b.jpg"),
// "$DRIVE" (the root's drive designator, for example "C:") and "$DRIVEROOT" (for example "C:\") in an input are replaced by the oracle; outputs never contain them expanded
// (they are turned back into the tokens).
//
// Inputs (tests/golden-inputs/shell-launch.json):
//   "launch": [ { "id", "args": [string], "cwd": "$ROOT" | null } ]   -> ShellLaunch.Parse(args) with the current directory set to cwd (restored afterwards)
//   Output = the input case plus "kind": "launch", "arguments": [string], "initial_file", "initial_folder", "launch_folder", "initial_path" (string|null), "smoke" (bool),
//   "existing_paths": [string]; last element: { "kind": "constants", "prefix": ShellInstanceNames.Prefix, "smoke_argument": ShellLaunch.SmokeArgument }
// Inputs (tests/golden-inputs/shell-error-log.json):
//   "format":   [ { "id", "utc": "2026-10-13T06:07:08Z", "context", "exception": { "type", "message", "inner": { "type", "message" } | absent } } ]
//                -> "text": ShellErrorLog.Format(utc, context, exception) (the exception is never thrown, so it has no stack trace)
//   "thrown":  [ { "id", "depth": int, "message" } ] -> an InvalidOperationException thrown through `depth` nested calls: "frame_lines" (lines of Format that start with 4 spaces
//                and "at "), "max_frames" (ShellErrorLog.MaxStackFrames), "header_line" (first line, stack-independent)
//   "truncate": [ { "id", "lines": int, "pad": int, "limit": long } ] -> a file of `lines` lines "OLD-iii" + pad dots + "\n", then Append(path, "after", exception, limit):
//                "text_length", "under_limit" (bool), "starts_with_old" (bool), "has_after_tag" (bool), "first_old" (int, -1 none), "last_old" (int), "kept_old_lines" (int)
//   "unusable": [ { "id", "path_kind": "bad-drive" | "long-name" } ] -> "threw": bool (Append with an unusable path must never throw)
//   "report":   [ { "id", "configure": string|null, "reports": [context] } ] -> Configure(path) then Report(context, exception) for each; "current_path_set" (bool),
//                "lines": the header lines of the file with the time turned into "<utc>", "file_exists" (bool), "folder_created" (bool)
//   "constants": one case { "kind": "constants", "max_bytes", "max_message_chars", "max_stack_frames", "file_name" }
static partial class Commands
{
    private static string W817aExpand(string? text, string root)
    {
        var drive = Path.GetPathRoot(root)!;
        return (text ?? string.Empty)
            .Replace("$DRIVEROOT", drive, StringComparison.Ordinal)
            .Replace("$DRIVE", drive.TrimEnd('\\'), StringComparison.Ordinal)
            .Replace("$ROOT", root, StringComparison.Ordinal);
    }

    private static string? W817aCollapse(string? text, string root)
    {
        if (text is null)
        {
            return null;
        }

        var drive = Path.GetPathRoot(root)!;
        var collapsed = text.Replace(root, "$ROOT", StringComparison.Ordinal);
        return string.Equals(collapsed, drive, StringComparison.Ordinal) ? "$DRIVEROOT" : collapsed;
    }

    private static Exception W817aException(JsonObject spec)
    {
        var message = spec["message"]!.GetValue<string>();
        Exception? inner = spec["inner"] is JsonObject innerSpec ? W817aException(innerSpec) : null;
        return spec["type"]!.GetValue<string>() switch
        {
            "InvalidOperationException" => new InvalidOperationException(message, inner),
            "IOException" => new IOException(message, inner),
            "ArgumentException" => new ArgumentException(message, inner),
            "UnauthorizedAccessException" => new UnauthorizedAccessException(message, inner),
            "Exception" => new Exception(message, inner),
            var other => throw new InvalidOperationException("Unknown exception type in the input: " + other),
        };
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void W817aRecurse(int depth, string message)
    {
        if (depth == 0)
        {
            throw new InvalidOperationException(message);
        }

        W817aRecurse(depth - 1, message);
        GC.KeepAlive(message);
    }

    public static JsonArray W817aShellLaunchCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        var root = Path.Combine(Path.GetTempPath(), "oracle-w8-17a-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(root, "photos", "sub"));
        File.WriteAllText(Path.Combine(root, "a.jpg"), "x");
        File.WriteAllText(Path.Combine(root, "photos", "b.jpg"), "x");
        var previous = Environment.CurrentDirectory;
        try
        {
            foreach (var node in inputs["launch"]!.AsArray())
            {
                var input = node!.AsObject();
                var cwd = input["cwd"]?.GetValue<string>();
                Environment.CurrentDirectory = cwd is null ? previous : W817aExpand(cwd, root);
                var args = input["args"]!.AsArray().Select(a => W817aExpand(a!.GetValue<string>(), root)).ToArray();
                var launch = ShellLaunch.Parse(args);
                Environment.CurrentDirectory = previous;
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "launch";
                entry["arguments"] = new JsonArray(launch.Arguments.Select(a => (JsonNode?)JsonValue.Create(W817aCollapse(a, root))).ToArray());
                entry["initial_file"] = W817aCollapse(launch.InitialFile, root);
                entry["initial_folder"] = W817aCollapse(launch.InitialFolder, root);
                entry["launch_folder"] = W817aCollapse(launch.LaunchFolder, root);
                entry["initial_path"] = W817aCollapse(launch.InitialPath, root);
                entry["smoke"] = launch.Smoke;
                entry["existing_paths"] = new JsonArray(launch.ExistingPaths.Select(a => (JsonNode?)JsonValue.Create(W817aCollapse(a, root))).ToArray());
                result.Add(entry);
            }
        }
        finally
        {
            Environment.CurrentDirectory = previous;
            Directory.Delete(root, recursive: true);
        }

        result.Add(new JsonObject
        {
            ["kind"] = "constants",
            ["prefix"] = ShellInstanceNames.Prefix,
            ["smoke_argument"] = ShellLaunch.SmokeArgument,
        });
        return result;
    }

    public static JsonArray W817aShellErrorLogCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        var root = Path.Combine(Path.GetTempPath(), "oracle-w8-17a-log-" + Guid.NewGuid().ToString("N"));
        try
        {
            foreach (var node in inputs["format"]!.AsArray())
            {
                var input = node!.AsObject();
                var utc = DateTime.ParseExact(input["utc"]!.GetValue<string>(), "yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture,
                    DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal);
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "format";
                entry["text"] = ShellErrorLog.Format(utc, input["context"]!.GetValue<string>(), W817aException(input["exception"]!.AsObject()));
                result.Add(entry);
            }

            foreach (var node in inputs["thrown"]!.AsArray())
            {
                var input = node!.AsObject();
                var message = input["message"]!.GetValue<string>();
                Exception? thrown = null;
                try
                {
                    W817aRecurse(input["depth"]!.GetValue<int>(), message);
                }
                catch (InvalidOperationException ex)
                {
                    thrown = ex;
                }

                var text = ShellErrorLog.Format(new DateTime(2026, 10, 13, 6, 7, 8, DateTimeKind.Utc), "deep", thrown!);
                var lines = text.Split(Environment.NewLine);
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "thrown";
                entry["frame_lines"] = lines.Count(l => l.StartsWith("    at ", StringComparison.Ordinal));
                entry["max_frames"] = ShellErrorLog.MaxStackFrames;
                entry["header_line"] = lines[0];
                result.Add(entry);
            }

            foreach (var node in inputs["truncate"]!.AsArray())
            {
                var input = node!.AsObject();
                Directory.CreateDirectory(root);
                var path = Path.Combine(root, "truncate-" + Guid.NewGuid().ToString("N") + ".log");
                var pad = input["pad"]!.GetValue<int>();
                var old = string.Concat(Enumerable.Range(0, input["lines"]!.GetValue<int>())
                    .Select(i => "OLD-" + i.ToString("D3", CultureInfo.InvariantCulture) + new string('.', pad) + "\n"));
                File.WriteAllText(path, old);
                var limit = input["limit"]!.GetValue<long>();
                ShellErrorLog.Append(path, "after", new InvalidOperationException("fresh"), limit);
                var text = File.ReadAllText(path);
                var olds = Regex.Matches(text, @"^OLD-(\d{3})", RegexOptions.Multiline).Select(m => int.Parse(m.Groups[1].Value, CultureInfo.InvariantCulture)).ToArray();
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "truncate";
                entry["text_length"] = text.Length;
                entry["under_limit"] = text.Length < limit;
                entry["starts_with_old"] = text.StartsWith("OLD-", StringComparison.Ordinal);
                entry["has_after_tag"] = text.Contains("[after]", StringComparison.Ordinal);
                entry["first_old"] = olds.Length == 0 ? -1 : olds[0];
                entry["last_old"] = olds.Length == 0 ? -1 : olds[^1];
                entry["kept_old_lines"] = olds.Length;
                result.Add(entry);
            }

            foreach (var node in inputs["unusable"]!.AsArray())
            {
                var input = node!.AsObject();
                var path = input["path_kind"]!.GetValue<string>() switch
                {
                    "bad-drive" => "Z:\\no\\such\\drive\\<>|\\shell-errors.log",
                    _ => new string('a', 5000),
                };
                var threw = false;
                try
                {
                    ShellErrorLog.Append(path, "ctx", new InvalidOperationException("e"));
                }
                catch (Exception)
                {
                    threw = true;
                }

                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "unusable";
                entry["threw"] = threw;
                result.Add(entry);
            }

            foreach (var node in inputs["report"]!.AsArray())
            {
                var input = node!.AsObject();
                var configure = input["configure"]?.GetValue<string>();
                var path = configure is null ? null : W817aExpand(configure, root);
                ShellErrorLog.Configure(path);
                foreach (var context in input["reports"]!.AsArray())
                {
                    ShellErrorLog.Report(context!.GetValue<string>(), new InvalidOperationException("boom"));
                }

                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "report";
                entry["current_path_set"] = ShellErrorLog.CurrentPath is not null && string.Equals(ShellErrorLog.CurrentPath, path, StringComparison.Ordinal);
                entry["file_exists"] = path is not null && File.Exists(path);
                entry["folder_created"] = path is not null && Directory.Exists(Path.GetDirectoryName(path));
                var lines = path is not null && File.Exists(path)
                    ? File.ReadAllText(path).Split(Environment.NewLine, StringSplitOptions.RemoveEmptyEntries)
                        .Where(l => !l.StartsWith("    ", StringComparison.Ordinal))
                        .Select(l => Regex.Replace(l, @"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z ", "<utc> "))
                        .ToArray()
                    : [];
                entry["lines"] = new JsonArray(lines.Select(l => (JsonNode?)JsonValue.Create(l)).ToArray());
                ShellErrorLog.Configure(null);
                result.Add(entry);
            }
        }
        finally
        {
            ShellErrorLog.Configure(null);
            if (Directory.Exists(root))
            {
                Directory.Delete(root, recursive: true);
            }
        }

        result.Add(new JsonObject
        {
            ["kind"] = "constants",
            ["max_bytes"] = ShellErrorLog.MaxBytes,
            ["max_message_chars"] = ShellErrorLog.MaxMessageChars,
            ["max_stack_frames"] = ShellErrorLog.MaxStackFrames,
            ["file_name"] = ShellErrorLog.FileName,
        });
        return result;
    }
}
