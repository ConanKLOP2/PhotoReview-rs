using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.IO;
using PhotoReview.Core.Model;

// W3-07 golden package: RecoveryFileCheck (C# d92d6aae, src/PhotoReview.Core/FileActions/RecoveryFileCheck.cs).
// Copy to tools/oracle-cs/Commands/W3-07.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W3-07
//     "recovery-decide" => Commands.RecoveryDecide(inputs),
//     "recovery-check" => Commands.RecoveryCheck(inputs),
// Inputs: tests/golden-inputs/{recovery-decide,recovery-check}.json (regenerate with tools/gen_golden_inputs.py).
// Self-contained (no W2Util). Never touches the Recycle Bin: RecoveryFileCheck only stats.

static partial class Commands
{
    /// <summary>
    /// An IFileSystem that answers only DirectoryExists / GetFileStat (what RecoveryFileCheck is allowed to call) from a
    /// dictionary, with the same blank-path ArgumentException that PhysicalFileSystem and InMemoryFileSystem raise, and
    /// injectable exceptions. Any other member throws InvalidOperationException (never caught by the check: it would show
    /// up as {"throws": ...}, proving the check did something else).
    /// </summary>
    public class W307Fs : DispatchProxy
    {
        public Dictionary<string, (long Size, long Ticks)> Files { get; } = new(StringComparer.OrdinalIgnoreCase);
        public HashSet<string> Dirs { get; } = new(StringComparer.OrdinalIgnoreCase);
        public Dictionary<string, string> StatThrows { get; } = new(StringComparer.OrdinalIgnoreCase);
        public Dictionary<string, string> DirThrows { get; } = new(StringComparer.OrdinalIgnoreCase);
        public int StatCalls { get; private set; }

        public static string Key(string path) => path.Length > 3 ? path.TrimEnd('\\', '/') : path;

        protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
        {
            switch (targetMethod!.Name)
            {
                case nameof(IFileSystem.DirectoryExists):
                {
                    var path = (string)args![0]!;
                    ArgumentException.ThrowIfNullOrWhiteSpace(path);
                    if (DirThrows.TryGetValue(path, out var kind)) throw Make(kind);
                    return Dirs.Contains(Key(path));
                }
                case nameof(IFileSystem.GetFileStat):
                {
                    var path = (string)args![0]!;
                    StatCalls++;
                    ArgumentException.ThrowIfNullOrWhiteSpace(path);
                    if (StatThrows.TryGetValue(path, out var kind)) throw Make(kind);
                    return Files.TryGetValue(Key(path), out var f)
                        ? new FileStat(f.Size, new DateTime(f.Ticks, DateTimeKind.Utc))
                        : null;
                }
                default:
                    throw new InvalidOperationException("RecoveryFileCheck called IFileSystem." + targetMethod.Name);
            }
        }

        private static Exception Make(string kind) => kind switch
        {
            "UnauthorizedAccessException" => new UnauthorizedAccessException("denied"),
            "IOException" => new IOException("io"),
            "ArgumentException" => new ArgumentException("arg"),
            "NotSupportedException" => new NotSupportedException("ns"),
            "PathTooLongException" => new PathTooLongException("long"),
            "FileNotFoundException" => new FileNotFoundException("nf"),
            "InvalidOperationException" => new InvalidOperationException("not in the catch filter"),
            _ => throw new ArgumentException("unknown exception kind " + kind),
        };
    }

    private static DateTime W307Time(JsonNode? node) =>
        new(long.Parse(node!["ticks"]!.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture),
            Enum.Parse<DateTimeKind>(node["kind"]!.GetValue<string>(), ignoreCase: true));

    private static JournalEntry W307Entry(JsonObject o, Func<string?, string?> text) => new(
        o["id"]!.GetValue<string>(),
        Enum.Parse<FileOperationType>(o["type"]!.GetValue<string>()),
        Enum.Parse<JournalState>(o["state"]!.GetValue<string>()),
        text(o["source"]!.GetValue<string>())!,
        text(o["destination"]?.GetValue<string>()),
        o["size"]!.GetValue<long>(),
        W307Time(o["last_write"]),
        W307Time(o["timestamp"]),
        o["error"]?.GetValue<string>(),
        o["error_code"]?.GetValue<string>(),
        o["permanent"]?.GetValue<bool>(),
        o["undo"]?.GetValue<bool>(),
        o["group_id"]?.GetValue<string>(),
        o["group_members"]?.AsArray().Select(m => new JournalGroupMember(
            text(m!["source"]!.GetValue<string>())!,
            text(m["destination"]?.GetValue<string>()),
            m["size"]!.GetValue<long>(),
            W307Time(m["last_write"]),
            m["permanent"]!.GetValue<bool>())).ToList());

