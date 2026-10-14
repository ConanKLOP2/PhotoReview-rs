// W2-14 golden package: PhysicalFileSystem / CountingFileSystem (C# d92d6aae).
// Copy to tools/oracle-cs/Commands/W2-14.cs and add these dispatch lines to the `command switch` in tools/oracle-cs/Program.cs
// (before the `_ =>` line):
//     // W2-14
//     "physical-fs-ops" => Commands.PhysFsOpsCases(inputs),
//     "physical-fs-pattern" => Commands.PhysFsPatternCases(inputs),
//     "physical-fs-read-text" => Commands.PhysFsReadTextCases(inputs),
//     "physical-fs-resolve" => Commands.PhysFsResolveCases(inputs),
// Inputs: tests/golden-inputs/{physical-fs-ops,physical-fs-pattern,physical-fs-read-text,physical-fs-resolve}.json.
// The ops command creates a private temp folder per case (real disk, removed afterwards; junctions via `cmd /c mklink /J`).
using System.Diagnostics;
using System.Globalization;
using System.Text;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.IO;

static partial class Commands
{
    private static string PhysFsTempRoot()
    {
        var root = Path.Combine(Path.GetTempPath(), "pr-oracle-physfs-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        return root;
    }

    private static void PhysFsCleanup(string root, List<IDisposable> holders)
    {
        foreach (var h in holders) { try { h.Dispose(); } catch (Exception) { } }
        try
        {
            foreach (var entry in Directory.EnumerateFileSystemEntries(root, "*", SearchOption.AllDirectories).ToList())
            {
                try
                {
                    var attrs = File.GetAttributes(entry);
                    if ((attrs & FileAttributes.ReparsePoint) != 0 && (attrs & FileAttributes.Directory) != 0) { Directory.Delete(entry); continue; }
                    File.SetAttributes(entry, (attrs & FileAttributes.Directory) != 0 ? FileAttributes.Directory : FileAttributes.Normal);
                }
                catch (Exception) { }
            }
            Directory.Delete(root, true);
        }
        catch (Exception) { }
    }

    private static void PhysFsMakeJunction(string link, string target)
    {
        var psi = new ProcessStartInfo("cmd.exe") { UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true };
        psi.ArgumentList.Add("/c"); psi.ArgumentList.Add("mklink"); psi.ArgumentList.Add("/J"); psi.ArgumentList.Add(link); psi.ArgumentList.Add(target);
        using var p = Process.Start(psi)!;
        var text = p.StandardOutput.ReadToEnd() + p.StandardError.ReadToEnd();
        p.WaitForExit();
        if (p.ExitCode != 0) throw new InvalidOperationException("mklink failed: " + text);
    }

    private static int PhysFsWin32(Exception ex) =>
        (ex.HResult & 0x7FFF0000) == 0x70000 ? ex.HResult & 0xFFFF : 0;

    /// <summary>
    /// Scenario interpreter over a real folder. Case: name, setup (list), op, args (strings), optional text/durable/pattern/hex.
    /// Setup items: {file,text,readonly,lastWriteUtc} | {fileHex,hex} | {dir} | {junction,target} | {hold,access,share} | {holdAppend}
    /// | {deleteDir} (applied in order). An arg is relative to the folder unless it contains "{root}" / "{rootfwd}" (substituted) or starts
    /// with "@" (literal path without the "@"). Output: ok + value (strings have the folder replaced by "&lt;root&gt;") or
    /// ok=false + exception + win32 (HResult low word when it is a Win32 HRESULT, else 0), plus "after" (post-state probes).
    /// </summary>
    public static JsonArray PhysFsOpsCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var root = PhysFsTempRoot();
            var holders = new List<IDisposable>();
            var fs = new PhysicalFileSystem();
            string Arg(int i)
            {
                var a = node!["args"]![i]!.GetValue<string>();
                if (a.StartsWith('@')) return a[1..];
                if (a.Contains("{root}") || a.Contains("{rootfwd}")) return a.Replace("{root}", root).Replace("{rootfwd}", root.Replace('\\', '/'));
                return Path.Combine(root, a);
            }
            string Mask(string s) => s.Replace(root, "<root>").Replace(root.Replace('\\', '/'), "<rootfwd>");
            string? Rel(string? s) => s is null ? null : Mask(s);
            var item = new JsonObject();
            foreach (var kv in node!.AsObject()) item[kv.Key] = kv.Value?.DeepClone(); // echo the whole input case: the golden file is self-contained
            try
            {
                foreach (var s in node["setup"]?.AsArray() ?? [])
                {
                    var o = s!.AsObject();
                    if (o["file"] is { } f)
                    {
                        var path = Path.Combine(root, f.GetValue<string>());
                        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
                        File.WriteAllText(path, o["text"]?.GetValue<string>() ?? "x", new UTF8Encoding(false));
                        if (o["lastWriteUtc"] is { } lw) File.SetLastWriteTimeUtc(path, DateTime.Parse(lw.GetValue<string>(), CultureInfo.InvariantCulture, DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal));
                        if (o["readonly"]?.GetValue<bool>() == true) File.SetAttributes(path, FileAttributes.ReadOnly);
                    }
                    else if (o["fileHex"] is { } fh)
                    {
                        var path = Path.Combine(root, fh.GetValue<string>());
                        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
                        File.WriteAllBytes(path, Convert.FromHexString(o["hex"]!.GetValue<string>()));
                    }
                    else if (o["dir"] is { } d) Directory.CreateDirectory(Path.Combine(root, d.GetValue<string>()));
                    else if (o["junction"] is { } j) PhysFsMakeJunction(Path.Combine(root, j.GetValue<string>()), Path.Combine(root, o["target"]!.GetValue<string>()));
                    else if (o["deleteDir"] is { } dd) Directory.Delete(Path.Combine(root, dd.GetValue<string>()));
                    else if (o["hold"] is { } h)
                    {
                        var access = o["access"]!.GetValue<string>() switch { "read" => FileAccess.Read, "write" => FileAccess.Write, _ => FileAccess.ReadWrite };
                        var share = o["share"]!.GetValue<string>() switch
                        {
                            "none" => FileShare.None, "read" => FileShare.Read, "readwrite" => FileShare.ReadWrite, _ => FileShare.ReadWrite | FileShare.Delete,
                        };
                        holders.Add(new FileStream(Path.Combine(root, h.GetValue<string>()), FileMode.Open, access, share));
                    }
                    else if (o["holdAppend"] is { } ha) holders.Add(fs.OpenAppend(Path.Combine(root, ha.GetValue<string>()), o["durable"]?.GetValue<bool>() ?? false));
                }

                var op = node["op"]!.GetValue<string>();
                JsonNode? value = null;
                var after = new JsonObject();
                try
                {
                    switch (op)
                    {
                        case "delete": try { fs.Delete(Arg(0)); } finally { after["exists"] = File.Exists(Arg(0)); } break;
                        case "move": try { fs.Move(Arg(0), Arg(1)); } finally { after["src_exists"] = File.Exists(Arg(0)) || Directory.Exists(Arg(0)); after["dst_exists"] = File.Exists(Arg(1)) || Directory.Exists(Arg(1)); } break;
                        case "copy": try { fs.Copy(Arg(0), Arg(1)); } finally { after["dst_exists"] = File.Exists(Arg(1)); } break;
                        case "copy-attrs":
                            fs.Copy(Arg(0), Arg(1));
                            value = new JsonObject
                            {
                                ["same_write_time"] = File.GetLastWriteTimeUtc(Arg(0)) == File.GetLastWriteTimeUtc(Arg(1)),
                                ["readonly"] = (File.GetAttributes(Arg(1)) & FileAttributes.ReadOnly) != 0,
                                ["write_ticks"] = File.GetLastWriteTimeUtc(Arg(1)).Ticks,
                            };
                            break;
                        case "trycopynew": try { value = fs.TryCopyNew(Arg(0), Arg(1)); } finally { after["dst_exists"] = File.Exists(Arg(1)) || Directory.Exists(Arg(1)); } break;
                        case "trycopynew-proof":
                        {
                            var proof = new CopyCreationProof();
                            try { value = fs.TryCopyNew(Arg(0), Arg(1), proof); } finally { after["proof_created"] = proof.DestinationCreated; }
                            break;
                        }
                        case "stat":
                        {
                            var st = fs.GetFileStat(Arg(0));
                            value = st is null ? null : new JsonObject { ["length"] = st.Length, ["write_ticks"] = st.LastWriteUtc.Ticks, ["utc"] = st.LastWriteUtc.Kind == DateTimeKind.Utc };
                            break;
                        }
                        case "fileexists": value = fs.FileExists(Arg(0)); break;
                        case "direxists": value = fs.DirectoryExists(Arg(0)); break;
                        case "mkdir": try { fs.CreateDirectory(Arg(0)); } finally { after["dir_exists"] = Directory.Exists(Arg(0)); } break;
                        case "enumdirs": value = new JsonArray(fs.EnumerateDirectories(Arg(0)).Select(Mask).Order(StringComparer.Ordinal).Select(x => (JsonNode)JsonValue.Create(x)!).ToArray()); break;
                        case "enumfiles":
                        {
                            var pattern = node["pattern"]?.GetValue<string>();
                            var files = pattern is null ? fs.EnumerateFiles(Arg(0)) : fs.EnumerateFiles(Arg(0), pattern);
                            value = new JsonArray(files.Select(Mask).Order(StringComparer.Ordinal).Select(x => (JsonNode)JsonValue.Create(x)!).ToArray());
                            break;
                        }
                        case "enumstat":
                        case "enumreadable":
                        {
                            var suffix = node["includeSuffix"]?.GetValue<string>();
                            var skipped = new JsonArray();
                            Func<string, bool> include = p => suffix is null || p.EndsWith(suffix, StringComparison.OrdinalIgnoreCase);
                            Action<SkippedEntry> onSkipped = s => skipped.Add(new JsonObject { ["path"] = Mask(s.Path), ["kind"] = s.Kind.ToString(), ["reason_present"] = !string.IsNullOrWhiteSpace(s.Reason) });
                            var listing = op == "enumstat" ? fs.EnumerateFilesWithStat(Arg(0), include, onSkipped) : fs.EnumerateReadableFilesWithStat(Arg(0), include, onSkipped);
                            var files = listing.Select(f => new JsonObject { ["path"] = Mask(f.Path), ["length"] = f.Stat?.Length, ["write_ticks"] = f.Stat?.LastWriteUtc.Ticks }).OrderBy(x => x["path"]!.GetValue<string>(), StringComparer.Ordinal).ToList();
                            value = new JsonObject { ["files"] = new JsonArray(files.Select(x => (JsonNode)x).ToArray()), ["skipped"] = skipped };
                            break;
                        }
                        case "readtext": value = fs.ReadAllText(Arg(0)); break;
                        case "openread":
                        {
                            using var s = fs.OpenReadShared(Arg(0));
                            using var ms = new MemoryStream();
                            s.CopyTo(ms);
                            value = Encoding.UTF8.GetString(ms.ToArray());
                            break;
                        }
                        case "openread-then-rename":
                        {
                            using var s = fs.OpenReadShared(Arg(0));
                            File.Move(Arg(0), Arg(1));
                            File.Delete(Arg(1));
                            using var ms = new MemoryStream();
                            s.CopyTo(ms);
                            value = Encoding.UTF8.GetString(ms.ToArray());
                            break;
                        }
                        case "openappend":
                        {
                            var durable = node["durable"]?.GetValue<bool>() ?? false;
                            foreach (var text in node["texts"]!.AsArray())
                            {
                                using var s = fs.OpenAppend(Arg(0), durable);
                                s.Write(Encoding.UTF8.GetBytes(text!.GetValue<string>()));
                                s.Flush();
                            }
                            value = File.ReadAllText(Arg(0));
                            break;
                        }
                        case "openappend-twice":
                        {
                            using var first = fs.OpenAppend(Arg(0), false);
                            using var second = fs.OpenAppend(Arg(0), false);
                            break;
                        }
                        case "openappend-then-delete":
                        {
                            using var s = fs.OpenAppend(Arg(0), false);
                            File.Delete(Arg(0));
                            break;
                        }
                        case "openappend-then-open-reader":
                        {
                            using var s = fs.OpenAppend(Arg(0), true);
                            var share = node["readerShare"]!.GetValue<string>() == "read" ? FileShare.Read : FileShare.ReadWrite | FileShare.Delete;
                            using var r = new FileStream(Arg(0), FileMode.Open, FileAccess.Read, share);
                            break;
                        }
                        case "writeatomic":
                        {
                            try { fs.WriteAllTextAtomic(Arg(0), node["text"]!.GetValue<string>(), node["durable"]?.GetValue<bool>() ?? true); }
                            finally
                            {
                                var dir = Path.GetDirectoryName(Arg(0))!;
                                after["tmp_files"] = Directory.Exists(dir) ? Directory.EnumerateFiles(dir, "*.tmp").Count() : 0;
                            }
                            value = BitConverter.ToString(File.ReadAllBytes(Arg(0))).Replace("-", "");
                            break;
                        }
                        case "probe":
                        {
                            var ok = fs.TryProbeReadable(Arg(0), out var failure);
                            value = new JsonObject { ["readable"] = ok, ["reason_present"] = !string.IsNullOrWhiteSpace(failure), ["failure_is_null"] = failure is null };
                            break;
                        }
                        case "trydeleteempty": value = fs.TryDeleteEmptyDirectory(Arg(0)); after["dir_exists"] = Directory.Exists(Arg(0)); break;
                        case "resolve": value = Rel(fs.ResolveRealPath(Arg(0))); break;
                        default: throw new ArgumentException("unknown op " + op);
                    }
                    item["ok"] = true;
                    item["value"] = value;
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
                {
                    item["ok"] = false;
                    item["exception"] = ex.GetType().Name;
                    item["win32"] = PhysFsWin32(ex);
                }
                item["after"] = after;
            }
            finally { PhysFsCleanup(root, holders); }
            result.Add(item);
        }
        return result;
    }

