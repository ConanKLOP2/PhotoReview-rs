using System.Drawing;
using System.Text.Json.Nodes;
using PhotoReview.App.Windowing;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-03a
//     "window-placement-rules" => Commands.WindowPlacementRulesCases(inputs),
//     "window-placement-visibility" => Commands.WindowPlacementVisibilityCases(inputs),
//     "window-placement-json" => Commands.WindowPlacementJsonCases(inputs),
//
// Wiring (the lead does this, the writer did not run the oracle):
//  1. tools/oracle-cs/OracleCs.csproj, inside the existing `<ItemGroup Condition="'$(CSharpSrcDir)' != ''">` (the W7-03d pattern: the five
//     baseline files are compiled into the oracle UNMODIFIED, so the internal types ScreenRect / IMonitorLayout / WindowPlacementVisibility
//     and the internal statics Serialize / Deserialize are used directly, no reflection, no InternalsVisibleTo, nothing copied by hand):
//       <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Windowing\WindowingContracts.cs" Link="cs-baseline\WindowingContracts.cs" />
//       <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Windowing\MonitorLayout.cs" Link="cs-baseline\MonitorLayout.cs" />
//       <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Windowing\WindowPlacementVisibility.cs" Link="cs-baseline\WindowPlacementVisibility.cs" />
//       <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Windowing\WindowPlacementJsonContext.cs" Link="cs-baseline\WindowPlacementJsonContext.cs" />
//       <Compile Include="$(CSharpSrcDir)\src\PhotoReview.App.Shared\Windowing\JsonWindowPlacementStore.cs" Link="cs-baseline\JsonWindowPlacementStore.cs" />
//     FullscreenController.cs (W8-03b) is NOT needed. The oracle project already has AllowUnsafeBlocks (MonitorLayout.cs uses LibraryImport
//     and a function pointer) and the SDK's System.Text.Json source generator (WindowPlacementJsonContext).
//     tools/oracle.ps1 needs no new build step: $work\src is already extracted and passed as -p:CSharpSrcDir.
//  2. Compile-checked by the writer in a scratch project against %TEMP%\pr-oracle\<sha>\src (the same five files + this file); not run.
// No ambient state in the OUTPUT: no clock, cwd or temp path is written. `read_raw_*` cases create and delete a private temp file or folder.
// Exceptions appear as `"threw": "<type name>"`; messages are not compared.
//
// Commands (case input -> added result):
//  window-placement-rules       {name, kind: normalize|plan|resolve, show_command, restore_state?}  -> "value" (int, or the WindowShowState name for plan)
//  window-placement-visibility  {name, rect: [l,t,r,b], work: [[l,t,r,b]...]}                       -> "store" (JsonWindowPlacementStore.IsVisible over a
//                               fixed layout), "direct" (WindowPlacementVisibility.IsVisible over Rectangle.FromLTRB of the same work areas)
//  window-placement-json        {name, kind, ...}
//     serialize:        {data}                       -> "json" (exact text of JsonWindowPlacementStore.Serialize, CRLF line ends on Windows)
//     deserialize:      {text}                       -> "threw", "value" (null when the document is `null` or it threw)
//     read_raw_file:    {bytes_hex}                  -> "threw", "value" (ReadRaw of a temp file holding exactly these bytes)
//     read_raw_missing: {}                           -> "threw", "value" (ReadRaw of a path that does not exist)
//     read_raw_directory: {}                         -> "threw", "value" (ReadRaw of an existing DIRECTORY: File.Exists is false)
//   `data` / `value` shape: {length, flags, show_command, min_x, min_y, max_x, max_y, normal_left, normal_top, normal_right, normal_bottom}.

// W8-03a: WindowShowState, WindowPlacementData, WindowPlacementRules, ScreenRect/IMonitorLayout (fixed layout only),
//         WindowPlacementVisibility, JsonWindowPlacementStore (Serialize / Deserialize / ReadRaw / IsVisible).
static partial class Commands
{
    private sealed class W803aFixedLayout(IReadOnlyList<ScreenRect> works) : IMonitorLayout
    {
        public nint MonitorOf(nint hwnd) => 1;

        public bool TryGetWork(nint monitor, out ScreenRect work)
        {
            work = works.Count > 0 ? works[0] : default;
            return works.Count > 0;
        }

        public IReadOnlyList<ScreenRect> WorkAreas() => works;
    }

    private static int W803aInt(JsonObject o, string key) => o[key]!.GetValue<int>();

    private static WindowPlacementData W803aData(JsonObject o) => new(
        W803aInt(o, "length"), W803aInt(o, "flags"), W803aInt(o, "show_command"),
        W803aInt(o, "min_x"), W803aInt(o, "min_y"), W803aInt(o, "max_x"), W803aInt(o, "max_y"),
        W803aInt(o, "normal_left"), W803aInt(o, "normal_top"), W803aInt(o, "normal_right"), W803aInt(o, "normal_bottom"));

