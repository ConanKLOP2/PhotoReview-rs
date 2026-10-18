using System.Globalization;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;
using PhotoReview.App.ViewModels;
using PhotoReview.App.Viewport;
using PhotoReview.Shell.Win32.Viewing;

// Destination: tools/oracle-cs/Commands/W8-16e.cs (this draft is moved there by the lead).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-16e
//     "scrollbar-geometry" => Commands.W816eScrollBarGeometryCases(inputs),
//     "scrollbar-repeat-timing" => Commands.W816eRepeatTimingCases(inputs),
//
// Wiring (the lead does this, the writer does not run the oracle):
//   * tools/oracle-cs/OracleCs.csproj: add, inside the existing `<ItemGroup Condition="'$(CSharpSrcDir)' != ''">` (the W7-03d pattern),
//         <Compile Include="$(CSharpSrcDir)\src\PhotoReview.Shell.Win32\Viewing\ScrollBarGeometry.cs" Link="cs-baseline\ScrollBarGeometry.cs" />
//         <Compile Include="$(CSharpSrcDir)\src\PhotoReview.Shell.Win32\Viewing\ScrollBarNative.cs" Link="cs-baseline\ScrollBarNative.cs" />
//     The two files are compiled into the oracle exactly as they are at the baseline (nothing copied by hand, the C# repo is not
//     touched). They are `internal`, which is fine inside the oracle assembly. ScrollBarNative.cs uses `[LibraryImport]`: the oracle
//     already has AllowUnsafeBlocks. They need only PUBLIC types of PhotoReview.App.Shared (PointD, SizeD, RectD, ViewportLayout,
//     ViewportInput, ScrollBarPolicy, ViewerStretchMode, ViewportLayoutEngine), so the App.Shared reference/build that W8-01a adds
//     (OracleCs.csproj Reference + oracle.ps1 build of PhotoReview.App.Shared into $coreDir) is the only other prerequisite.
//
// Conventions (shared with W8-01a, Perf.cs and W1-15.cs):
//   * Inputs: a double is a JSON number, or one of the strings "NaN", "Infinity", "-Infinity", "-0".
//   * Outputs: the same strings for non-finite values and for negative zero, JSON numbers otherwise.
//   * Every case object is the input case with the expected fields added, so the Rust side reads only the golden file.
//   * Culture is pinned to Invariant. No clock, no cwd, no file system, no window: ScrollBarNative.RepeatTiming() and
//     TrackMouseLeave() (the two OS calls) are NOT called, only the pure RepeatTimingFor and the constants.

static partial class Commands
{
    // ---- JSON <-> double ----

    private static double W816eNum(JsonNode? node)
    {
        if (node is JsonValue value && value.TryGetValue(out string? text))
        {
            return text switch
            {
                "NaN" => double.NaN,
                "Infinity" => double.PositiveInfinity,
                "-Infinity" => double.NegativeInfinity,
                "-0" => -0.0,
                _ => double.Parse(text!, NumberStyles.Float, CultureInfo.InvariantCulture),
            };
        }
        return node!.GetValue<double>();
    }

    private static double W816eNumOr(JsonObject o, string key, double fallback) => o[key] is null ? fallback : W816eNum(o[key]);

    private static JsonNode W816eOut(double value)
    {
        if (double.IsNaN(value)) return JsonValue.Create("NaN")!;
        if (double.IsPositiveInfinity(value)) return JsonValue.Create("Infinity")!;
        if (double.IsNegativeInfinity(value)) return JsonValue.Create("-Infinity")!;
        if (value == 0 && double.IsNegative(value)) return JsonValue.Create("-0")!;
        return JsonValue.Create(value)!;
    }

    // ---- records in ----

    private static PointD W816ePoint(JsonObject o) => new(W816eNum(o["x"]), W816eNum(o["y"]));

    private static RectD W816eRect(JsonNode? node) =>
        node is JsonObject o ? new(W816eNumOr(o, "x", 0), W816eNumOr(o, "y", 0), W816eNumOr(o, "width", 0), W816eNumOr(o, "height", 0)) : default;

    private static ScrollBarMetrics W816eMetrics(JsonNode? node)
    {
        var m = new ScrollBarMetrics();
        if (node is not JsonObject o) return m;
        if (o["thickness"] is not null) m = m with { Thickness = W816eNum(o["thickness"]) };
        if (o["track_margin_along"] is not null) m = m with { TrackMarginAlong = W816eNum(o["track_margin_along"]) };
        if (o["track_margin_across"] is not null) m = m with { TrackMarginAcross = W816eNum(o["track_margin_across"]) };
        if (o["min_thumb_length"] is not null) m = m with { MinThumbLength = W816eNum(o["min_thumb_length"]) };
        if (o["thumb_radius"] is not null) m = m with { ThumbRadius = W816eNum(o["thumb_radius"]) };
        return m;
    }