    private static JsonNode? W307PathJson(RecoveryPathCheck? p, Func<string, string> back) => p is null
        ? null
        : new JsonObject
        {
            ["path"] = back(p.Path),
            ["status"] = p.Status.ToString(),
            ["size"] = JsonValue.Create(p.CurrentSize),
            ["last_write_ticks"] = p.CurrentLastWriteUtc?.Ticks.ToString(System.Globalization.CultureInfo.InvariantCulture),
            ["folder_exists"] = p.FolderExists,
        };

    private static JsonObject W307ResultJson(RecoveryCheckResult r, Func<string, string> back)
    {
        var o = new JsonObject
        {
            ["verdict"] = r.Verdict.ToString(),
            ["source"] = W307PathJson(r.Source, back),
            ["destination"] = W307PathJson(r.Destination, back),
            ["destination_folder_missing"] = r.DestinationFolderMissing,
            ["is_group"] = r.IsGroup,
        };
        o["members"] = r.GroupMembers is null
            ? null
            : new JsonArray(r.GroupMembers.Select(m => (JsonNode?)new JsonObject
            {
                ["member_source"] = back(m.Member.Source),
                ["verdict"] = m.Verdict.ToString(),
                ["source"] = W307PathJson(m.Source, back),
                ["destination"] = W307PathJson(m.Destination, back),
                ["entry_is_the_group_entry"] = ReferenceEquals(m.Check.Entry, r.Entry),
                ["entry_has_members"] = m.Check.Entry.GroupMembers is not null,
            }).ToArray());
        return o;
    }

    /// <summary>
    /// RecoveryFileCheck.Decide (internal, reached by reflection) over the full product of the axes in the input:
    /// types x states x permanent x source status x destination status (null = no destination check). 720 cases.
    /// </summary>
    public static JsonArray RecoveryDecide(JsonObject inputs)
    {
        var decide = typeof(RecoveryFileCheck).GetMethod("Decide", BindingFlags.NonPublic | BindingFlags.Static)
            ?? throw new InvalidOperationException("RecoveryFileCheck.Decide not found");
        var stamp = new DateTime(2026, 1, 2, 3, 4, 5, DateTimeKind.Utc);
        var result = new JsonArray();
        foreach (var typeNode in inputs["types"]!.AsArray())
        foreach (var stateNode in inputs["states"]!.AsArray())
        foreach (var permNode in inputs["permanent"]!.AsArray())
        foreach (var sNode in inputs["source_statuses"]!.AsArray())
        foreach (var dNode in inputs["destination_statuses"]!.AsArray())
        {
            var type = Enum.Parse<FileOperationType>(typeNode!.GetValue<string>());
            var state = Enum.Parse<JournalState>(stateNode!.GetValue<string>());
            bool? permanent = permNode?.GetValue<bool>();
            var sStatus = Enum.Parse<RecoveryPathStatus>(sNode!.GetValue<string>());
            RecoveryPathStatus? dStatus = dNode is null ? null : Enum.Parse<RecoveryPathStatus>(dNode.GetValue<string>());
            var entry = new JournalEntry("id", type, state, @"C:\a.jpg", dStatus is null ? null : @"D:\a.jpg", 10, stamp, stamp,
                Permanent: permanent);
            var source = new RecoveryPathCheck(@"C:\a.jpg", sStatus, null, null, true);
            var destination = dStatus is null ? null : new RecoveryPathCheck(@"D:\a.jpg", dStatus.Value, null, null, true);
            var verdict = (RecoveryVerdict)decide.Invoke(null, [entry, source, destination])!;
            result.Add(new JsonObject
            {
                ["type"] = type.ToString(),
                ["state"] = state.ToString(),
                ["permanent"] = permNode is null ? null : permanent,
                ["source"] = sStatus.ToString(),
                ["destination"] = dStatus?.ToString(),
                ["verdict"] = verdict.ToString(),
            });
        }
        return result;
    }

