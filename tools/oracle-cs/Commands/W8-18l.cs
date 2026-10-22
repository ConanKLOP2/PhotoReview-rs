using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;

// W8-18l oracle (the G half of the painter): the classification of controls, the predicates, the scale helper, the colour keys and the list view custom-draw
// correction of NativeDialogChromePainter. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18l
//     "painter-kinds" => Commands.W818lPainterKindsCases(inputs),
//
// Wiring (the lead does this; nothing new if W8-15b / W8-18d are wired): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" =
// PhotoReview.Shell.Win32). Every type used here is `internal` or private and is reached by reflection, so no InternalsVisibleTo and no change to the C# repo is
// needed. AllowUnsafeBlocks is already on.
//
// What is NOT covered here and why: the drawing (pixels) is the native tests of W8-18m/18n; BorderThickness and the geometry of the widgets depend on the DPI of the
// process and are hand-written pure tests of the Rust side.
//
// Conventions as W8-18d: every case object is the input case with the expected fields added; Invariant culture; no clock, no cwd, no file system. kind_for needs a
// desktop (it creates and destroys one hidden window per case); everything else is memory only.
//
// Output fields per kind (see tests/golden-inputs/painter-kinds.json notes):
//   kind_for:    kind_name (enum name), value (int), style_read (uint)
//   predicates:  is_full_paint, has_flat_border, tracks_hover (bool)
//   repaints:    value (bool)
//   scaled:      value (int)      [input: scale (double), px (int)]
//   key_color:   found (true) and value (uint), or found (false) and threw (exception type name)
//   custom_draw: [inputs: stage, result_in, item_state_in] result (long), item_state (uint), text_color (uint), back_color (uint)
//   constants:   control_subclass_id, dialog_subclass_id (ulong), control_prop, dialog_prop (string), kinds (object name -> int)

static partial class Commands
{
    private const string W818lAssembly = "PhotoReview";
    private const string W818lNamespace = "PhotoReview.Shell.Win32.Dialogs.Native.";
    private const BindingFlags W818lFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;
    private const int W818lNmLvCustomDrawSize = 136; // x64 NMLVCUSTOMDRAW (the C# code reads at most offset 88)

