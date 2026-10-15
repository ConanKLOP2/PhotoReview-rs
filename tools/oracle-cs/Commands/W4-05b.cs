using System.Text.Json.Nodes;
using PhotoReview.Core.Settings;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Preload;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-05b
//     "preload-order" => Commands.PreloadOrder(inputs),
//     "preload-window" => Commands.PreloadWindowCases(inputs),
//     "preload-options" => Commands.PreloadOptionsDefaults(inputs),
// WIRING: PreloadWindow / PreloadOrderService / PreloadOptions live in PhotoReview.Imaging.dll, which tools/oracle-cs/OracleCs.csproj
// does not reference yet (same wiring as W4-05a: <Reference Include="PhotoReview.Imaging"> + tools/oracle.ps1 copies the baseline
// build output). AppSettings comes from PhotoReview.Core.dll (already referenced). All three commands are pure and deterministic
// (D19/D27). Do NOT run the oracle in the contract step.

// W4-05b: PreloadOrderService.Build, PreloadWindow, PreloadOptions defaults
static partial class Commands
{
    /// <summary>
    /// Build for each case {name, center, count, full_folder, direction, lead, forward, backward}.
    /// Result: {name, order: [int...]} (the full materialized sequence, in yield order, duplicates preserved).
    /// </summary>
    public static JsonArray PreloadOrder(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var window = new PreloadWindow(c["forward"]!.GetValue<int>(), c["backward"]!.GetValue<int>());
            var order = new JsonArray();
            foreach (var index in PreloadOrderService.Build(
                         c["center"]!.GetValue<int>(),
                         c["count"]!.GetValue<int>(),
                         c["full_folder"]!.GetValue<bool>(),
                         c["direction"]!.GetValue<int>(),
                         c["lead"]!.GetValue<int>(),
                         window))
            {
                order.Add(index);
            }

            result.Add(new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["order"] = order });
        }

        return result;
    }

    /// <summary>
    /// Two groups in one array. `cases` {name, forward, backward}: {name, forward, backward, image_count}.
    /// `settings_cases` {name, forward_count?, backward_count?} (an omitted field keeps the AppSettings default):
    /// {name, kind: "from_settings", forward, backward, equals_default}.
    /// ImageCount is plain unchecked int arithmetic, so the int.MaxValue rows wrap (the baseline build has no checked context).
    /// </summary>
    public static JsonArray PreloadWindowCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var window = new PreloadWindow(c["forward"]!.GetValue<int>(), c["backward"]!.GetValue<int>());
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["forward"] = window.Forward,
                ["backward"] = window.Backward,
                ["image_count"] = window.ImageCount,
            });
        }

        foreach (var node in inputs["settings_cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var settings = new AppSettings();
            if (c["forward_count"] is { } f) settings.PreloadForwardCount = f.GetValue<int>();
            if (c["backward_count"] is { } b) settings.PreloadBackwardCount = b.GetValue<int>();
            var window = PreloadWindow.FromSettings(settings);
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["kind"] = "from_settings",
                ["forward"] = window.Forward,
                ["backward"] = window.Backward,
                ["equals_default"] = window == PreloadWindow.Default,
            });
        }

        return result;
    }

    /// <summary>
    /// `new PreloadOptions()` (optionally `{ Window = new PreloadWindow(forward, backward) }`) for each case {name, forward?, backward?}.
    /// Result: {name, worker_count, memory_load_limit, reserve_bytes, full_folder_threshold_bytes, window_forward, window_backward}.
    /// </summary>
    public static JsonArray PreloadOptionsDefaults(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var options = new PreloadOptions();
            if (c["forward"] is { } f && c["backward"] is { } b)
                options = options with { Window = new PreloadWindow(f.GetValue<int>(), b.GetValue<int>()) };
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["worker_count"] = options.WorkerCount,
                ["memory_load_limit"] = options.MemoryLoadLimit,
                ["reserve_bytes"] = options.ReserveBytes,
                ["full_folder_threshold_bytes"] = options.FullFolderThresholdBytes,
                ["window_forward"] = options.Window.Forward,
                ["window_backward"] = options.Window.Backward,
            });
        }

        return result;
    }
}
