// W2-16 golden package: FileLog (C# d92d6aae).
// Copy to tools/oracle-cs/Commands/W2-16.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W2-16
//     "file-log-format" => Commands.FileLogFormatCases(inputs),
//     "file-log-backup-name" => Commands.FileLogBackupNameCases(inputs),
//     "file-log-bytes" => Commands.FileLogBytesCases(inputs),
// Inputs: tests/golden-inputs/{file-log-format,file-log-backup-name,file-log-bytes}.json.
using System.Globalization;
using System.Reflection;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Core.Diagnostics;

static partial class Commands
{
    /// <summary>An exception whose ToString() is exactly the given text (the real one carries a stack trace Rust cannot reproduce).</summary>
    private sealed class FileLogFixedTextException(string text) : Exception("fixed")
    {
        public override string ToString() => text;
    }

    /// <summary>
    /// FileLog.FormatEntry (private static, via reflection) for a given level, message, local timestamp (ticks of a DateTime, any kind), thread id and
    /// optional exception text. The result must be culture independent: every case is formatted under the invariant culture AND under ar-SA, th-TH
    /// and fa-IR and the oracle fails if the texts differ.
    /// </summary>
    public static JsonArray FileLogFormatCases(JsonObject inputs)
    {
        var entryType = typeof(FileLog).GetNestedType("Entry", BindingFlags.NonPublic)!;
        var format = typeof(FileLog).GetMethod("FormatEntry", BindingFlags.NonPublic | BindingFlags.Static)!;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var ticks = long.Parse(node!["ticks"]!.GetValue<string>(), CultureInfo.InvariantCulture);
            var exceptionText = node["exceptionText"]?.GetValue<string>();
            Exception? exception = exceptionText is null ? null : new FileLogFixedTextException(exceptionText);
            string Run()
            {
                var entry = Activator.CreateInstance(entryType, node["level"]!.GetValue<string>(), node["message"]!.GetValue<string>(), exception,
                    new DateTime(ticks, DateTimeKind.Local), node["threadId"]!.GetValue<int>())!;
                return (string)format.Invoke(null, [entry])!;
            }
            var text = Run();
            var previous = CultureInfo.CurrentCulture;
            try
            {
                foreach (var name in new[] { "ar-SA", "th-TH", "fa-IR" })
                {
                    CultureInfo.CurrentCulture = new CultureInfo(name);
                    if (Run() != text) throw new InvalidOperationException($"FormatEntry depends on the culture ({name})");
                }
            }
            finally { CultureInfo.CurrentCulture = previous; }
            result.Add(new JsonObject
            {
                ["level"] = node["level"]!.DeepClone(),
                ["message"] = node["message"]!.DeepClone(),
                ["ticks"] = node["ticks"]!.DeepClone(),
                ["threadId"] = node["threadId"]!.DeepClone(),
                ["exceptionText"] = node["exceptionText"]?.DeepClone(),
                ["line"] = text,
            });
        }
        return result;
    }

    /// <summary>
    /// Rotation naming: a log file of 2048 bytes with maxLogBytes 1024, one entry written, flushed; lists the files left in the folder.
    /// Input: names (log file names). Output per name: the sorted file names, and the length of the backup.
    /// </summary>
    public static JsonArray FileLogBackupNameCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["names"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            var dir = Path.Combine(Path.GetTempPath(), "pr-oracle-filelog-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(dir);
            try
            {
                var path = Path.Combine(dir, name);
                File.WriteAllBytes(path, new byte[2048]);
                using (var log = new FileLog(path, 1024) { Enabled = true })
                {
                    log.Info("after rotation");
                    log.Flush();
                }
                var files = Directory.EnumerateFiles(dir).Select(f => new FileInfo(f)).OrderBy(f => f.Name, StringComparer.Ordinal).ToList();
                result.Add(new JsonObject
                {
                    ["name"] = name,
                    ["files"] = new JsonArray(files.Select(f => (JsonNode)new JsonObject { ["name"] = f.Name, ["length_is_2048"] = f.Length == 2048 }).ToArray()),
                });
            }
            finally { try { Directory.Delete(dir, true); } catch (IOException) { } }
        }
        return result;
    }

    /// <summary>
    /// What reaches the file: encoding, line terminators, exception text placement. Each case enables a FileLog, writes the entries in order
    /// (level, message, exceptionText), flushes and reports the file bytes as hex with the timestamp and thread id of each line masked
    /// ("&lt;ts&gt; [LEVEL] [T]" in place of "yyyy-MM-dd HH:mm:ss.fff [LEVEL] [T12]").
    /// </summary>
    public static JsonArray FileLogBytesCases(JsonObject inputs)
    {
        var mask = new System.Text.RegularExpressions.Regex(@"(?m)^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} \[(INFO|WARN|ERROR)\] \[T\d+\]");
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var dir = Path.Combine(Path.GetTempPath(), "pr-oracle-filelog-" + Guid.NewGuid().ToString("N"));
            try
            {
                var path = Path.Combine(dir, "logs", "t.log");
                using (var log = new FileLog(path) { Enabled = true })
                {
                    foreach (var e in node!["entries"]!.AsArray())
                    {
                        var level = e!["level"]!.GetValue<string>();
                        var message = e["message"]!.GetValue<string>();
                        var exceptionText = e["exceptionText"]?.GetValue<string>();
                        if (level == "INFO") log.Info(message);
                        else if (level == "WARN") log.Warn(message);
                        else log.Error(message, exceptionText is null ? null : new FileLogFixedTextException(exceptionText));
                    }
                    log.Flush();
                }
                var bytes = File.ReadAllBytes(path);
                var text = Encoding.UTF8.GetString(bytes);
                var masked = mask.Replace(text, m => $"<ts> [{m.Groups[1].Value}] [T]");
                result.Add(new JsonObject
                {
                    ["name"] = node["name"]!.DeepClone(),
                    ["entries"] = node["entries"]!.DeepClone(),
                    ["masked_hex"] = Convert.ToHexString(Encoding.UTF8.GetBytes(masked)),
                    ["masked_text"] = masked,
                    ["has_bom"] = bytes.Length >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF,
                });
            }
            finally { try { Directory.Delete(dir, true); } catch (IOException) { } }
        }
        return result;
    }
}
