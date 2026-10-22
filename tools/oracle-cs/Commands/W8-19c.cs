using System.Collections;
using System.Linq.Expressions;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19c oracle (the F half of the unit): SettingsPageBuilder (SettingsPageBuilder.cs lines 367-645): the items it creates (class, id, text, style, extended style), the
// text providers, the note/key box/warning/combo/slider registrations, the layout element tree, the WS_GROUP/WS_TABSTOP bookkeeping of radio groups and the errors
// it throws. The layout elements themselves are W8-19b (command settings-layout). Nothing here opens a window. NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19c
//     "settings-builder" => Commands.W819cSettingsBuilderCases(inputs),
//
// Wiring: none beyond the line above (PhotoReview.dll is referenced; the builder and the elements are `internal` and are reached by reflection, as W8-18d). The Group
// callback (an Action<SettingsPageBuilder>) is built with System.Linq.Expressions because the builder type is internal.
//
// Case input: { "name", "kind": "builder", "id_base": int, "ops": [op], "dump": "full" | "counts" (default "full") }
//   op = { "op": "group", "title", "ops": [op] }
//      | { "op": "check", "text", "indent" }                          | { "op": "combo", "label", "items": [text] | null, "width" }
//      | { "op": "edit", "label", "width" }                           | { "op": "slider", "label", "min", "max" }
//      | { "op": "note", "text": text | null, "aligned" }             | { "op": "text", "text": text | null }
//      | { "op": "buttons", "aligned", "texts": [text] }              | { "op": "radio", "texts": [text], "hints": [text | null] | null, "indent" }
//      | { "op": "check_flow", "texts": [text], "indent" }            | { "op": "warning", "text": text | null }
//      | { "op": "key_grid", "specs": [ { "label", "clearable" } ] }
//      | { "op": "repeat", "count": n, "inner": op }                  (the inner op n times; every result is appended)
// Output fields: results (one entry per executed op, in order: an int id, an array of ids, an object {label, control} / {label, slider, value} / [ {label, box, clear} ],
//   or {"threw": "<exception type name>"} when the call threw; a group's inner results are appended after the group's own entry), then the dump:
//   full:   items [ {class (enum name), id, text, style, ex_style} ], texts [ {id, text} ] in id order (the provider invoked), notes, key_boxes, warnings (sorted ids),
//           combo_items [ {id, items} ], sliders [ {id, min, max} ], elements (tree: {type, control_ids, children?}).
//   counts: item_count, text_count, element_count (root elements only).
static partial class Commands
{
    private const string W819cNs = "PhotoReview.Shell.Win32.Dialogs.SettingsUi.";
    private const BindingFlags W819cFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W819cType(string name) => Assembly.Load("PhotoReview").GetType(W819cNs + name, throwOnError: true)!;