    /// <summary>Directory.EnumerateFiles(dir, pattern) through PhysicalFileSystem over real files: which names a pattern matches.</summary>
    public static JsonArray PhysFsPatternCases(JsonObject inputs)
    {
        var root = PhysFsTempRoot();
        try
        {
            foreach (var n in inputs["names"]!.AsArray()) File.WriteAllText(Path.Combine(root, n!.GetValue<string>()), "x");
            foreach (var d in inputs["dirs"]!.AsArray()) Directory.CreateDirectory(Path.Combine(root, d!.GetValue<string>()));
            var fs = new PhysicalFileSystem();
            var result = new JsonArray();
            foreach (var p in inputs["patterns"]!.AsArray())
            {
                var pattern = p!.GetValue<string>();
                var item = new JsonObject { ["pattern"] = pattern, ["names"] = inputs["names"]!.DeepClone(), ["dirs"] = inputs["dirs"]!.DeepClone() };
                try
                {
                    var found = fs.EnumerateFiles(root, pattern).Select(Path.GetFileName).Order(StringComparer.Ordinal).ToList();
                    item["matched"] = new JsonArray(found.Select(f => (JsonNode)JsonValue.Create(f)!).ToArray());
                }
                catch (Exception ex) { item["exception"] = ex.GetType().Name; }
                result.Add(item);
            }
            return result;
        }
        finally { PhysFsCleanup(root, []); }
    }

