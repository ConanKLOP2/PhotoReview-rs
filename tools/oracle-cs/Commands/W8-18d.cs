using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;

// Destination: tools/oracle-cs/Commands/W8-18d.cs (this draft is moved there by the lead). NOT compiled or run by the writer.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18d
//     "dialog-template" => Commands.W818dDialogTemplateCases(inputs),
//
// Wiring (the lead does this, as for W8-15b; nothing new if W8-15b is already wired):
//   * OracleCs.csproj already references PhotoReview.dll (PhotoReview.Shell.Win32, assembly name "PhotoReview") and oracle.ps1 builds it into $coreDir.
//   * Every type used here is `internal` (DialogTemplate, DialogItem, DialogControlClass, DialogColors, DarkDialogChrome) and is reached by reflection, so no
//     InternalsVisibleTo and no change to the C# repo is needed. DialogColors.FromKey and DarkPalette are only called, never copied.
//
// Conventions (as W8-15b and W8-16e): every case object is the input case with the expected fields added; culture is Invariant; no clock, cwd, file system or
// window: nothing here touches the OS (DarkDialogChrome.ThemeFor and ClassicTheme are pure statics; the chrome instance is never created).
//
// Output fields per kind:
//   build:        bytes_length (int), bytes_hex (lowercase hex, only when the template is at most 4096 bytes), or threw (exception type name).
//                 An input `repeat` {class, count, text} stands for `count` identical items with ids 1..count and style 0 instead of `items`.
//   class_name:   value (ushort of the enum), class_name (string or null), window_class_name (string).
//   theme_for:    theme (string, "" is the classic theme, or null).
//   color_ref:    value (uint). r, g, b are narrowed with a (float) cast before the call.
//   from_key:     found (true) and value (uint), or found (false) and threw.
//   named_colors: window, input, text, text_strong, text_secondary, warning (uint each).
//   constants:    header_bytes, atom_marker, classic_theme.

static partial class Commands
{
    private const string W818dAssembly = "PhotoReview";
    private const string W818dNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818dFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818dType(string name) => Assembly.Load(W818dAssembly).GetType(W818dNamespace + name, throwOnError: true)!;

    private static object W818dItem(Type itemType, Type classType, string cls, int id, string text, uint style, uint exStyle) =>
        itemType.GetConstructor([classType, typeof(int), typeof(string), typeof(uint), typeof(uint)])!
            .Invoke([Enum.Parse(classType, cls), id, text, style, exStyle]);

    public static JsonArray W818dDialogTemplateCases(JsonObject inputs)
    {
        Type template = W818dType("DialogTemplate");
        Type itemType = W818dType("DialogItem");
        Type classType = W818dType("DialogControlClass");
        Type colors = W818dType("DialogColors");
        Type chrome = W818dType("DarkDialogChrome");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "build":
                    W818dBuild(c, template, itemType, classType);
                    break;
                case "class_name":
                {
                    object cls = Enum.Parse(classType, c["class"]!.GetValue<string>());
                    c["value"] = Convert.ToUInt16(cls, CultureInfo.InvariantCulture);
                    c["class_name"] = (string?)template.GetMethod("ClassName", W818dFlags)!.Invoke(null, [cls]);
                    c["window_class_name"] = (string)template.GetMethod("WindowClassName", W818dFlags)!.Invoke(null, [cls])!;
                    break;
                }

                case "theme_for":
                {
                    object cls = Enum.Parse(classType, c["class"]!.GetValue<string>());
                    c["theme"] = (string?)chrome.GetMethod("ThemeFor", W818dFlags)!.Invoke(null, [cls]);
                    break;
                }

                case "color_ref":
                    c["value"] = (uint)colors.GetMethod("ToColorRef", W818dFlags)!.Invoke(null,
                        [(float)c["r"]!.GetValue<double>(), (float)c["g"]!.GetValue<double>(), (float)c["b"]!.GetValue<double>()])!;
                    break;
                case "from_key":
                    try
                    {
                        c["value"] = (uint)colors.GetMethod("FromKey", W818dFlags)!.Invoke(null, [c["key"]!.GetValue<string>()])!;
                        c["found"] = true;
                    }
                    catch (TargetInvocationException ex)
                    {
                        // DialogColors.FromKey -> DarkPalette.Get throws KeyNotFoundException for an unknown key (exact, ordinal, case-sensitive).
                        c["found"] = false;
                        c["threw"] = ex.InnerException!.GetType().Name;
                    }

                    break;
                case "named_colors":
                    c["window"] = (uint)colors.GetProperty("Window", W818dFlags)!.GetValue(null)!;
                    c["input"] = (uint)colors.GetProperty("Input", W818dFlags)!.GetValue(null)!;
                    c["text"] = (uint)colors.GetProperty("Text", W818dFlags)!.GetValue(null)!;
                    c["text_strong"] = (uint)colors.GetProperty("TextStrong", W818dFlags)!.GetValue(null)!;
                    c["text_secondary"] = (uint)colors.GetProperty("TextSecondary", W818dFlags)!.GetValue(null)!;
                    c["warning"] = (uint)colors.GetProperty("Warning", W818dFlags)!.GetValue(null)!;
                    break;
                case "constants":
                    c["header_bytes"] = (int)template.GetField("HeaderBytes", W818dFlags)!.GetRawConstantValue()!;
                    c["atom_marker"] = (ushort)template.GetField("AtomMarker", W818dFlags)!.GetRawConstantValue()!;
                    c["classic_theme"] = (string)chrome.GetField("ClassicTheme", W818dFlags)!.GetRawConstantValue()!;
                    break;
                default:
                    throw new InvalidOperationException("unknown dialog-template kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static void W818dBuild(JsonObject c, Type template, Type itemType, Type classType)
    {
        Array items;
        if (c["repeat"] is JsonObject repeat)
        {
            int count = repeat["count"]!.GetValue<int>();
            items = Array.CreateInstance(itemType, count);
            for (int i = 0; i < count; i++)
            {
                items.SetValue(W818dItem(itemType, classType, repeat["class"]!.GetValue<string>(), i + 1, repeat["text"]!.GetValue<string>(), 0, 0), i);
            }
        }
        else
        {
            var list = c["items"]!.AsArray();
            items = Array.CreateInstance(itemType, list.Count);
            for (int i = 0; i < list.Count; i++)
            {
                var it = list[i]!.AsObject();
                items.SetValue(W818dItem(itemType, classType, it["class"]!.GetValue<string>(), it["id"]!.GetValue<int>(), it["text"]!.GetValue<string>(),
                    it["style"]!.GetValue<uint>(), it["ex_style"]!.GetValue<uint>()), i);
            }
        }

        try
        {
            // Build(string title, uint style, uint exStyle, IReadOnlyList<DialogItem> items): an array is an IReadOnlyList<DialogItem>.
            var bytes = (byte[])template.GetMethod("Build", W818dFlags)!.Invoke(null,
                [c["title"]!.GetValue<string>(), c["style"]!.GetValue<uint>(), c["ex_style"]!.GetValue<uint>(), items])!;
            c["bytes_length"] = bytes.Length;
            if (bytes.Length <= 4096)
            {
                c["bytes_hex"] = Convert.ToHexString(bytes).ToLowerInvariant();
            }
        }
        catch (TargetInvocationException ex)
        {
            c["threw"] = ex.InnerException!.GetType().Name;
        }
    }
}
