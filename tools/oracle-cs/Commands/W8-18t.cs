using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;
using PhotoReview.App;

// W8-18t oracle (the F half of the unit): the control ids, the template of the Recovery dialog (BuildItems), the colour of a tone and the size rules of
// RecoveryDialog (Margin, FieldHeight, ButtonHeight, PreferredClientSize, MinimumClientSize) as functions of the dialog-unit metrics. NOT compiled or run by the
// writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18t
//     "recovery-dialog-layout" => Commands.W818tRecoveryDialogLayoutCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-18i is wired): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" = PhotoReview.Shell.Win32),
// PhotoReview.Core.dll (Tr) and PhotoReview.App.Shared.dll (the public enum RecoveryTone of namespace PhotoReview.App). Every other type is `internal` and is reached by
// reflection: no InternalsVisibleTo, no change to the C# repo.
//
// A window-less dialog: RuntimeHelpers.GetUninitializedObject(typeof(RecoveryDialog)) allocates the object without running a constructor, then the private fields of the
// base class that Dx / Dy / LineHeight / ButtonRowWidth / ButtonWidth read are set by reflection (_avgCharWidth, _lineHeight, an empty _childHosts): the same trick as
// W8-18i. _hwnd stays 0, so Item(id) is 0, GetItemText is "" and MeasureTextWidth is 0: ButtonWidth(ids) is exactly Dx(50) and ButtonRowWidth(ids) is the C# arithmetic of
// W8-18i. BuildItems is a private static method and needs no instance.
//
// Conventions as W8-18i: every output case is the input case with the expected fields added; CurrentCulture is Invariant; the texts are the English catalog (the
// default localizer, as W8-18b); no clock, no cwd, no file system.
//
// Inputs (tests/golden-inputs/recovery-dialog-layout.json), kinds and the fields added:
//   constants:  (none)                           -> ids (object: snake_case name -> int, the 28 control ids), ss_center (uint), cursor_arrow, cursor_wait (int)
//   items:      (none)                           -> items (array of { class (enum name), id, text, style (uint), ex_style (uint) }) in template order
//   color_ref:  tone (RecoveryTone name)         -> color_ref (uint): the private-looking but internal static RecoveryDialog.ToColorRef(tone)
//   sizes:      avg (int), line (int)            -> margin, field_height, button_height, preferred_w, preferred_h, minimum_w, minimum_h (int)

static partial class Commands
{
    private const string W818tAssembly = "PhotoReview";
    private const string W818tNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818tFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static readonly string[] W818tIdNames =
    [
        "HeaderId", "SummaryId", "FilterLabelId", "FilterId", "RecheckId", "CheckStatusId", "ListId", "FilterEmptyId", "DetailsEmptyId", "VerdictLabelId",
        "VerdictBadgeId", "ErrorId", "SourceTitleId", "SourceStatusId", "SourcePathId", "CopySourceId", "ShowSourceId", "DestinationTitleId", "DestinationStatusId",
        "DestinationPathId", "CopyDestinationId", "ShowDestinationId", "NoDestinationId", "DetailsId", "ClearSelectedId", "ClearAllId", "RetryId", "CloseId",
    ];

    private static Type W818tType(string name) => Assembly.Load(W818tAssembly).GetType(W818tNamespace + name, throwOnError: true)!;

    private static object W818tBare(int avg, int line)
    {
        Type baseType = W818tType("NativeDialog");
        object dialog = RuntimeHelpers.GetUninitializedObject(W818tType("RecoveryDialog"));
        baseType.GetField("_avgCharWidth", W818tFlags)!.SetValue(dialog, avg);
        baseType.GetField("_lineHeight", W818tFlags)!.SetValue(dialog, line);
        baseType.GetField("_childHosts", W818tFlags)!.SetValue(dialog, new List<nint>());
        return dialog;
    }

    // "HeaderId" -> "header_id"
    private static string W818tSnake(string pascal)
    {
        var text = new System.Text.StringBuilder();
        for (int i = 0; i < pascal.Length; i++)
        {
            if (i > 0 && char.IsUpper(pascal[i]))
            {
                text.Append('_');
            }

            text.Append(char.ToLowerInvariant(pascal[i]));
        }

        return text.ToString();
    }

    private static long W818tRaw(FieldInfo field) =>
        field.GetRawConstantValue() is IntPtr p ? p.ToInt64() : Convert.ToInt64(field.GetRawConstantValue(), CultureInfo.InvariantCulture);

    private static (int, int) W818tPair(Type type, object dialog, string method) => ((int, int))type.GetMethod(method, W818tFlags)!.Invoke(dialog, null)!;

    public static JsonArray W818tRecoveryDialogLayoutCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Type dialogType = W818tType("RecoveryDialog");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "constants":
                {
                    var ids = new JsonObject();
                    foreach (string name in W818tIdNames)
                    {
                        ids[W818tSnake(name)] = (int)W818tRaw(dialogType.GetField(name, W818tFlags)!);
                    }

                    c["ids"] = ids;
                    c["ss_center"] = (uint)W818tRaw(dialogType.GetField("SsCenter", W818tFlags)!);
                    c["cursor_arrow"] = (int)W818tRaw(dialogType.GetField("CursorArrow", W818tFlags)!);
                    c["cursor_wait"] = (int)W818tRaw(dialogType.GetField("CursorWait", W818tFlags)!);
                    break;
                }

                case "items":
                {
                    var list = (System.Collections.IEnumerable)dialogType.GetMethod("BuildItems", W818tFlags)!.Invoke(null, null)!;
                    var items = new JsonArray();
                    foreach (object item in list)
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
                    break;
                }

                case "color_ref":
                {
                    var tone = Enum.Parse<RecoveryTone>(c["tone"]!.GetValue<string>());
                    c["color_ref"] = (uint)dialogType.GetMethod("ToColorRef", W818tFlags)!.Invoke(null, [tone])!;
                    break;
                }

                case "sizes":
                {
                    object d = W818tBare(c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    c["margin"] = (int)dialogType.GetProperty("Margin", W818tFlags)!.GetValue(d)!;
                    c["field_height"] = (int)dialogType.GetProperty("FieldHeight", W818tFlags)!.GetValue(d)!;
                    c["button_height"] = (int)dialogType.GetProperty("ButtonHeight", W818tFlags)!.GetValue(d)!;
                    (int pw, int ph) = W818tPair(dialogType, d, "PreferredClientSize");
                    (int mw, int mh) = W818tPair(dialogType, d, "MinimumClientSize");
                    c["preferred_w"] = pw;
                    c["preferred_h"] = ph;
                    c["minimum_w"] = mw;
                    c["minimum_h"] = mh;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown recovery-dialog-layout kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
