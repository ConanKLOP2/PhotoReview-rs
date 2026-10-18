using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PhotoReview.Benchmark.Cli;
using PhotoReview.Benchmarking;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Model;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W7-03d
//     "cli-arguments" => Commands.CliArguments(inputs),
//     "cli-tool-path-guard" => Commands.CliToolPathGuard(inputs),
//     "cli-dataset-plan" => Commands.CliDatasetPlan(inputs),
//     "cli-process" => Commands.CliProcess(inputs),
// Needs W2-00.cs (class W2Util).
//
// Wiring (lead, once; this file does not change the C# repo):
//  1. tools/oracle-cs/OracleCs.csproj: add `<Reference Include="PhotoReview.Benchmarking"><HintPath>$(CSharpCoreDir)\PhotoReview.Benchmarking.dll</HintPath></Reference>`
//     and compile the two INTERNAL classes of the CLI project into the oracle (they only need Core + Benchmarking):
//       <Compile Include="$(CSharpSrcDir)\tools\PhotoReview.Benchmark.Cli\BenchmarkCliArguments.cs" Link="cs-baseline\BenchmarkCliArguments.cs" />
//       <Compile Include="$(CSharpSrcDir)\tools\PhotoReview.Benchmark.Cli\ToolPathGuard.cs" Link="cs-baseline\ToolPathGuard.cs" />
//     (tools/oracle.ps1 already extracts the baseline into $work\src; add `src\PhotoReview.Benchmarking` to its `dotnet build` list and pass
//     -p:CSharpSrcDir=<$work\src>). Nothing is copied by hand: the oracle runs the baseline source.
//  2. `cli-process` runs the real baseline executable: tools/oracle.ps1 builds `tools\PhotoReview.Benchmark.Cli` (Release, WPF app graph, slow)
//     into $work\cli and sets PHOTOREVIEW_ORACLE_BENCH_CLI to its PhotoReview.Benchmark.Cli.exe. Without the variable the command throws.
//
// W7-03d: BenchmarkCliArguments, ToolPathGuard, the dataset step of RunCliBenchmarksAsync (Program.cs) and the process level behaviour of Program.cs.
static partial class Commands
{
    private static JsonNode Outcome(Func<JsonNode?> f, Func<string, string>? scrub = null)
    {
        try
        {
            return new JsonObject { ["ok"] = f() };
        }
        catch (Exception ex)
        {
            return new JsonObject { ["throws"] = ex.GetType().Name, ["message"] = scrub is null ? ex.Message : scrub(ex.Message) };
        }
    }

    private static JsonArray W703Ints(IEnumerable<int> items) => new(items.Select(i => (JsonNode?)JsonValue.Create(i)).ToArray());

    private static Exception? W703Exception(string name) => name switch
    {
        "Exception" => new Exception("x"),
        "ArgumentException" => new ArgumentException("x"),
        "ArgumentNullException" => new ArgumentNullException("x"),
        "ArgumentOutOfRangeException" => new ArgumentOutOfRangeException("x"),
        "BenchmarkProfileException" => new BenchmarkProfileException(BenchmarkProfileProblem.InvalidSettings, "x", "x"),
        "IOException" => new IOException("x"),
        "FileNotFoundException" => new FileNotFoundException("x"),
        "DirectoryNotFoundException" => new DirectoryNotFoundException("x"),
        "PathTooLongException" => new PathTooLongException("x"),
        "EndOfStreamException" => new EndOfStreamException("x"),
        "FileLoadException" => new FileLoadException("x"),
        "DriveNotFoundException" => new DriveNotFoundException("x"),
        "InvalidOperationException" => new InvalidOperationException("x"),
        "ObjectDisposedException" => new ObjectDisposedException("x"),
        "NotSupportedException" => new NotSupportedException("x"),
        "PlatformNotSupportedException" => new PlatformNotSupportedException("x"),
        "UnauthorizedAccessException" => new UnauthorizedAccessException("x"),
        "FormatException" => new FormatException("x"),
        "InvalidDataException" => new InvalidDataException("x"),
        "OperationCanceledException" => new OperationCanceledException("x"),
        "TaskCanceledException" => new TaskCanceledException("x"),
        "OutOfMemoryException" => new OutOfMemoryException("x"),
        "JsonException" => new JsonException("x"),
        "NullReferenceException" => new NullReferenceException("x"),
        "InvalidCastException" => new InvalidCastException("x"),
        "TimeoutException" => new TimeoutException("x"),
        "KeyNotFoundException" => new KeyNotFoundException("x"),
        "AggregateException" => new AggregateException("x"),
        "InvalidProgramException" => new InvalidProgramException("x"),
        _ => null,
    };