    private static ViewportLayout W816eLayoutIn(JsonObject o) => new(
        W816eNumOr(o, "viewport_width", 0), W816eNumOr(o, "viewport_height", 0),
        W816eNumOr(o, "extent_width", 0), W816eNumOr(o, "extent_height", 0),
        o["horizontal_bar_visible"]?.GetValue<bool>() ?? false, o["vertical_bar_visible"]?.GetValue<bool>() ?? false,
        W816eRect(o["image_rect"]),
        W816eNumOr(o, "max_horizontal_offset", 0), W816eNumOr(o, "max_vertical_offset", 0));

    private static ScrollBarBox W816eBoxIn(JsonObject o) => new(
        Enum.Parse<ScrollAxis>(o["axis"]!.GetValue<string>()),
        o["visible"]?.GetValue<bool>() ?? false,
        W816eRect(o["bar"]), W816eRect(o["track"]), W816eRect(o["thumb"]),
        o["thumb_visible"]?.GetValue<bool>() ?? false,
        W816eNumOr(o, "max_offset", 0), W816eNumOr(o, "offset", 0), W816eNumOr(o, "viewport_length", 0));

    // ---- records out ----

    private static JsonObject W816eRectOut(RectD r) => new()
    {
        ["x"] = W816eOut(r.X),
        ["y"] = W816eOut(r.Y),
        ["width"] = W816eOut(r.Width),
        ["height"] = W816eOut(r.Height),
    };

    private static JsonObject W816eBoxOut(ScrollBarBox b) => new()
    {
        ["axis"] = JsonValue.Create(b.Axis.ToString()),
        ["visible"] = JsonValue.Create(b.Visible),
        ["bar"] = W816eRectOut(b.Bar),
        ["track"] = W816eRectOut(b.Track),
        ["thumb"] = W816eRectOut(b.Thumb),
        ["thumb_visible"] = JsonValue.Create(b.ThumbVisible),
        ["max_offset"] = W816eOut(b.MaxOffset),
        ["offset"] = W816eOut(b.Offset),
        ["viewport_length"] = W816eOut(b.ViewportLength),
        ["track_start"] = W816eOut(b.TrackStart),
        ["track_length"] = W816eOut(b.TrackLength),
        ["thumb_start"] = W816eOut(b.ThumbStart),
        ["thumb_length"] = W816eOut(b.ThumbLength),
        ["travel"] = W816eOut(b.Travel),
    };

    private static JsonObject W816eLayoutOut(ScrollBarLayout l) => new()
    {
        ["vertical"] = W816eBoxOut(l.Vertical),
        ["horizontal"] = W816eBoxOut(l.Horizontal),
        ["corner"] = W816eRectOut(l.Corner),
        ["has_corner"] = JsonValue.Create(l.HasCorner),
        ["any_visible"] = JsonValue.Create(l.AnyVisible),
    };

    private static JsonObject W816eViewportLayoutOut(ViewportLayout l) => new()
    {
        ["viewport_width"] = W816eOut(l.ViewportWidth),
        ["viewport_height"] = W816eOut(l.ViewportHeight),
        ["extent_width"] = W816eOut(l.ExtentWidth),
        ["extent_height"] = W816eOut(l.ExtentHeight),
        ["horizontal_bar_visible"] = JsonValue.Create(l.HorizontalBarVisible),
        ["vertical_bar_visible"] = JsonValue.Create(l.VerticalBarVisible),
        ["image_rect"] = W816eRectOut(l.ImageRect),
        ["max_horizontal_offset"] = W816eOut(l.MaxHorizontalOffset),
        ["max_vertical_offset"] = W816eOut(l.MaxVerticalOffset),
    };

    private static JsonObject W816eMetricsOut(ScrollBarMetrics m) => new()
    {
        ["thickness"] = W816eOut(m.Thickness),
        ["track_margin_along"] = W816eOut(m.TrackMarginAlong),
        ["track_margin_across"] = W816eOut(m.TrackMarginAcross),
        ["min_thumb_length"] = W816eOut(m.MinThumbLength),
        ["thumb_radius"] = W816eOut(m.ThumbRadius),
    };

    private static JsonNode? W816eAxisOut(ScrollAxis? axis) => axis is { } a ? JsonValue.Create(a.ToString()) : null;

    /// <summary>Hit results for every "points" entry: {x, y, part (enum name), axis ("Vertical", "Horizontal" or null)}.</summary>
    private static JsonArray W816eHits(JsonNode? points, ScrollBarLayout layout)
    {
        var hits = new JsonArray();
        if (points is not JsonArray array) return hits;
        foreach (var p in array)
        {
            var point = W816ePoint(p!.AsObject());
            hits.Add(new JsonObject
            {
                ["x"] = W816eOut(point.X),
                ["y"] = W816eOut(point.Y),
                ["part"] = JsonValue.Create(ScrollBarGeometry.HitTest(layout, point).ToString()),
                ["axis"] = W816eAxisOut(ScrollBarGeometry.AxisAt(layout, point)),
            });
        }
        return hits;
    }

