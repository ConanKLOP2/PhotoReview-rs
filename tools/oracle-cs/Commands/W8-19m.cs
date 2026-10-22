using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json.Nodes;

// W8-19m oracle (the pure half of the unit; the plan says N, the contract adds this arithmetic test): the dialog-unit metrics, the preferred and minimum client sizes, the
// viewport size that LayoutControls computes, and the scroll offset book-keeping (SetScroll clamp, line/page/top/bottom bar codes, mouse wheel) of SettingsDialog without a window.
// NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19m
//     "settings-dialog-layout" => Commands.W819mSettingsDialogLayoutCases(inputs),
//
// Needs W8-19j.cs (the window-less dialog driver) and W8-10b.cs. Build and run against the C# BASELINE d92d6aae.
//
// The window-less dialog (see the header of W8-19j.cs) gets the two private fields of the NativeDialog base that the dialog-unit arithmetic reads, as W8-18i does: `_avgCharWidth`
// and `_lineHeight`. Hwnd stays 0, so Item(id) is 0, GetItemText is "", MeasureTextWidth("") is 0 and MeasureItemHeight is 0: every button is exactly Dx(50) wide and every
// measured text height is 0. The Rust test therefore uses a fake `DialogMeasure` with the same properties. `Place` is a no-op without a control, so the placements themselves are
// NOT in this golden (they are hand-derived in the Rust tests); what IS observable is `_viewportWidth` / `_viewportHeight` after LayoutControls.
//
// For the scroll cases the private fields `_active` (0), `_scroll[0]`, `_contentHeight[0]` and `_viewportHeight` are set by reflection; `_attached` stays false, so `SetScroll` stores
// the clamped value and `LayoutPage` returns at once. The thumb codes (SbThumbTrack, SbThumbPosition) read `GetScrollInfo(Item(ScrollId))`, which fails without a control and
// leaves TrackPos at 0: the golden records that (track_pos is always 0 here; the Rust function takes it as a parameter and is tested with other values by hand).
//
// Inputs (tests/golden-inputs/settings-dialog-layout.json), kinds and the fields added:
//   metrics: avg, line (int)                    -> margin, field_height, button_height, scroll_bar_width (the private properties)
//   sizes:   avg, line (int)                    -> preferred_width, preferred_height, minimum_width, minimum_height
//   layout:  avg, line, width, height (int)     -> viewport_width, viewport_height (the private fields after LayoutControls(width, height))
//   scroll:  line (int), content, viewport, start (int), ops [{ op: "set", value } | { op: "wheel", delta } | { op: "bar", code }]
//                                               -> offsets: the scroll offset of page 0 after each op

static partial class Commands
{
    private static void W819mMeasure(object dialog, int avg, int line)
    {
        W819jSetField(dialog, "_avgCharWidth", avg);
        W819jSetField(dialog, "_lineHeight", line);
    }

    public static JsonArray W819mSettingsDialogLayoutCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            object d = W819jNewDialog(null, null, null);
            switch (c["kind"]!.GetValue<string>())
            {
                case "metrics":
                    W819mMeasure(d, c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    c["margin"] = (int)W819jGet(d, "Margin")!;
                    c["field_height"] = (int)W819jGet(d, "FieldHeight")!;
                    c["button_height"] = (int)W819jGet(d, "ButtonHeight")!;
                    c["scroll_bar_width"] = (int)W819jGet(d, "ScrollBarWidth")!;
                    break;

                case "sizes":
                {
                    W819mMeasure(d, c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    var preferred = (ITuple)W819jCall(d, "PreferredClientSize")!;
                    var minimum = (ITuple)W819jCall(d, "MinimumClientSize")!;
                    c["preferred_width"] = (int)preferred[0]!;
                    c["preferred_height"] = (int)preferred[1]!;
                    c["minimum_width"] = (int)minimum[0]!;
                    c["minimum_height"] = (int)minimum[1]!;
                    break;
                }

                case "layout":
                    W819mMeasure(d, c["avg"]!.GetValue<int>(), c["line"]!.GetValue<int>());
                    W819jCall(d, "LayoutControls", c["width"]!.GetValue<int>(), c["height"]!.GetValue<int>());
                    c["viewport_width"] = (int)W819jGet(d, "_viewportWidth")!;
                    c["viewport_height"] = (int)W819jGet(d, "_viewportHeight")!;
                    break;

                case "scroll":
                {
                    W819mMeasure(d, 6, c["line"]!.GetValue<int>());
                    W819jSetField(d, "_active", 0);
                    W819jSetField(d, "_viewportHeight", c["viewport"]!.GetValue<int>());
                    var scroll = (int[])W819jGet(d, "_scroll")!;
                    var content = (int[])W819jGet(d, "_contentHeight")!;
                    content[0] = c["content"]!.GetValue<int>();
                    scroll[0] = c["start"]!.GetValue<int>();
                    var offsets = new JsonArray();
                    foreach (var opNode in c["ops"]!.AsArray())
                    {
                        var op = opNode!.AsObject();
                        switch (op["op"]!.GetValue<string>())
                        {
                            case "set":
                                W819jCall(d, "SetScroll", op["value"]!.GetValue<int>());
                                break;
                            case "wheel":
                                W819jCall(d, "ScrollByWheel", op["delta"]!.GetValue<int>());
                                break;
                            case "bar":
                                W819jCall(d, "ScrollByBar", op["code"]!.GetValue<int>());
                                break;
                            default:
                                throw new InvalidOperationException("unknown scroll op");
                        }

                        offsets.Add(scroll[0]);
                    }

                    c["offsets"] = offsets;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-dialog-layout kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }
}