    /// <summary>
    /// BenchmarkCliArguments, one case {name, op, ...} each. Ops: widths{text}, positive_int{text,name}, int_range{text,name,min,max},
    /// positive_double{text,name}, defined_enum{text,name,enum: LoadingMode|ImageSortMode}, resolve_profiles{mode,args}, resolve_output{mode,args},
    /// perf_rules{args}, surplus{args}, expected_failure{type}. The result is {"ok": value} or {"throws": type, "message": text}.
    /// </summary>
    public static JsonArray CliArguments(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var op = c["op"]!.GetValue<string>();
            var text = W2Util.Str(c["text"]);
            var name = W2Util.Str(c["name_arg"]) ?? "n";
            var args = W2Util.Strings(c["args"]);
            var mode = W2Util.Str(c["mode"]) ?? "";
            JsonNode outcome;
            JsonNode? extra = null;
            switch (op)
            {
                case "widths":
                    outcome = Outcome(() => W703Ints(BenchmarkCliArguments.ParseWidths(text!)));
                    break;
                case "positive_int":
                    outcome = Outcome(() => BenchmarkCliArguments.ParsePositiveInt(text!, name));
                    break;
                case "int_range":
                    outcome = Outcome(() => BenchmarkCliArguments.ParseIntInRange(text!, name, c["min"]!.GetValue<int>(), c["max"]!.GetValue<int>()));
                    break;
                case "positive_double":
                    // "R" round-trip text: the Rust test compares the parsed number, not the formatting.
                    outcome = Outcome(() => BenchmarkCliArguments.ParsePositiveFiniteDouble(text!, name));
                    break;
                case "defined_enum":
                    if (c["enum"]!.GetValue<string>() == "LoadingMode")
                    {
                        outcome = Outcome(() => (int)BenchmarkCliArguments.ParseDefinedEnum<LoadingMode>(text!, name));
                        extra = W2Util.StrArray(Enum.GetNames<LoadingMode>().Select((n, i) => n + "=" + (int)Enum.GetValues<LoadingMode>()[i]));
                    }
                    else
                    {
                        outcome = Outcome(() => (int)BenchmarkCliArguments.ParseDefinedEnum<ImageSortMode>(text!, name));
                        extra = W2Util.StrArray(Enum.GetNames<ImageSortMode>().Select((n, i) => n + "=" + (int)Enum.GetValues<ImageSortMode>()[i]));
                    }
                    break;
                case "resolve_profiles":
                    outcome = Outcome(() => W2Util.StrArray(BenchmarkCliArguments.ResolveProfiles(mode, args).Select(p => p.Id)));
                    break;
                case "resolve_output":
                    outcome = Outcome(() => BenchmarkCliArguments.ResolveOutput(mode, args));
                    break;
                case "perf_rules":
                    outcome = Outcome(() => BenchmarkCliArguments.ParsePerfAnalyzeRules(args));
                    break;
                case "surplus":
                    outcome = Outcome(() => { BenchmarkCliArguments.RejectSurplusBenchmarkArguments(args); return null; });
                    break;
                case "expected_failure":
                    var typeName = c["type"]!.GetValue<string>();
                    var ex = W703Exception(typeName) ?? throw new ArgumentException("unknown exception type " + typeName);
                    outcome = Outcome(() => BenchmarkCliArguments.IsExpectedToolFailure(ex));
                    break;
                default:
                    throw new ArgumentException("unknown op " + op);
            }

            var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op, ["result"] = outcome };
            if (extra is not null) row["members"] = extra;
            result.Add(row);
        }

        // Catalog facts the Rust fake catalog must mirror (ids in registry order), reported once.
        result.Add(new JsonObject
        {
            ["name"] = "catalog",
            ["op"] = "catalog",
            ["all"] = W2Util.StrArray(BenchmarkProfiles.All.Select(p => p.Id)),
            ["runnable"] = W2Util.StrArray(BenchmarkProfiles.Runnable.Select(p => p.Id)),
            ["file_action"] = W2Util.StrArray(BenchmarkProfiles.All.Where(p => p.Workload == BenchmarkWorkload.FileAction).Select(p => p.Id)),
        });
        return result;
    }

    // ---- ToolPathGuard on a real temp tree ----------------------------------------------------------------------

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateHardLinkW(string newFileName, string existingFileName, IntPtr securityAttributes);

    private static string W703Path(string root, string? relative) =>
        relative is null ? "" : relative.Length == 0 ? root : relative.StartsWith('@') ? ToolPathGuard.AppDataFolder() + relative[1..] : Path.Combine(root, relative.Replace('/', '\\'));

    private static void W703Build(string root, JsonNode? tree, List<string> links)
    {
        if (tree is null) return;
        foreach (var entry in tree.AsArray())
        {
            if (entry is JsonObject o)
            {
                // {"junction": "link", "target": "photos"}: a real junction (symlink needs privilege).
                var link = W703Path(root, o["junction"]!.GetValue<string>());
                var target = W703Path(root, o["target"]!.GetValue<string>());
                using var p = Process.Start(new ProcessStartInfo("cmd.exe", $"/c mklink /J \"{link}\" \"{target}\"")
                {
                    UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true,
                })!;
                p.WaitForExit(30_000);
                if (p.ExitCode != 0) throw new InvalidOperationException("mklink failed");
                links.Add(link);
                continue;
            }

            var rel = entry!.GetValue<string>();
            var full = W703Path(root, rel.TrimEnd('/'));
            if (rel.EndsWith('/')) Directory.CreateDirectory(full);
            else
            {
                Directory.CreateDirectory(Path.GetDirectoryName(full)!);
                File.WriteAllBytes(full, [1, 2, 3]);
            }
        }
    }

    private static string W703Scrub(string text, string root) =>
        Regex.Replace(text.Replace(root, "{root}", StringComparison.OrdinalIgnoreCase)
            .Replace(ToolPathGuard.AppDataFolder(), "{appdata}", StringComparison.OrdinalIgnoreCase), @"\.[0-9a-f]{8}\.tmp", ".{tmp}.tmp");

    private static void W703Cleanup(string root, List<string> links)
    {
        foreach (var link in links)
        {
            try { Directory.Delete(link); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }

        try { Directory.Delete(root, recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }

    /// <summary>
    /// ToolPathGuard / ResolveReportDirectory on a temp tree. Case {name, tree:[ "dir/", "dir/file.jpg", {"junction":"link","target":"photos"} ], op, ...}; every path
    /// argument is relative to the temp root ("" = the root itself, "@x" = AppDataFolder()+x). Ops: output_directory{out,photo}, output_file{file,photo},
    /// overlaps{a,b}, same_or_under{path,root}, cache_directory{cache,must:[{path,name}]}, claim_cache{cache}, report_directory{photo,override|override_raw|null},
    /// write_report{path,content,link:none|hardlink|symlink,photo}. Results use {root}/{appdata} instead of the machine specific folders.
    /// </summary>
    public static JsonArray CliToolPathGuard(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-w703d-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            var links = new List<string>();
            try
            {
                W703Build(root, c["tree"], links);
                string P(string key) => W703Path(root, W2Util.Str(c[key]));
                Func<string, string> scrub = s => W703Scrub(s, root);
                var op = c["op"]!.GetValue<string>();
                JsonNode outcome;
                JsonNode? after = null;
                switch (op)
                {
                    case "output_directory":
                        outcome = Outcome(() => { ToolPathGuard.EnsureOutputDirectory(P("out"), P("photo")); return null; }, scrub);
                        break;
                    case "output_file":
                        outcome = Outcome(() => { ToolPathGuard.EnsureOutputFile(P("file"), P("photo")); return null; }, scrub);
                        break;
                    case "overlaps":
                        outcome = Outcome(() => ToolPathGuard.Overlaps(P("a"), P("b")), scrub);
                        break;
                    case "same_or_under":
                        outcome = Outcome(() => ToolPathGuard.IsSameOrUnder(P("path"), P("root")), scrub);
                        break;
                    case "cache_directory":
                        var must = c["must"]!.AsArray().Select(m => (W703Path(root, m!["path"]!.GetValue<string>()), m["name"]!.GetValue<string>())).ToArray();
                        outcome = Outcome(() => { ToolPathGuard.EnsureCacheDirectory(P("cache"), must); return null; }, scrub);
                        break;
                    case "claim_cache":
                        outcome = Outcome(() => { ToolPathGuard.ClaimCacheDirectory(P("cache")); ToolPathGuard.ClaimCacheDirectory(P("cache")); return null; }, scrub);
                        var marker = Path.Combine(P("cache"), ToolPathGuard.CacheMarker);
                        after = new JsonObject
                        {
                            ["marker_exists"] = File.Exists(marker),
                            ["marker_text"] = File.Exists(marker) ? File.ReadAllText(marker) : null,
                            ["marker_bytes"] = File.Exists(marker) ? File.ReadAllBytes(marker).Length : null,
                        };
                        break;
                    case "report_directory":
                        var photo = P("photo");
                        var overrideText = W2Util.Str(c["override"]);
                        var overridePath = W2Util.Str(c["override_raw"]) ?? (overrideText is null ? null : W703Path(root, overrideText)); // override_raw: the literal text ("" and " " included)
                        outcome = Outcome(() =>
                        {
                            var dir = BenchmarkCliArguments.ResolveReportDirectory(photo, overridePath);
                            if (!string.IsNullOrEmpty(overridePath)) return dir;
                            // Default folder: only its shape is stable (timestamp and guid differ per run).
                            var prefix = Path.Combine(Path.GetTempPath(), "PhotoReview-Benchmark-Reports") + Path.DirectorySeparatorChar;
                            var tail = dir.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) ? dir[prefix.Length..] : "<outside>";
                            return Regex.IsMatch(tail, @"^\d{8}-\d{6}-[0-9a-f]{6}$") ? "default-shape-ok" : tail;
                        }, scrub);
                        if (outcome["ok"]?.GetValue<string>() is { } okText) outcome["ok"] = scrub(okText);
                        after = new JsonObject { ["exists"] = !string.IsNullOrEmpty(overridePath) && Directory.Exists(overridePath) };
                        break;
                    case "write_report":
                        var path = P("path");
                        var link = c["link"]?.GetValue<string>() ?? "none";
                        var photoFile = c["photo"] is null ? null : P("photo");
                        var skipped = false;
                        if (link == "hardlink") CreateHardLinkW(path, photoFile!, IntPtr.Zero);
                        else if (link == "symlink")
                        {
                            try { File.CreateSymbolicLink(path, photoFile!); } catch (Exception e) when (e is IOException or UnauthorizedAccessException) { skipped = true; }
                        }

                        outcome = skipped
                            ? new JsonObject { ["ok"] = "skipped-no-symlink-privilege" }
                            : Outcome(() => { ToolPathGuard.WriteReportFileAsync(path, c["content"]!.GetValue<string>()).GetAwaiter().GetResult(); return null; }, scrub);
                        after = new JsonObject
                        {
                            ["report_bytes"] = File.Exists(path) ? W703Ints(File.ReadAllBytes(path).Select(b => (int)b)) : null,
                            ["photo_bytes"] = photoFile is not null && File.Exists(photoFile) ? W703Ints(File.ReadAllBytes(photoFile).Select(b => (int)b)) : null,
                            ["is_link"] = File.Exists(path) && new FileInfo(path).LinkTarget is not null,
                            ["leftover_tmp"] = W2Util.StrArray(Directory.Exists(Path.GetDirectoryName(path)) ? Directory.EnumerateFiles(Path.GetDirectoryName(path)!, "*.tmp").Select(Path.GetFileName) : []),
                        };
                        break;
                    default:
                        throw new ArgumentException("unknown op " + op);
                }

                var row = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["op"] = op, ["result"] = outcome };
                if (after is not null) row["after"] = after;
                result.Add(row);
            }
            finally
            {
                W703Cleanup(root, links);
            }
        }

        return result;
    }

    /// <summary>
    /// The file selection step of RunCliBenchmarksAsync (Program.cs, copied expression by expression, there is no callable seam in the top-level program;
    /// the process golden below pins the same code end to end). Case {name, files:[absolute paths]}: no disk access.
    /// </summary>
    public static JsonArray CliDatasetPlan(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var listing = W2Util.Strings(c["files"]);
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["files"] = W2Util.StrArray(listing),
                ["result"] = Outcome(() =>
                {
                    var supported = ImageFileTypes.SupportedExtensions;
                    var allFiles = listing
                        .OrderBy(Path.GetFileName, StringComparer.OrdinalIgnoreCase).ThenBy(p => p, StringComparer.Ordinal)
                        .ToArray();
                    var unsupportedExtensions = allFiles
                        .Where(p => !supported.Contains(Path.GetExtension(p)))
                        .GroupBy(p => string.IsNullOrWhiteSpace(Path.GetExtension(p)) ? "<none>" : Path.GetExtension(p).ToLowerInvariant(), StringComparer.Ordinal)
                        .OrderBy(g => g.Key, StringComparer.Ordinal)
                        .ToDictionary(g => g.Key, g => g.Count(), StringComparer.Ordinal);
                    var files = allFiles.Where(p => supported.Contains(Path.GetExtension(p))).Take(64).ToArray();
                    if (files.Length == 0) throw new InvalidOperationException("Benchmark folder contains no supported images");
                    return new JsonObject
                    {
                        ["files"] = W2Util.StrArray(files.Select(Path.GetFullPath)),
                        ["unsupported"] = new JsonArray(unsupportedExtensions.Select(kv => (JsonNode?)new JsonArray(kv.Key, kv.Value)).ToArray()),
                    };
                }),
            });
        }

        return result;
    }

    private static void W703Listing(string root, string dir, List<string> lines)
    {
        foreach (var entry in new DirectoryInfo(dir).EnumerateFileSystemInfos().OrderBy(e => e.Name, StringComparer.Ordinal))
        {
            var rel = Path.GetRelativePath(root, entry.FullName).Replace('\\', '/');
            if (entry.Attributes.HasFlag(FileAttributes.ReparsePoint)) lines.Add(rel + "@");
            else if (entry is DirectoryInfo)
            {
                lines.Add(rel + "/");
                W703Listing(root, entry.FullName, lines);
            }
            else lines.Add(rel);
        }
    }

    /// <summary>
    /// Runs the BASELINE Benchmark.Cli executable (PHOTOREVIEW_ORACLE_BENCH_CLI) with {name, tree, args} ("{root}" in an argument = the temp root) and reports
    /// stdout / stderr exactly as bytes decoded as UTF-8 (CRLF kept), the exit code and the tree afterwards ("/" directory, "@" reparse point). Only invocations
    /// that finish BEFORE any image is measured are allowed: list, usage, argument errors, missing folder, guard refusals, folder without supported images.
    /// </summary>
    public static JsonArray CliProcess(JsonObject inputs)
    {
        var exe = Environment.GetEnvironmentVariable("PHOTOREVIEW_ORACLE_BENCH_CLI")
                  ?? throw new InvalidOperationException("PHOTOREVIEW_ORACLE_BENCH_CLI is not set (tools/oracle.ps1 builds PhotoReview.Benchmark.Cli and sets it)");
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var root = Path.Combine(Path.GetTempPath(), "pr-oracle-w703d-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            var links = new List<string>();
            try
            {
                W703Build(root, c["tree"], links);
                var args = W2Util.Strings(c["args"]).Select(a => a.Replace("{root}", root, StringComparison.Ordinal)).ToList();
                var info = new ProcessStartInfo(exe)
                {
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    CreateNoWindow = true,
                    WorkingDirectory = root,
                    StandardOutputEncoding = new UTF8Encoding(false),
                    StandardErrorEncoding = new UTF8Encoding(false),
                };
                foreach (var a in args) info.ArgumentList.Add(a);
                using var process = Process.Start(info)!;
                var stdoutTask = process.StandardOutput.ReadToEndAsync();
                var stderrTask = process.StandardError.ReadToEndAsync();
                if (!process.WaitForExit(120_000))
                {
                    process.Kill(true);
                    throw new TimeoutException("baseline CLI did not finish: " + c["name"]);
                }

                var lines = new List<string>();
                W703Listing(root, root, lines);
                result.Add(new JsonObject
                {
                    ["name"] = c["name"]!.GetValue<string>(),
                    ["args"] = c["args"]!.DeepClone(),
                    ["exit_code"] = process.ExitCode,
                    ["stdout"] = W703Scrub(stdoutTask.GetAwaiter().GetResult(), root),
                    ["stderr"] = W703Scrub(stderrTask.GetAwaiter().GetResult(), root),
                    ["tree_after"] = W2Util.StrArray(lines),
                });
            }
            finally
            {
                W703Cleanup(root, links);
            }
        }

        return result;
    }
}