    public static JsonArray W819cSettingsBuilderCases(JsonObject inputs)
    {
        Type builderType = W819cType("SettingsPageBuilder");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            object builder = builderType.GetConstructors(W819cFlags).Single(ctor => ctor.GetParameters().Length == 1)
                .Invoke([c["id_base"]!.GetValue<int>()]);
            var results = new JsonArray();
            var runner = new W819cRunner(builderType, results);
            runner.RunAll(builder, c["ops"]!.AsArray());
            c["results"] = results;
            if ((c["dump"]?.GetValue<string>() ?? "full") == "counts")
            {
                c["item_count"] = ((IList)builderType.GetProperty("Items", W819cFlags)!.GetValue(builder)!).Count;
                c["text_count"] = ((ICollection)builderType.GetProperty("Texts", W819cFlags)!.GetValue(builder)!).Count;
                c["element_count"] = ((IList)builderType.GetProperty("Elements", W819cFlags)!.GetValue(builder)!).Count;
            }
            else
            {
                W819cDump(c, builderType, builder);
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static void W819cDump(JsonObject c, Type builderType, object builder)
    {
        object Get(string name) => builderType.GetProperty(name, W819cFlags)!.GetValue(builder)!;
        var items = new JsonArray();
        foreach (var item in (IEnumerable)Get("Items"))
        {
            Type t = item.GetType();
            items.Add(new JsonObject
            {
                ["class"] = t.GetProperty("Class")!.GetValue(item)!.ToString(),
                ["id"] = (int)t.GetProperty("Id")!.GetValue(item)!,
                ["text"] = (string)t.GetProperty("Text")!.GetValue(item)!,
                ["style"] = (uint)t.GetProperty("Style")!.GetValue(item)!,
                ["ex_style"] = (uint)t.GetProperty("ExStyle")!.GetValue(item)!,
            });
        }

        c["items"] = items;
        var texts = new JsonArray();
        foreach (var pair in ((IEnumerable)Get("Texts")).Cast<object>().OrderBy(p => (int)p.GetType().GetProperty("Key")!.GetValue(p)!))
        {
            var provider = (Func<string>)pair.GetType().GetProperty("Value")!.GetValue(pair)!;
            texts.Add(new JsonObject { ["id"] = (int)pair.GetType().GetProperty("Key")!.GetValue(pair)!, ["text"] = provider() });
        }

        c["texts"] = texts;
        c["notes"] = W819cSortedInts((IEnumerable)Get("Notes"));
        c["key_boxes"] = W819cSortedInts((IEnumerable)Get("KeyBoxes"));
        c["warnings"] = W819cSortedInts((IEnumerable)Get("WarningTexts"));
        var combos = new JsonArray();
        foreach (var pair in ((IEnumerable)Get("ComboItems")).Cast<object>().OrderBy(p => (int)p.GetType().GetProperty("Key")!.GetValue(p)!))
        {
            var list = new JsonArray();
            foreach (var provider in (IEnumerable)pair.GetType().GetProperty("Value")!.GetValue(pair)!)
            {
                list.Add(((Func<string>)provider)());
            }

            combos.Add(new JsonObject { ["id"] = (int)pair.GetType().GetProperty("Key")!.GetValue(pair)!, ["items"] = list });
        }

        c["combo_items"] = combos;
        var sliders = new JsonArray();
        foreach (var pair in ((IEnumerable)Get("Sliders")).Cast<object>().OrderBy(p => (int)p.GetType().GetProperty("Key")!.GetValue(p)!))
        {
            var range = pair.GetType().GetProperty("Value")!.GetValue(pair)!;
            sliders.Add(new JsonObject
            {
                ["id"] = (int)pair.GetType().GetProperty("Key")!.GetValue(pair)!,
                ["min"] = (int)range.GetType().GetField("Item1")!.GetValue(range)!,
                ["max"] = (int)range.GetType().GetField("Item2")!.GetValue(range)!,
            });
        }

        c["sliders"] = sliders;
        c["elements"] = W819cTree((IEnumerable)Get("Elements"));
    }

    private static JsonArray W819cSortedInts(IEnumerable values)
    {
        var array = new JsonArray();
        foreach (var v in values.Cast<int>().OrderBy(v => v))
        {
            array.Add(v);
        }

        return array;
    }

    private static JsonArray W819cTree(IEnumerable elements)
    {
        var array = new JsonArray();
        foreach (var element in elements)
        {
            Type t = element.GetType();
            var ids = new JsonArray();
            foreach (var id in (IEnumerable)t.GetProperty("ControlIds", W819cFlags)!.GetValue(element)!)
            {
                ids.Add((int)id);
            }

            var obj = new JsonObject { ["type"] = t.Name, ["control_ids"] = ids };
            if (t.Name == "SettingsGroupElement")
            {
                obj["children"] = W819cTree((IEnumerable)t.GetProperty("Children", W819cFlags)!.GetValue(element)!);
            }

            array.Add(obj);
        }

        return array;
    }
}

/// <summary>Runs the "ops" of a builder case on the internal builder by reflection. Public so the compiled expression can call it.</summary>
public sealed class W819cRunner(Type builderType, JsonArray results)
{
    private const BindingFlags Flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    public void RunAll(object builder, JsonArray ops)
    {
        foreach (var op in ops)
        {
            Run(builder, op!.AsObject());
        }
    }

    // Used by the Group callback (Expression.Call target): the closure gets the group's nested ops through the field below.
    private JsonArray _nested = new();

    public void RunNested(object builder) => RunAll(builder, _nested);

    private static Func<string> Text(JsonNode? node) => node is null ? null! : (() => node.GetValue<string>());

    private static Func<string>? NullableText(JsonNode? node) => node is null ? null : (() => node.GetValue<string>());

    private static Func<string>[] Texts(JsonNode? node) => node!.AsArray().Select(n => (Func<string>)(() => n!.GetValue<string>())).ToArray();

    private MethodInfo Method(string name) => builderType.GetMethod(name, Flags)!;

    private static JsonNode Describe(object? value)
    {
        switch (value)
        {
            case null:
                return JsonValue.Create<string?>(null)!;
            case int i:
                return i;
            case IEnumerable<int> ids:
                return new JsonArray(ids.Select(id => (JsonNode)id).ToArray());
            case IEnumerable cells:
            {
                var array = new JsonArray();
                foreach (var cell in cells)
                {
                    array.Add(Describe(cell));
                }

                return array;
            }

            default:
            {
                var obj = new JsonObject();
                foreach (var property in value.GetType().GetProperties(BindingFlags.Public | BindingFlags.Instance).Where(p => p.PropertyType == typeof(int)))
                {
                    obj[property.Name.ToLowerInvariant()] = (int)property.GetValue(value)!;
                }

                return obj;
            }
        }
    }

    private void Run(object builder, JsonObject op)
    {
        string name = op["op"]!.GetValue<string>();
        try
        {
            switch (name)
            {
                case "repeat":
                {
                    int count = op["count"]!.GetValue<int>();
                    for (int i = 0; i < count; i++)
                    {
                        Run(builder, op["inner"]!.AsObject());
                    }

                    return;
                }

                case "group":
                {
                    var saved = _nested;
                    _nested = op["ops"]!.AsArray();
                    var param = Expression.Parameter(builderType, "b");
                    var call = Expression.Call(Expression.Constant(this), typeof(W819cRunner).GetMethod(nameof(RunNested))!, Expression.Convert(param, typeof(object)));
                    Delegate callback = Expression.Lambda(typeof(Action<>).MakeGenericType(builderType), call, param).Compile();
                    // The group's own entry first, then the inner results (RunNested appends them while Group runs the callback).
                    int index = results.Count;
                    results.Add(null);
                    try
                    {
                        object id = Method("Group").Invoke(builder, [Text(op["title"]), callback])!;
                        results[index] = Describe(id);
                    }
                    catch (TargetInvocationException ex)
                    {
                        // The group box itself could not be added (999-control limit): the content callback never ran.
                        results[index] = new JsonObject { ["threw"] = ex.InnerException!.GetType().Name };
                    }

                    _nested = saved;
                    return;
                }

                case "check":
                    results.Add(Describe(Method("Check").Invoke(builder, [Text(op["text"]), op["indent"]!.GetValue<int>()])));
                    return;
                case "combo":
                {
                    Func<string>[]? items = op["items"] is null ? null : Texts(op["items"]);
                    results.Add(Describe(Method("Combo").Invoke(builder, [Text(op["label"]), items, op["width"]!.GetValue<int>()])));
                    return;
                }

                case "edit":
                    results.Add(Describe(Method("Edit").Invoke(builder, [Text(op["label"]), op["width"]!.GetValue<int>()])));
                    return;
                case "slider":
                    results.Add(Describe(Method("Slider").Invoke(builder, [Text(op["label"]), op["min"]!.GetValue<int>(), op["max"]!.GetValue<int>()])));
                    return;
                case "note":
                    results.Add(Describe(Method("Note").Invoke(builder, [NullableText(op["text"]), op["aligned"]!.GetValue<bool>()])));
                    return;
                case "text":
                    results.Add(Describe(Method("Text").Invoke(builder, [NullableText(op["text"])])));
                    return;
                case "buttons":
                    results.Add(Describe(Method("Buttons").Invoke(builder, [op["aligned"]!.GetValue<bool>(), Texts(op["texts"])])));
                    return;
                case "radio":
                {
                    Func<string>?[]? hints = op["hints"] is null
                        ? null
                        : op["hints"]!.AsArray().Select(n => n is null ? null : (Func<string>)(() => n.GetValue<string>())).ToArray();
                    results.Add(Describe(Method("Radio").Invoke(builder, [Texts(op["texts"]), hints, op["indent"]!.GetValue<int>()])));
                    return;
                }

                case "check_flow":
                    results.Add(Describe(Method("CheckFlow").Invoke(builder, [Texts(op["texts"]), op["indent"]!.GetValue<int>()])));
                    return;
                case "warning":
                    results.Add(Describe(Method("Warning").Invoke(builder, [NullableText(op["text"])])));
                    return;
                case "key_grid":
                {
                    Type specType = builderType.Assembly.GetType("PhotoReview.Shell.Win32.Dialogs.SettingsUi.SettingsKeyCellSpec", true)!;
                    var specs = op["specs"]!.AsArray();
                    Array array = Array.CreateInstance(specType, specs.Count);
                    for (int i = 0; i < specs.Count; i++)
                    {
                        var spec = specs[i]!.AsObject();
                        array.SetValue(specType.GetConstructors(Flags)[0].Invoke([Text(spec["label"]), spec["clearable"]!.GetValue<bool>()]), i);
                    }

                    results.Add(Describe(Method("KeyGrid").Invoke(builder, [array])));
                    return;
                }

                default:
                    throw new InvalidOperationException("unknown builder op " + name);
            }
        }
        catch (TargetInvocationException ex)
        {
            results.Add(new JsonObject { ["threw"] = ex.InnerException!.GetType().Name });
        }
    }
}