    /// <summary>PhysicalFileSystem.ReadAllText (File.ReadAllText: BOM detection, invalid bytes become U+FFFD): UTF-16 code units of the result.</summary>
    public static JsonArray PhysFsReadTextCases(JsonObject inputs)
    {
        var root = PhysFsTempRoot();
        try
        {
            var fs = new PhysicalFileSystem();
            var result = new JsonArray();
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var hex = node!["hex"]!.GetValue<string>();
                var path = Path.Combine(root, "t.txt");
                File.WriteAllBytes(path, Convert.FromHexString(hex));
                var text = fs.ReadAllText(path);
                result.Add(new JsonObject
                {
                    ["name"] = node["name"]!.DeepClone(),
                    ["hex"] = hex,
                    ["utf16_units"] = new JsonArray(text.Select(c => (JsonNode)JsonValue.Create((int)c)!).ToArray()),
                });
            }
            return result;
        }
        finally { PhysFsCleanup(root, []); }
    }

    /// <summary>ResolveRealPath of absolute paths that do not exist on any machine (pure path normalisation plus roots).</summary>
    public static JsonArray PhysFsResolveCases(JsonObject inputs)
    {
        var fs = new PhysicalFileSystem();
        var result = new JsonArray();
        foreach (var node in inputs["paths"]!.AsArray())
        {
            var path = node!.GetValue<string>();
            var item = new JsonObject { ["path"] = path };
            try { item["resolved"] = fs.ResolveRealPath(path); }
            catch (Exception ex) { item["resolved"] = null; item["exception"] = ex.GetType().Name; }
            result.Add(item);
        }
        return result;
    }
}