    [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "CreateWindowExW", SetLastError = true)]
    private static extern nint W818lCreateWindowEx(uint exStyle, string className, string text, uint style, int x, int y, int width, int height,
        nint parent, nint menu, nint instance, nint param);

    [DllImport("user32.dll", EntryPoint = "DestroyWindow", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool W818lDestroyWindow(nint hwnd);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern nint W818lGetWindowLongPtr(nint hwnd, int index);

    private static Type W818lType(string name) => Assembly.Load(W818lAssembly).GetType(W818lNamespace + name, throwOnError: true)!;

    private static MethodInfo W818lMethod(Type type, string name) => type.GetMethod(name, W818lFlags)
        ?? throw new MissingMethodException(type.FullName, name);

    // The Win32 window class name for each DialogControlClass (DialogTemplate.WindowClassName is internal static and used as is, nothing is copied).
    private static string W818lWindowClass(string controlClass)
    {
        Type template = W818lType("DialogTemplate");
        Type classType = W818lType("DialogControlClass");
        return (string)W818lMethod(template, "WindowClassName").Invoke(null, [Enum.Parse(classType, controlClass)])!;
    }

    public static JsonArray W818lPainterKindsCases(JsonObject inputs)
    {
        Type painter = W818lType("NativeDialogChromePainter");
        Type kindType = W818lType("ChromePaintKind");
        Type classType = W818lType("DialogControlClass");
        Type colors = W818lType("DialogColors");
        Type init = W818lType("CommonControlsInit");
        W818lMethod(init, "Ensure").Invoke(null, null);

        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "kind_for":
                {
                    string controlClass = c["class"]!.GetValue<string>();
                    uint style = c["style"]!.GetValue<uint>();
                    nint owner = W818lCreateWindowEx(0, "STATIC", "owner", 0x80000000u /* WS_POPUP */, 0, 0, 200, 100, 0, 0, 0, 0);
                    if (owner == 0)
                    {
                        throw new InvalidOperationException("could not create the hidden owner window");
                    }

                    try
                    {
                        nint hwnd = W818lCreateWindowEx(0, W818lWindowClass(controlClass), "x", 0x40000000u /* WS_CHILD */ | style, 0, 0, 50, 20, owner, 0, 0, 0);
                        if (hwnd == 0)
                        {
                            throw new InvalidOperationException($"could not create a {controlClass} window");
                        }

                        try
                        {
                            object kind = W818lMethod(painter, "KindFor").Invoke(null, [(IntPtr)hwnd, Enum.Parse(classType, controlClass)])!;
                            c["kind_name"] = kind.ToString();
                            c["value"] = Convert.ToInt32(kind, CultureInfo.InvariantCulture);
                            c["style_read"] = (uint)W818lGetWindowLongPtr(hwnd, -16 /* GWL_STYLE */);
                        }
                        finally
                        {
                            W818lDestroyWindow(hwnd);
                        }
                    }
                    finally
                    {
                        W818lDestroyWindow(owner);
                    }

                    break;
                }

                case "predicates":
                {
                    object[] args = [c["paint_kind"]!.GetValue<int>()];
                    c["is_full_paint"] = (bool)W818lMethod(painter, "IsFullPaint").Invoke(null, args)!;
                    c["has_flat_border"] = (bool)W818lMethod(painter, "HasFlatBorder").Invoke(null, args)!;
                    c["tracks_hover"] = (bool)W818lMethod(painter, "TracksHover").Invoke(null, args)!;
                    break;
                }

                case "repaints":
                {
                    c["value"] = (bool)W818lMethod(painter, "RepaintsAfterSystemPaint")
                        .Invoke(null, [c["paint_kind"]!.GetValue<int>(), c["message"]!.GetValue<uint>()])!;
                    break;
                }

                case "scaled":
                {
                    c["value"] = (int)W818lMethod(painter, "S").Invoke(null, [c["scale"]!.GetValue<double>(), c["px"]!.GetValue<int>()])!;
                    break;
                }

                case "key_color":
                {
                    try
                    {
                        c["value"] = (uint)W818lMethod(colors, "FromKey").Invoke(null, [c["key"]!.GetValue<string>()])!;
                        c["found"] = true;
                    }
                    catch (TargetInvocationException ex)
                    {
                        c["found"] = false;
                        c["threw"] = ex.InnerException!.GetType().Name;
                    }

                    break;
                }

                case "custom_draw":
                {
                    IntPtr buffer = Marshal.AllocHGlobal(W818lNmLvCustomDrawSize);
                    try
                    {
                        for (int i = 0; i < W818lNmLvCustomDrawSize; i += 8)
                        {
                            Marshal.WriteInt64(buffer, i, 0);
                        }

                        Marshal.WriteInt32(buffer, 24, (int)c["stage"]!.GetValue<uint>());
                        Marshal.WriteInt32(buffer, 64, (int)c["item_state_in"]!.GetValue<uint>());
                        nint result = c["result_in"]!.GetValue<int>();
                        object answer = W818lMethod(painter, "AdjustListViewCustomDraw").Invoke(null, [(IntPtr)buffer, (IntPtr)result])!;
                        c["result"] = ((IntPtr)answer).ToInt64();
                        c["item_state"] = (uint)Marshal.ReadInt32(buffer, 64);
                        c["text_color"] = (uint)Marshal.ReadInt32(buffer, 80);
                        c["back_color"] = (uint)Marshal.ReadInt32(buffer, 84);
                    }
                    finally
                    {
                        Marshal.FreeHGlobal(buffer);
                    }

                    break;
                }

                case "constants":
                {
                    c["control_subclass_id"] = Convert.ToUInt64(painter.GetField("ControlSubclassId", W818lFlags)!.GetRawConstantValue(), CultureInfo.InvariantCulture);
                    c["dialog_subclass_id"] = Convert.ToUInt64(painter.GetField("DialogSubclassId", W818lFlags)!.GetRawConstantValue(), CultureInfo.InvariantCulture);
                    c["control_prop"] = (string)painter.GetField("ControlProp", W818lFlags)!.GetRawConstantValue()!;
                    c["dialog_prop"] = (string)painter.GetField("DialogProp", W818lFlags)!.GetRawConstantValue()!;
                    var kinds = new JsonObject();
                    foreach (var value in Enum.GetValues(kindType))
                    {
                        kinds[value.ToString()!] = Convert.ToInt32(value, CultureInfo.InvariantCulture);
                    }

                    c["kinds"] = kinds;
                    break;
                }

                default:
                    throw new ArgumentException($"unknown kind '{c["kind"]}'");
            }

            outCases.Add(JsonNode.Parse(c.ToJsonString())!);
        }

        return outCases;
    }
}