    /// <summary>OffsetForThumbStart for every "thumb_starts" entry on both boxes: {thumb_start, vertical, horizontal}.</summary>
    private static JsonArray W816eOffsets(JsonNode? starts, ScrollBarBox vertical, ScrollBarBox horizontal)
    {
        var offsets = new JsonArray();
        if (starts is not JsonArray array) return offsets;
        foreach (var s in array)
        {
            var start = W816eNum(s);
            offsets.Add(new JsonObject
            {
                ["thumb_start"] = W816eOut(start),
                ["vertical"] = W816eOut(ScrollBarGeometry.OffsetForThumbStart(vertical, start)),
                ["horizontal"] = W816eOut(ScrollBarGeometry.OffsetForThumbStart(horizontal, start)),
            });
        }
        return offsets;
    }

    /// <summary>
    /// Command "scrollbar-geometry". Cases by "kind" (every case has "name"):
    ///   "defaults": no input. Adds "metrics" (new ScrollBarMetrics()), "default_min_thumb_length", "none" (ScrollBarLayout.None),
    ///     "hidden_vertical", "hidden_horizontal" (ScrollBarBox.Hidden).
    ///   "compute": "client_size" [w, h] (optional when "engine" is given: then the engine client), the viewport layout either
    ///     as "layout" (the 9 ViewportLayout fields, every key optional, image_rect optional) or as "engine"
    ///     {policy (ScrollBarPolicy name, default Auto), client [w, h], image [w, h], thickness (default 10)}, which runs the real
    ///     ViewportLayoutEngine.Compute exactly like the C# test helper LayoutFor (Stretch None, Max = +Infinity, bitmap = image).
    ///     Optional "h_offset", "v_offset" (default 0), "metrics" (any subset of the 5 metrics fields), "points" [{x, y}],
    ///     "thumb_starts" [number]. Adds "layout" (engine form only: the layout the engine produced), "result" (ScrollBarLayout),
    ///     "hits" ([{x, y, part, axis}]), "offsets" ([{thumb_start, vertical, horizontal}]), "roundtrip" ({vertical, horizontal}:
    ///     OffsetForThumbStart(box, box.ThumbStart) of each box).
    ///   "box_helpers": "box" (ScrollBarBox fields), optional "thumb_starts", "points". Adds "derived" ({track_start, track_length,
    ///     thumb_start, thumb_length, travel}), "offsets" ([{thumb_start, offset}]), "along" ([{x, y, value}]).
    ///   "hit_test": "boxes" {vertical, horizontal (ScrollBarBox fields), corner (rect), has_corner}, "points". Adds "hits".
    ///   "inside": "rect", "points". Adds "results" ([{x, y, inside}]).
    /// </summary>
    public static JsonArray W816eScrollBarGeometryCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["kind"]!.GetValue<string>())
            {
                case "defaults":
                    item["metrics"] = W816eMetricsOut(new ScrollBarMetrics());
                    item["default_min_thumb_length"] = W816eOut(ScrollBarMetrics.DefaultMinThumbLength);
                    item["none"] = W816eLayoutOut(ScrollBarLayout.None);
                    item["hidden_vertical"] = W816eBoxOut(ScrollBarBox.Hidden(ScrollAxis.Vertical));
                    item["hidden_horizontal"] = W816eBoxOut(ScrollBarBox.Hidden(ScrollAxis.Horizontal));
                    break;
                case "compute":
                {
                    ViewportLayout layout;
                    SizeD client;
                    if (c["engine"] is JsonObject engine)
                    {
                        var ec = engine["client"]!.AsArray();
                        var image = engine["image"]!.AsArray();
                        double cw = W816eNum(ec[0]), ch = W816eNum(ec[1]), iw = W816eNum(image[0]), ih = W816eNum(image[1]);
                        var policy = Enum.Parse<ScrollBarPolicy>(engine["policy"]?.GetValue<string>() ?? "Auto");
                        var input = new ViewportInput(cw, ch, W816eNumOr(engine, "thickness", 10), policy, ViewerStretchMode.None,
                            iw, ih, double.PositiveInfinity, double.PositiveInfinity, iw, ih);
                        layout = ViewportLayoutEngine.Compute(input);
                        item["layout"] = W816eViewportLayoutOut(layout);
                        client = new SizeD(cw, ch);
                    }
                    else
                    {
                        layout = W816eLayoutIn(c["layout"]!.AsObject());
                        client = default;
                    }
                    if (c["client_size"] is JsonArray size)
                    {
                        client = new SizeD(W816eNum(size[0]), W816eNum(size[1]));
                    }
                    var metrics = W816eMetrics(c["metrics"]);
                    var geometry = ScrollBarGeometry.Compute(client, layout, W816eNumOr(c, "h_offset", 0), W816eNumOr(c, "v_offset", 0), metrics);
                    item["result"] = W816eLayoutOut(geometry);
                    item["hits"] = W816eHits(c["points"], geometry);
                    item["offsets"] = W816eOffsets(c["thumb_starts"], geometry.Vertical, geometry.Horizontal);
                    item["roundtrip"] = new JsonObject
                    {
                        ["vertical"] = W816eOut(ScrollBarGeometry.OffsetForThumbStart(geometry.Vertical, geometry.Vertical.ThumbStart)),
                        ["horizontal"] = W816eOut(ScrollBarGeometry.OffsetForThumbStart(geometry.Horizontal, geometry.Horizontal.ThumbStart)),
                    };
                    break;
                }
                case "box_helpers":
                {
                    var box = W816eBoxIn(c["box"]!.AsObject());
                    item["derived"] = new JsonObject
                    {
                        ["track_start"] = W816eOut(box.TrackStart),
                        ["track_length"] = W816eOut(box.TrackLength),
                        ["thumb_start"] = W816eOut(box.ThumbStart),
                        ["thumb_length"] = W816eOut(box.ThumbLength),
                        ["travel"] = W816eOut(box.Travel),
                    };
                    var offsets = new JsonArray();
                    if (c["thumb_starts"] is JsonArray starts)
                    {
                        foreach (var s in starts)
                        {
                            var start = W816eNum(s);
                            offsets.Add(new JsonObject
                            {
                                ["thumb_start"] = W816eOut(start),
                                ["offset"] = W816eOut(ScrollBarGeometry.OffsetForThumbStart(box, start)),
                            });
                        }
                    }
                    item["offsets"] = offsets;
                    var along = new JsonArray();
                    if (c["points"] is JsonArray points)
                    {
                        foreach (var p in points)
                        {
                            var point = W816ePoint(p!.AsObject());
                            along.Add(new JsonObject
                            {
                                ["x"] = W816eOut(point.X),
                                ["y"] = W816eOut(point.Y),
                                ["value"] = W816eOut(box.Along(point)),
                            });
                        }
                    }
                    item["along"] = along;
                    break;
                }
                case "hit_test":
                {
                    var b = c["boxes"]!.AsObject();
                    var layout = new ScrollBarLayout(
                        W816eBoxIn(b["vertical"]!.AsObject()), W816eBoxIn(b["horizontal"]!.AsObject()),
                        W816eRect(b["corner"]), b["has_corner"]?.GetValue<bool>() ?? false);
                    item["hits"] = W816eHits(c["points"], layout);
                    break;
                }
                case "inside":
                {
                    var rect = W816eRect(c["rect"]);
                    var results = new JsonArray();
                    foreach (var p in c["points"]!.AsArray())
                    {
                        var point = W816ePoint(p!.AsObject());
                        results.Add(new JsonObject
                        {
                            ["x"] = W816eOut(point.X),
                            ["y"] = W816eOut(point.Y),
                            ["inside"] = JsonValue.Create(ScrollBarGeometry.Inside(rect, point)),
                        });
                    }
                    item["results"] = results;
                    break;
                }
                default:
                    throw new InvalidOperationException("W8-16e scrollbar-geometry: unknown kind");
            }
            result.Add(item);
        }
        return result;
    }

    /// <summary>
    /// Command "scrollbar-repeat-timing". Cases by "kind": "timing" ("keyboard_delay", "keyboard_speed": ints) adds "delay_ms",
    /// "interval_ms" (ScrollBarNative.RepeatTimingFor, C# int arithmetic, unchecked); "constants" adds "wm_mouse_leave",
    /// "wm_cancel_mode" (ScrollBarNative.WmMouseLeave / WmCancelMode).
    /// </summary>
    public static JsonArray W816eRepeatTimingCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            switch (c["kind"]!.GetValue<string>())
            {
                case "timing":
                {
                    var (delay, interval) = ScrollBarNative.RepeatTimingFor(c["keyboard_delay"]!.GetValue<int>(), c["keyboard_speed"]!.GetValue<int>());
                    item["delay_ms"] = JsonValue.Create(delay);
                    item["interval_ms"] = JsonValue.Create(interval);
                    break;
                }
                case "constants":
                    item["wm_mouse_leave"] = JsonValue.Create(ScrollBarNative.WmMouseLeave);
                    item["wm_cancel_mode"] = JsonValue.Create(ScrollBarNative.WmCancelMode);
                    break;
                default:
                    throw new InvalidOperationException("W8-16e scrollbar-repeat-timing: unknown kind");
            }
            result.Add(item);
        }
        return result;
    }
}