    private static JsonObject W803aDataJson(WindowPlacementData d) => new()
    {
        ["length"] = d.Length,
        ["flags"] = d.Flags,
        ["show_command"] = d.ShowCommand,
        ["min_x"] = d.MinX,
        ["min_y"] = d.MinY,
        ["max_x"] = d.MaxX,
        ["max_y"] = d.MaxY,
        ["normal_left"] = d.NormalLeft,
        ["normal_top"] = d.NormalTop,
        ["normal_right"] = d.NormalRight,
        ["normal_bottom"] = d.NormalBottom,
    };

    /// <summary>Cases {name, kind, show_command, restore_state?}; restore_state is null/absent or a WindowShowState name.</summary>
    public static JsonArray WindowPlacementRulesCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var showCommand = c["show_command"]!.GetValue<int>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind, ["show_command"] = showCommand };
            switch (kind)
            {
                case "normalize":
                    item["value"] = WindowPlacementRules.NormalizeShowCommand(showCommand);
                    break;
                case "plan":
                    item["value"] = WindowPlacementRules.PlanStateBeforeShow(showCommand).ToString();
                    break;
                case "resolve":
                {
                    var restoreName = c["restore_state"]?.GetValue<string>();
                    WindowShowState? restore = restoreName is null ? null : Enum.Parse<WindowShowState>(restoreName);
                    item["restore_state"] = restoreName;
                    item["value"] = WindowPlacementRules.ResolveShowCommand(showCommand, restore);
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + kind);
            }
            result.Add(item);
        }
        return result;
    }

    private static ScreenRect W803aRect(JsonNode? n)
    {
        var a = n!.AsArray();
        return new ScreenRect(a[0]!.GetValue<int>(), a[1]!.GetValue<int>(), a[2]!.GetValue<int>(), a[3]!.GetValue<int>());
    }

    /// <summary>Cases {name, rect: [l,t,r,b], work: [[l,t,r,b], ...]}.</summary>
    public static JsonArray WindowPlacementVisibilityCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var rect = W803aRect(c["rect"]);
            var works = c["work"]!.AsArray().Select(W803aRect).ToList();
            var data = new WindowPlacementData(44, 0, 1, -1, -1, -1, -1, rect.Left, rect.Top, rect.Right, rect.Bottom);
            var store = new JsonWindowPlacementStore(new W803aFixedLayout(works));
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["rect"] = c["rect"]!.DeepClone(),
                ["work"] = c["work"]!.DeepClone(),
                ["store"] = store.IsVisible(data),
                ["direct"] = WindowPlacementVisibility.IsVisible(rect.Left, rect.Top, rect.Right, rect.Bottom,
                    works.Select(w => Rectangle.FromLTRB(w.Left, w.Top, w.Right, w.Bottom))),
            });
        }
        return result;
    }

    private static void W803aOutcome(JsonObject item, Func<WindowPlacementData?> read)
    {
        try
        {
            var value = read();
            item["threw"] = null;
            item["value"] = value is null ? null : W803aDataJson(value);
        }
        catch (Exception ex)
        {
            item["threw"] = ex.GetType().Name;
            item["value"] = null;
        }
    }

    /// <summary>Cases {name, kind, ...}: see the header.</summary>
    public static JsonArray WindowPlacementJsonCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var kind = c["kind"]!.GetValue<string>();
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["kind"] = kind };
            switch (kind)
            {
                case "serialize":
                    item["data"] = c["data"]!.DeepClone();
                    item["json"] = JsonWindowPlacementStore.Serialize(W803aData(c["data"]!.AsObject()));
                    break;
                case "deserialize":
                {
                    var text = c["text"]!.GetValue<string>();
                    item["text"] = text;
                    W803aOutcome(item, () => JsonWindowPlacementStore.Deserialize(text));
                    break;
                }
                case "read_raw_file":
                {
                    var hex = c["bytes_hex"]!.GetValue<string>();
                    item["bytes_hex"] = hex;
                    var path = Path.Combine(Path.GetTempPath(), "pr-oracle-w803a-" + Guid.NewGuid().ToString("N") + ".json");
                    File.WriteAllBytes(path, Convert.FromHexString(hex));
                    try { W803aOutcome(item, () => JsonWindowPlacementStore.ReadRaw(path)); }
                    finally { File.Delete(path); }
                    break;
                }
                case "read_raw_missing":
                {
                    var path = Path.Combine(Path.GetTempPath(), "pr-oracle-w803a-missing-" + Guid.NewGuid().ToString("N") + ".json");
                    W803aOutcome(item, () => JsonWindowPlacementStore.ReadRaw(path));
                    break;
                }
                case "read_raw_directory":
                {
                    var dir = Path.Combine(Path.GetTempPath(), "pr-oracle-w803a-dir-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(dir);
                    try { W803aOutcome(item, () => JsonWindowPlacementStore.ReadRaw(dir)); }
                    finally { Directory.Delete(dir); }
                    break;
                }
                default:
                    throw new ArgumentException("unknown kind " + kind);
            }
            result.Add(item);
        }
        return result;
    }
}
