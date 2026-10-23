using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W9-E2 oracle (ExternalEditorsDialog; baseline PhotoReview commit 7eaee2a2).
// Dispatch line in tools/oracle-cs/Program.cs (inside the command switch, after W9-E7):
//     // Commands\Commands\W9-E2
//     "dialog-external-editors" => Commands.W9E2ExternalEditorsDialogCases(inputs),
// The internal dialog is reached by reflection; ExternalEditor is a public Core type reached by reflection too.
// Its real constructor builds items but creates no window. The bare instance for layout arithmetic has _avgCharWidth,
// _lineHeight and _childHosts initialized like W8-18q; no window is created.
// Inputs in tests/golden-inputs/dialog-external-editors.json:
//   dialog: editors [{id,name,path,arguments}], default_id -> title, items [{class,id,text,style,ex_style}], initial_focus_id,
//           default_button_id, resizable, header_ids, uses_modal_frame, extra_window_style
//   row_text: editors, default_id, index -> text, badge (string or null), via RowText(index) in range only
//   preferred: avg, line -> preferred_width/height, minimum_width/height
//   metrics: avg, line, client_width -> margin, field_height, button_height, left_width (private helpers)
//   constants: every declared internal const int *Id -> lowercase_snake_case integer fields
//   effective_default: editors, default_id -> effective_default_id and index_of_default (private helpers)
// Every output case copies the input and adds expected fields; process language is English.

static partial class Commands
{
    private const string W9e2Assembly = "PhotoReview";
    private const string W9e2CoreAssembly = "PhotoReview.Core";
    private const string W9e2Namespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W9e2Flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W9e2Type(string name) => Assembly.Load(W9e2Assembly).GetType(W9e2Namespace + name, throwOnError: true)!;

    private static MethodInfo W9e2Getter(Type type, string property)
    {
        for (Type? t = type; t is not null; t = t.BaseType)
        {
            PropertyInfo? p = t.GetProperty(property, W9e2Flags | BindingFlags.DeclaredOnly);
            if (p?.GetGetMethod(nonPublic: true) is { } getter) return getter;
        }
        throw new InvalidOperationException("no property " + property);
    }

    private static object? W9e2Get(object dialog, string property) => W9e2Getter(dialog.GetType(), property).Invoke(dialog, null);

