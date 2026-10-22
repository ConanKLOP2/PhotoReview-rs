using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W8-18i oracle (the F half of the unit; kinds "style" and "constants" are also read by W8-18h and W8-18j): the dialog-unit arithmetic, the button row width and the
// window style words of the NativeDialog base class. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18i
//     "dialog-layout" => Commands.W818iDialogLayoutCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-15b / W8-18d are wired): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" =
// PhotoReview.Shell.Win32). Every type used here is `internal` and is reached by reflection, so no InternalsVisibleTo and no change to the C# repo is needed.
//
// How a window-less NativeDialog is obtained: RuntimeHelpers.GetUninitializedObject(typeof(ListDialog)) allocates the object WITHOUT running any constructor or
// field initializer, then the private fields the arithmetic reads are set by reflection: _avgCharWidth, _lineHeight and (so that Item(id) can loop over it) an empty
// _childHosts list. _hwnd stays 0, so Item(id) is 0, GetItemText is "" and MeasureTextWidth measures an empty string on the screen DC (width 0): ButtonWidth(ids)
// is exactly Dx(50). Nothing here creates a window.
//
// Conventions as W8-18d: every case object is the input case with the expected fields added; Invariant culture; no clock, no cwd, no file system.
//
// Inputs (tests/golden-inputs/dialog-layout.json), kinds and the fields added:
//   dlu:        avg (int), line (int), units (int)                   -> dx, dy (int): protected Dx(units) and Dy(units)
//   button_row: avg, line, count (int, 0..4)                         -> row_width (ButtonRowWidth(ids)), placed (PlaceButtonRow(600, 10, 5, 20, ids)), button_width
//                                                                       (ButtonWidth(ids)); ids are 1..count
//   style:      type ("ListDialog" | "DiagnosticsDialog" | "BenchmarkDialog" | "ZoomPromptDialog")
//                                                                    -> style, ex_style (uint): the private DialogStyle() and DialogExStyle()
//   constants:  (none)                                               -> id_ok, id_cancel, size_minimized, ws_default_dialog, list_view_subclass_id,
//                                                                       item_tip_subclass_id, idc_wait

static partial class Commands
{
    private const string W818iAssembly = "PhotoReview";
    private const string W818iNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818iFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818iType(string name) => Assembly.Load(W818iAssembly).GetType(W818iNamespace + name, throwOnError: true)!;

    private static object W818iBare(Type derived, Type baseType, int avg, int line)
    {
        object dialog = RuntimeHelpers.GetUninitializedObject(derived);
        baseType.GetField("_avgCharWidth", W818iFlags)!.SetValue(dialog, avg);
        baseType.GetField("_lineHeight", W818iFlags)!.SetValue(dialog, line);
        baseType.GetField("_childHosts", W818iFlags)!.SetValue(dialog, new List<nint>());
        return dialog;
    }

    // `const nuint` / `const nint` come back from GetRawConstantValue as UIntPtr / IntPtr or as the underlying integer, depending on the compiler: accept both.
    private static ulong W818iU64(object raw) => raw is UIntPtr p ? p.ToUInt64() : Convert.ToUInt64(raw, CultureInfo.InvariantCulture);

    private static long W818iI64(object raw) => raw is IntPtr p ? p.ToInt64() : Convert.ToInt64(raw, CultureInfo.InvariantCulture);

    public static JsonArray W818iDialogLayoutCases(JsonObject inputs)
    {
        Type baseType = W818iType("NativeDialog");
        Type listDialog = W818iType("ListDialog");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "dlu":
                {
                    object d = W818iBare(listDialog, baseType, c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    int units = c["units"]!.GetValue<int>();
                    c["dx"] = (int)baseType.GetMethod("Dx", W818iFlags)!.Invoke(d, [units])!;
                    c["dy"] = (int)baseType.GetMethod("Dy", W818iFlags)!.Invoke(d, [units])!;
                    break;
                }

                case "button_row":
                {
                    object d = W818iBare(listDialog, baseType, c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    int count = c["count"]!.GetValue<int>();
                    int[] ids = Enumerable.Range(1, count).ToArray();
                    c["row_width"] = (int)baseType.GetMethod("ButtonRowWidth", W818iFlags)!.Invoke(d, [ids])!;
                    c["button_width"] = (int)baseType.GetMethod("ButtonWidth", W818iFlags)!.Invoke(d, [ids])!;
                    c["placed"] = (int)baseType.GetMethod("PlaceButtonRow", W818iFlags)!.Invoke(d, [600, 10, 5, 20, ids])!;
                    break;
                }

                case "style":
                {
                    Type derived = W818iType(c["type"]!.GetValue<string>());
                    object d = W818iBare(derived, baseType, 6, 15);
                    c["style"] = (uint)baseType.GetMethod("DialogStyle", W818iFlags)!.Invoke(d, null)!;
                    c["ex_style"] = (uint)baseType.GetMethod("DialogExStyle", W818iFlags)!.Invoke(d, null)!;
                    break;
                }

                case "constants":
                    c["id_ok"] = (int)baseType.GetField("IdOk", W818iFlags)!.GetRawConstantValue()!;
                    c["id_cancel"] = (int)baseType.GetField("IdCancel", W818iFlags)!.GetRawConstantValue()!;
                    c["size_minimized"] = (uint)baseType.GetField("SizeMinimized", W818iFlags)!.GetRawConstantValue()!;
                    c["ws_default_dialog"] = (uint)baseType.GetField("WsDefaultDialog", W818iFlags)!.GetRawConstantValue()!;
                    c["list_view_subclass_id"] = W818iU64(baseType.GetField("ListViewSubclassId", W818iFlags)!.GetRawConstantValue()!);
                    c["item_tip_subclass_id"] = W818iU64(baseType.GetField("ItemTipSubclassId", W818iFlags)!.GetRawConstantValue()!);
                    c["idc_wait"] = W818iI64(baseType.GetField("IdcWait", W818iFlags)!.GetRawConstantValue()!);
                    break;
                default:
                    throw new InvalidOperationException("unknown dialog-layout kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