    /// <summary>
    /// RecoveryFileCheck.Check for each case {name, mode: virtual|physical, entry, files:[{path,size,last_write_ticks}], dirs,
    /// stat_throws:{path:exception}, dir_throws:{path:exception}}. virtual = W307Fs (parents of files are directories);
    /// physical = the real PhysicalFileSystem over a private temp folder, "{root}" in any text is replaced by it (and mapped back
    /// in the output). Result: the verdict, both path checks, DestinationFolderMissing, IsGroup and the per-member checks; a throw
    /// that the check does not catch is {"throws": type}.
    /// </summary>
    public static JsonArray RecoveryCheck(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var name = c["name"]!.GetValue<string>();
            var physical = c["mode"]!.GetValue<string>() == "physical";
            string? root = null;
            try
            {
                IFileSystem fs;
                W307Fs? fake = null;
                Func<string?, string?> text = s => s;
                Func<string, string> back = s => s;
                if (physical)
                {
                    root = Path.Combine(Path.GetTempPath(), "pr-oracle-w307-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(root);
                    var r = root;
                    text = s => s?.Replace("{root}", r, StringComparison.Ordinal);
                    back = s => s.Replace(r, "{root}", StringComparison.Ordinal);
                    foreach (var d in c["dirs"]!.AsArray()) Directory.CreateDirectory(text(d!.GetValue<string>())!);
                    foreach (var f in c["files"]!.AsArray())
                    {
                        var path = text(f!["path"]!.GetValue<string>())!;
                        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
                        File.WriteAllBytes(path, new byte[f["size"]!.GetValue<long>()]);
                        File.SetLastWriteTimeUtc(path, new DateTime(
                            long.Parse(f["last_write_ticks"]!.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture),
                            DateTimeKind.Utc));
                    }
                    fs = new PhysicalFileSystem();
                }
                else
                {
                    var proxy = DispatchProxy.Create<IFileSystem, W307Fs>();
                    fake = (W307Fs)(object)proxy;
                    foreach (var f in c["files"]!.AsArray())
                    {
                        var path = f!["path"]!.GetValue<string>();
                        fake.Files[W307Fs.Key(path)] = (f["size"]!.GetValue<long>(),
                            long.Parse(f["last_write_ticks"]!.GetValue<string>(), System.Globalization.CultureInfo.InvariantCulture));
                        // Like InMemoryFileSystem.AddFile and the Rust FakeFileSystem: every ancestor folder exists.
                        for (var dir = Path.GetDirectoryName(path); !string.IsNullOrEmpty(dir); dir = Path.GetDirectoryName(dir))
                            fake.Dirs.Add(W307Fs.Key(dir));
                    }
                    foreach (var d in c["dirs"]!.AsArray())
                        for (var dir = d!.GetValue<string>(); !string.IsNullOrEmpty(dir); dir = Path.GetDirectoryName(dir))
                            fake.Dirs.Add(W307Fs.Key(dir));
                    if (c["stat_throws"] is JsonObject st) foreach (var kv in st) fake.StatThrows[kv.Key] = kv.Value!.GetValue<string>();
                    if (c["dir_throws"] is JsonObject dt) foreach (var kv in dt) fake.DirThrows[kv.Key] = kv.Value!.GetValue<string>();
                    fs = proxy;
                }

                var entry = W307Entry(c["entry"]!.AsObject(), text);
                var json = W307ResultJson(new RecoveryFileCheck(fs).Check(entry), back);
                json["name"] = name;
                json["mode"] = c["mode"]!.GetValue<string>();
                json["stat_calls"] = fake?.StatCalls;
                result.Add(json);
            }
            catch (Exception ex)
            {
                result.Add(new JsonObject { ["name"] = name, ["mode"] = c["mode"]!.GetValue<string>(), ["throws"] = ex.GetType().Name });
            }
            finally
            {
                if (root is not null) { try { Directory.Delete(root, recursive: true); } catch { } }
            }
        }
        return result;
    }
}