    private static IList W9e2Editors(JsonObject c)
    {
        Type editorType = Assembly.Load(W9e2CoreAssembly).GetType("PhotoReview.Core.Settings.ExternalEditor", throwOnError: true)!;
        var list = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(editorType))!;
        foreach (var node in c["editors"]!.AsArray())
        {
            var o = node!.AsObject();
            object editor = Activator.CreateInstance(editorType)!;
            foreach (string field in new[] { "id", "name", "path", "arguments" })
                if (o.ContainsKey(field)) editorType.GetProperty(char.ToUpperInvariant(field[0]) + field[1..])!.SetValue(editor, o[field]!.GetValue<string>());
            list.Add(editor);
        }
        return list;
    }

    private static object W9e2Dialog(JsonObject c) => Activator.CreateInstance(W9e2Type("ExternalEditorsDialog"), W9e2Flags,
        binder: null, args: [W9e2Editors(c), c["default_id"]?.GetValue<string>(), null, null, null, null], culture: CultureInfo.InvariantCulture)!;

    private static object W9e2Bare(int avg, int line)
    {
        object dialog = RuntimeHelpers.GetUninitializedObject(W9e2Type("ExternalEditorsDialog"));
        Type baseType = W9e2Type("NativeDialog");
        baseType.GetField("_avgCharWidth", W9e2Flags)!.SetValue(dialog, avg);
        baseType.GetField("_lineHeight", W9e2Flags)!.SetValue(dialog, line);
        baseType.GetField("_childHosts", W9e2Flags)!.SetValue(dialog, new List<nint>());
        return dialog;
    }

    private static JsonArray W9e2Items(IEnumerable items)
    {
        var result = new JsonArray();
        foreach (object item in items)
        {
            Type t = item.GetType();
            result.Add(new JsonObject
            {
                ["class"] = t.GetProperty("Class")!.GetValue(item)!.ToString(),
                ["id"] = (int)t.GetProperty("Id")!.GetValue(item)!,
                ["text"] = (string?)t.GetProperty("Text")!.GetValue(item) ?? string.Empty,
                ["style"] = (uint)t.GetProperty("Style")!.GetValue(item)!,
                ["ex_style"] = (uint)t.GetProperty("ExStyle")!.GetValue(item)!,
            });
        }
        return result;
    }

    private static (int, int) W9e2Size(object boxed) => ((int)boxed.GetType().GetField("Item1")!.GetValue(boxed)!, (int)boxed.GetType().GetField("Item2")!.GetValue(boxed)!);

    public static JsonArray W9E2ExternalEditorsDialogCases(JsonObject inputs)
    {
        Type dialogType = W9e2Type("ExternalEditorsDialog");
        var output = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "dialog":
                {
                    object dialog = W9e2Dialog(c);
                    c["title"] = (string)W9e2Get(dialog, "Title")!;
                    c["items"] = W9e2Items((IEnumerable)W9e2Get(dialog, "Items")!);
                    c["initial_focus_id"] = (int)W9e2Get(dialog, "InitialFocusId")!;
                    c["default_button_id"] = (int)W9e2Get(dialog, "DefaultButtonId")!;
                    c["resizable"] = (bool)W9e2Get(dialog, "Resizable")!;
                    var headers = new JsonArray();
                    foreach (int id in (IEnumerable<int>)W9e2Get(dialog, "HeaderControlIds")!) headers.Add(id);
                    c["header_ids"] = headers;
                    c["uses_modal_frame"] = (bool)W9e2Get(dialog, "UsesModalFrame")!;
                    c["extra_window_style"] = (uint)W9e2Get(dialog, "ExtraWindowStyle")!;
                    break;
                }
                case "row_text":
                {
                    object dialog = W9e2Dialog(c);
                    object tuple = dialogType.GetMethod("RowText", W9e2Flags)!.Invoke(dialog, [c["index"]!.GetValue<int>()])!;
                    c["text"] = (string)tuple.GetType().GetField("Item1")!.GetValue(tuple)!;
                    c["badge"] = (string?)tuple.GetType().GetField("Item2")!.GetValue(tuple);
                    break;
                }
                case "preferred":
                {
                    object dialog = W9e2Bare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    var preferred = W9e2Size(dialogType.GetMethod("PreferredClientSize", W9e2Flags)!.Invoke(dialog, null)!);
                    var minimum = W9e2Size(dialogType.GetMethod("MinimumClientSize", W9e2Flags)!.Invoke(dialog, null)!);
                    c["preferred_width"] = preferred.Item1; c["preferred_height"] = preferred.Item2;
                    c["minimum_width"] = minimum.Item1; c["minimum_height"] = minimum.Item2;
                    break;
                }
                case "metrics":
                {
                    object dialog = W9e2Bare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    c["margin"] = (int)W9e2Get(dialog, "Margin")!;
                    c["field_height"] = (int)W9e2Get(dialog, "FieldHeight")!;
                    c["button_height"] = (int)W9e2Get(dialog, "ButtonHeight")!;
                    c["left_width"] = (int)dialogType.GetMethod("LeftWidth", W9e2Flags)!.Invoke(dialog, [c["client_width"]!.GetValue<int>()])!;
                    break;
                }
                case "constants":
                {
                    var values = new JsonObject();
                    foreach (FieldInfo field in dialogType.GetFields(W9e2Flags).Where(f => f.IsLiteral && f.FieldType == typeof(int) && f.Name.EndsWith("Id", StringComparison.Ordinal)))
                    {
                        string key = string.Concat(field.Name.Select((ch, i) => i > 0 && char.IsUpper(ch) ? "_" + char.ToLowerInvariant(ch) : char.ToLowerInvariant(ch).ToString()));
                        values[key] = (int)field.GetRawConstantValue()!;
                    }
                    c["values"] = values;
                    break;
                }
                case "effective_default":
                {
                    object dialog = W9e2Dialog(c);
                    c["effective_default_id"] = (string)dialogType.GetMethod("EffectiveDefaultId", W9e2Flags)!.Invoke(dialog, null)!;
                    c["index_of_default"] = (int)dialogType.GetMethod("IndexOfDefault", W9e2Flags)!.Invoke(dialog, null)!;
                    break;
                }
                default: throw new InvalidOperationException("unknown dialog-external-editors kind");
            }
            output.Add(c.DeepClone());
        }
        return output;
    }
}
