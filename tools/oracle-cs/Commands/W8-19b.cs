using System.Collections;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19b oracle (the F half of the unit): the layout elements of SettingsPageBuilder.cs lines 1-307 (SettingsGroupElement, SettingsCheckElement, SettingsRowElement,
// SettingsSliderElement, SettingsTextElement, SettingsButtonsElement, SettingsRadioElement, SettingsCheckFlowElement) and the key grid of lines 309-365 (W8-19c uses
// the same file and the same golden), run against a deterministic fake ISettingsLayoutSurface. Nothing here opens a window. NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19b
//     "settings-layout" => Commands.W819bSettingsLayoutCases(inputs),
//
// Wiring: none beyond the line above. OracleCs.csproj already references PhotoReview.dll (Shell.Win32, assembly name "PhotoReview"). ISettingsLayoutSurface and every
// element are `internal`; the surface is implemented with System.Reflection.DispatchProxy (the proxy generator grants itself access to the internal interface),
// the elements are built by reflection (as W8-18d), so there is no InternalsVisibleTo and no change to the C# repo.
//
// The fake surface is DETERMINISTIC and defined by the input "surfaces" object (ints; a name is referenced by each case's "surface"):
//   dx_mul/dx_div, dy_mul/dy_div : Dx(d) = d * dx_mul / dx_div and Dy(d) = d * dy_mul / dy_div (C# int arithmetic, truncating division)
//   line_height, field_height    : LineHeight, FieldHeight
//   avg_char_width               : TextWidth(text) = text.Length * avg_char_width   (text.Length = UTF-16 code units; a non-BMP character counts 2)
//   TextHeight(id, width)        : "heights"[id] when present, else Max(1, ceil(TextWidth(TextOf(id)) / Max(1, width))) * line_height
//   TextOf(id)                   : the case's "texts"[id] or ""
//   IsHidden(id)                 : the id is in the case's "hidden" array
//   ComboDropHeight(id)          : the case's "combo_drop"[id] or 0
// Case input: { "name", "kind": "element" | "label_width", "surface": <name>, ... }
//   element:     "element": <spec>, "x", "y", "width"; optional "texts": {"<id>": "..."}, "heights": {"<id>": n}, "hidden": [ids], "combo_drop": {"<id>": n}
//                spec = { "type": group|check|row|slider|text|buttons|radio|check_flow|key_grid, "visible": false (optional, sets Visible),
//                         group: "title", "children": [spec];  check: "id", "indent";  row: "label", "control", "row_kind": "Combo"|"Edit", "width_dlu";
//                         slider: "label", "slider", "value";  text: "id", "aligned";  buttons: "ids", "aligned";  radio: "id", "hint" (null|id), "indent";
//                         check_flow: "ids", "indent";  key_grid: "cells": [ { "label", "box", "clear" } ] }
//                Every output case also carries "surface_def" (a copy of the named surface object).
//                Output fields: height (the return value of Place), calls (array in call order: ["place", id, x, y, w, h] or ["hide", id]), control_ids (the element's ControlIds).
//   label_width: "widths": [ints] -> output "values": SettingsLayoutElement.LabelWidth(surface, width) for each.
static partial class Commands
{
    private const string W819bNs = "PhotoReview.Shell.Win32.Dialogs.SettingsUi.";
    private const BindingFlags W819bFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W819bType(string name) => Assembly.Load("PhotoReview").GetType(W819bNs + name, throwOnError: true)!;

    private static object W819bNew(string typeName, params object?[] args) =>
        W819bType(typeName).GetConstructors(W819bFlags)[0].Invoke(args);

    public static JsonArray W819bSettingsLayoutCases(JsonObject inputs)
    {
        Type baseType = W819bType("SettingsLayoutElement");
        Type surfaceType = W819bType("ISettingsLayoutSurface");
        var surfaces = inputs["surfaces"]!.AsObject();
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var proxy = (W819bSurfaceProxy)DispatchProxy.Create(surfaceType, typeof(W819bSurfaceProxy));
            var surfaceDef = surfaces[c["surface"]!.GetValue<string>()]!.AsObject();
            proxy.Configure(surfaceDef, c);
            c["surface_def"] = surfaceDef.DeepClone(); // the golden is self-contained: the Rust test builds its fake from these numbers
            switch (c["kind"]!.GetValue<string>())
            {
                case "label_width":
                {
                    var values = new JsonArray();
                    foreach (var w in c["widths"]!.AsArray())
                    {
                        values.Add((int)baseType.GetMethod("LabelWidth", W819bFlags)!.Invoke(null, [proxy, w!.GetValue<int>()])!);
                    }

                    c["values"] = values;
                    break;
                }

                case "element":
                {
                    object element = W819bBuild(c["element"]!.AsObject(), baseType);
                    int height = (int)baseType.GetMethod("Place", W819bFlags)!.Invoke(element,
                        [proxy, c["x"]!.GetValue<int>(), c["y"]!.GetValue<int>(), c["width"]!.GetValue<int>()])!;
                    c["height"] = height;
                    c["calls"] = proxy.Calls;
                    var ids = new JsonArray();
                    foreach (var id in (IEnumerable)baseType.GetProperty("ControlIds", W819bFlags)!.GetValue(element)!)
                    {
                        ids.Add((int)id);
                    }

                    c["control_ids"] = ids;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-layout kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static int[] W819bInts(JsonNode? node) => node!.AsArray().Select(n => n!.GetValue<int>()).ToArray();

    private static object W819bBuild(JsonObject spec, Type baseType)
    {
        object element;
        switch (spec["type"]!.GetValue<string>())
        {
            case "group":
            {
                var list = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(baseType))!;
                foreach (var child in spec["children"]!.AsArray())
                {
                    list.Add(W819bBuild(child!.AsObject(), baseType));
                }

                // The title id is the first control id of the group; a spec without "title" uses 900 + the child count (only the id matters here).
                element = W819bNew("SettingsGroupElement", spec["title"]?.GetValue<int>() ?? 900, list);
                break;
            }

            case "check":
                element = W819bNew("SettingsCheckElement", spec["id"]!.GetValue<int>(), spec["indent"]!.GetValue<int>());
                break;
            case "row":
            {
                object kind = Enum.Parse(W819bType("SettingsRowKind"), spec["row_kind"]!.GetValue<string>());
                element = W819bNew("SettingsRowElement", spec["label"]!.GetValue<int>(), spec["control"]!.GetValue<int>(), kind, spec["width_dlu"]!.GetValue<int>());
                break;
            }

            case "slider":
                element = W819bNew("SettingsSliderElement", spec["label"]!.GetValue<int>(), spec["slider"]!.GetValue<int>(), spec["value"]!.GetValue<int>());
                break;
            case "text":
                element = W819bNew("SettingsTextElement", spec["id"]!.GetValue<int>(), spec["aligned"]!.GetValue<bool>());
                break;
            case "buttons":
                element = W819bNew("SettingsButtonsElement", W819bInts(spec["ids"]), spec["aligned"]!.GetValue<bool>());
                break;
            case "radio":
                element = W819bNew("SettingsRadioElement", spec["id"]!.GetValue<int>(), spec["hint"]?.GetValue<int>(), spec["indent"]!.GetValue<int>());
                break;
            case "check_flow":
                element = W819bNew("SettingsCheckFlowElement", W819bInts(spec["ids"]), spec["indent"]!.GetValue<int>());
                break;
            case "key_grid":
            {
                Type cellType = W819bType("SettingsKeyCell");
                var cells = spec["cells"]!.AsArray();
                Array array = Array.CreateInstance(cellType, cells.Count);
                for (int i = 0; i < cells.Count; i++)
                {
                    var cell = cells[i]!.AsObject();
                    array.SetValue(cellType.GetConstructors(W819bFlags)[0].Invoke(
                        [cell["label"]!.GetValue<int>(), cell["box"]!.GetValue<int>(), cell["clear"]!.GetValue<int>()]), i);
                }

                element = W819bNew("SettingsKeyGridElement", array);
                break;
            }

            default:
                throw new InvalidOperationException("unknown element type");
        }

        if (spec["visible"] is { } visible)
        {
            baseType.GetProperty("Visible", W819bFlags)!.SetValue(element, visible.GetValue<bool>());
        }

        return element;
    }
}

/// <summary>Deterministic fake of the internal ISettingsLayoutSurface (see the header of W8-19b.cs for the formulas). Public so DispatchProxy can derive from it.</summary>
public class W819bSurfaceProxy : DispatchProxy
{
    private JsonObject _s = new();
    private JsonObject _case = new();

    public JsonArray Calls { get; } = new();

    public void Configure(JsonObject surface, JsonObject theCase)
    {
        _s = surface;
        _case = theCase;
    }

    private int I(string key) => _s[key]!.GetValue<int>();

    private string TextOfId(int id) => _case["texts"]?[id.ToString()]?.GetValue<string>() ?? string.Empty;

    private int TextWidthOf(string text) => text.Length * I("avg_char_width");

    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args)
    {
        switch (targetMethod!.Name)
        {
            case "Dx":
                return (int)args![0]! * I("dx_mul") / I("dx_div");
            case "Dy":
                return (int)args![0]! * I("dy_mul") / I("dy_div");
            case "get_LineHeight":
                return I("line_height");
            case "get_FieldHeight":
                return I("field_height");
            case "TextOf":
                return TextOfId((int)args![0]!);
            case "TextHeight":
            {
                int id = (int)args![0]!;
                if (_case["heights"]?[id.ToString()] is { } fixedHeight)
                {
                    return fixedHeight.GetValue<int>();
                }

                int width = Math.Max(1, (int)args![1]!);
                int textWidth = TextWidthOf(TextOfId(id));
                int lines = Math.Max(1, (textWidth + width - 1) / width);
                return lines * I("line_height");
            }

            case "TextWidth":
                return TextWidthOf((string)args![0]!);
            case "IsHidden":
                return _case["hidden"]?.AsArray().Any(n => n!.GetValue<int>() == (int)args![0]!) ?? false;
            case "ComboDropHeight":
                return _case["combo_drop"]?[((int)args![0]!).ToString()]?.GetValue<int>() ?? 0;
            case "Place":
                Calls.Add(new JsonArray("place", (int)args![0]!, (int)args![1]!, (int)args![2]!, (int)args![3]!, (int)args![4]!));
                return null;
            case "Hide":
                Calls.Add(new JsonArray("hide", (int)args![0]!));
                return null;
            default:
                throw new NotSupportedException(targetMethod.Name);
        }
    }
}
