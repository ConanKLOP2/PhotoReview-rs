// W2-12 golden package: SessionStore / SessionJsonContext (C# d92d6aae).
// Copy to tools/oracle-cs/Commands/W2-12.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W2-12
//     "session-key" => Commands.SessionKeyCases(inputs),
//     "session-json-write" => Commands.SessionJsonWriteCases(inputs),
//     "session-json-read" => Commands.SessionJsonReadCases(inputs),
// Inputs: tests/golden-inputs/{session-key,session-json-write,session-json-read}.json (ready to copy from golden-inputs/).
using System.Globalization;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.IO;
using PhotoReview.Core.Session;

static partial class Commands
{
    private sealed class SessionOraclePaths(string sessions) : IAppPaths
    {
        public string ConfigFile => @"C:\data\config.json";
        public string JournalFile => @"C:\data\operations.jsonl";
        public string SessionsDir => sessions;
        public string LogFile => @"C:\data\logs\app.log";
        public string PreviewCacheDir => @"C:\data\cache";
        public string ThumbnailCacheDir => @"C:\data\thumbnails";
        public string WindowPlacementFile => @"C:\data\window-placement.json";
    }

    private sealed class SessionOracleLog : ILog
    {
        public List<string> Warnings { get; } = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) => Warnings.Add(message);
        public void Error(string message, Exception? ex = null) { }
    }

    private static string SessionTempRoot()
    {
        var root = Path.Combine(Path.GetTempPath(), "pr-oracle-session-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        return root;
    }

    /// <summary>
    /// SessionStore.CanonicalFolder (internal, via reflection) and GetPath: the identity every spelling of a folder shares and the
    /// session file name derived from it. "canonical" is null when .NET throws ("exception" names the type). "upper" is
    /// canonical.ToUpperInvariant() (what is hashed); "key" is the file name without ".json" (SHA-256 hex, uppercase).
    /// </summary>
    public static JsonArray SessionKeyCases(JsonObject inputs)
    {
        var canonicalMethod = typeof(SessionStore).GetMethod("CanonicalFolder", BindingFlags.NonPublic | BindingFlags.Static)!;
        var root = SessionTempRoot();
        try
        {
            var store = new SessionStore(new SessionOraclePaths(Path.Combine(root, "Sessions")), new PhysicalFileSystem());
            var result = new JsonArray();
            foreach (var node in inputs["folders"]!.AsArray())
            {
                var folder = node!.GetValue<string>();
                var item = new JsonObject { ["folder"] = folder };
                try
                {
                    var canonical = (string)canonicalMethod.Invoke(null, [folder])!;
                    item["canonical"] = canonical;
                    item["upper"] = canonical.ToUpperInvariant();
                }
                catch (TargetInvocationException ex)
                {
                    item["canonical"] = null;
                    item["canonical_exception"] = ex.InnerException!.GetType().Name;
                }
                try
                {
                    var path = store.GetPath(folder);
                    var name = Path.GetFileName(path);
                    item["key"] = name.EndsWith(".json", StringComparison.Ordinal) ? name[..^5] : name;
                    item["dir_ok"] = string.Equals(Path.GetDirectoryName(path), Path.Combine(root, "Sessions"), StringComparison.Ordinal);
                }
                catch (Exception ex)
                {
                    item["key"] = null;
                    item["exception"] = ex.GetType().Name;
                }
                result.Add(item);
            }
            return result;
        }
        finally { try { Directory.Delete(root, true); } catch (IOException) { } }
    }

    /// <summary>
    /// SessionStore.Save: the exact file text (WriteIndented, default encoder, DateTime as ISO-8601) and the file name. Input case:
    /// name, folder, currentPath (string or null), skipped (strings), kind ("utc" or "unspecified"), ticks.
    /// </summary>
    public static JsonArray SessionJsonWriteCases(JsonObject inputs)
    {
        var root = SessionTempRoot();
        try
        {
            var store = new SessionStore(new SessionOraclePaths(Path.Combine(root, "Sessions")), new PhysicalFileSystem());
            var result = new JsonArray();
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var item = new JsonObject
                {
                    ["name"] = node!["name"]!.DeepClone(),
                    ["folder"] = node["folder"]!.DeepClone(),
                    ["currentPath"] = node["currentPath"]?.DeepClone(),
                    ["skipped"] = node["skipped"]!.DeepClone(),
                    ["kind"] = node["kind"]!.DeepClone(),
                    ["ticks"] = node["ticks"]!.DeepClone(),
                };
                var kind = node["kind"]!.GetValue<string>() == "utc" ? DateTimeKind.Utc : DateTimeKind.Unspecified;
                var state = new SessionState
                {
                    Folder = node["folder"]!.GetValue<string>(),
                    CurrentPath = node["currentPath"]?.GetValue<string>(),
                    Skipped = [.. node["skipped"]!.AsArray().Select(s => s!.GetValue<string>())],
                    UpdatedUtc = new DateTime(long.Parse(node["ticks"]!.GetValue<string>(), CultureInfo.InvariantCulture), kind),
                };
                store.Save(state);
                var path = store.GetPath(state.Folder);
                item["file_name"] = Path.GetFileName(path);
                item["text"] = File.ReadAllText(path);
                item["bytes_hex"] = Convert.ToHexString(File.ReadAllBytes(path));
                result.Add(item);
            }
            return result;
        }
        finally { try { Directory.Delete(root, true); } catch (IOException) { } }
    }

    /// <summary>
    /// SessionStore.Load of arbitrary file content for folder C:\photos\vacation. Input case: name and either "text" (written as UTF-8
    /// without BOM) or "hex" (raw bytes). Output: folder, currentPath, skipped (nulls kept), updated_kind ("Utc", "Unspecified" or
    /// "Local"), updated_utc_ticks (DateTime.ToUniversalTime().Ticks, machine independent), warned (the corrupt-file warning
    /// was logged) or "exception" (Load threw). "no session" is folder == C:\photos\vacation with everything else default.
    /// </summary>
    public static JsonArray SessionJsonReadCases(JsonObject inputs)
    {
        var root = SessionTempRoot();
        try
        {
            var log = new SessionOracleLog();
            var store = new SessionStore(new SessionOraclePaths(Path.Combine(root, "Sessions")), new PhysicalFileSystem());
            typeof(SessionStore).GetProperty("Log", BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(store, (Func<ILog>)(() => log));
            const string folder = @"C:\photos\vacation";
            Directory.CreateDirectory(Path.Combine(root, "Sessions"));
            var result = new JsonArray();
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var item = new JsonObject { ["name"] = node!["name"]!.DeepClone() };
                if (node["text"] is { } text) item["text"] = text.DeepClone();
                if (node["hex"] is { } hex) item["hex"] = hex.DeepClone();
                if (node["missing"] is { } missing) item["missing"] = missing.DeepClone();
                var path = store.GetPath(folder);
                if (node["missing"]?.GetValue<bool>() == true) { if (File.Exists(path)) File.Delete(path); }
                else if (node["hex"] is not null) File.WriteAllBytes(path, Convert.FromHexString(node["hex"]!.GetValue<string>()));
                else File.WriteAllBytes(path, new UTF8Encoding(false).GetBytes(node["text"]!.GetValue<string>()));
                log.Warnings.Clear();
                try
                {
                    var s = store.Load(folder);
                    item["folder"] = s.Folder;
                    item["currentPath"] = s.CurrentPath;
                    item["skipped"] = s.Skipped is null
                        ? null
                        : new JsonArray(s.Skipped.Select(x => x is null ? null : (JsonNode)JsonValue.Create(x)!).ToArray());
                    item["updated_kind"] = s.UpdatedUtc.Kind.ToString();
                    item["updated_utc_ticks"] = s.UpdatedUtc.Kind == DateTimeKind.Local ? s.UpdatedUtc.ToUniversalTime().Ticks : s.UpdatedUtc.Ticks;
                    item["warned"] = log.Warnings.Count > 0;
                    item["warning_mentions_path"] = log.Warnings.Count > 0 && log.Warnings[0].Contains(path, StringComparison.Ordinal);
                }
                catch (Exception ex) { item["exception"] = ex.GetType().Name; }
                result.Add(item);
            }
            return result;
        }
        finally { try { Directory.Delete(root, true); } catch (IOException) { } }
    }
}
