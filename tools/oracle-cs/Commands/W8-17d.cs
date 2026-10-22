using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Localization;

// W8-17d oracle (the F half of the unit): the environment-driven defaults of ShellStartupOptions, the constants of ShellStartup and the English texts of the second-instance boxes.
// NOT compiled or run by the writer (the lead does). ShellStartup.RunLoop / StartupCoreAsync need a window, a message loop and a real service graph: their expectations are hand-written
// from ShellStartupTests (see contracts/W8-17d.md).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-17d
//     "shell-startup-options" => Commands.W817dStartupOptionsCases(inputs),
// Wiring (the lead does this): none for the csproj. OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" = PhotoReview.Shell.Win32) and PhotoReview.Core.
// `ShellStartupOptions`, `ShellStartup` and `ShellInstanceNames` are `internal` and reached by reflection (no InternalsVisibleTo, no change to the C# repo). Nothing here creates a window.
//
// Inputs (tests/golden-inputs/shell-startup-options.json):
//   "options": [ { "id", "env": { "<VAR>": "<value>" } } ]   the C# variable names (PHOTOREVIEW_SHELL_INSTANCE_PREFIX, PHOTOREVIEW_SHELL_WARP, PHOTOREVIEW_SHELL_NOWARM); the Rust test maps
//                                                            the prefix PHOTOREVIEW_SHELL_ to PHOTOREVIEW_RS_ (R0002, decision D-W8B6-26). Each case clears the three variables, sets the listed
//                                                            ones, builds `new ShellStartupOptions()` and restores the previous values in a finally block.
//   "texts":   [ { "id", "kind": "folder_already_open" | "app_already_running" | "app_title" } ]
//   "constants": [ { "id": "constants" } ]
// Output:
//   options   -> the input case plus "instance_name_prefix", "prefer_warp", "warm_graphics", "activate", "install_process_hooks", "start_perf_listener", "smoke_timeout_ms" and "kind": "options".
//   texts     -> the input case plus "kind": "text", "text" (English, the default localizer).
//   constants -> "kind": "constants", "exit_startup_failed", "exit_smoke_timeout", "max_loop_faults", "explorer_prefetch_timeout_ms", "forward_coalesce_window_ms", "instance_prefix" (ShellInstanceNames.Prefix).
static partial class Commands
{
    private const string W817dAssembly = "PhotoReview";
    private const BindingFlags W817dFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;
    private static readonly string[] W817dVars = ["PHOTOREVIEW_SHELL_INSTANCE_PREFIX", "PHOTOREVIEW_SHELL_WARP", "PHOTOREVIEW_SHELL_NOWARM"];

    private static Type W817dType(string fullName) => Assembly.Load(W817dAssembly).GetType(fullName, throwOnError: true)!;

    private static object? W817dProperty(Type type, object instance, string name) => type.GetProperty(name, W817dFlags)!.GetValue(instance);

    public static JsonArray W817dStartupOptionsCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        var optionsType = W817dType("PhotoReview.Shell.Win32.Startup.ShellStartupOptions");
        var startupType = W817dType("PhotoReview.Shell.Win32.Startup.ShellStartup");

        foreach (var node in inputs["options"]!.AsArray())
        {
            var input = node!.AsObject();
            var saved = W817dVars.ToDictionary(name => name, name => Environment.GetEnvironmentVariable(name));
            try
            {
                foreach (var name in W817dVars)
                {
                    Environment.SetEnvironmentVariable(name, null);
                }

                foreach (var (name, value) in input["env"]!.AsObject())
                {
                    Environment.SetEnvironmentVariable(name, value!.GetValue<string>());
                }

                var options = Activator.CreateInstance(optionsType, nonPublic: true)!;
                var entry = (JsonObject)input.DeepClone();
                entry["kind"] = "options";
                entry["instance_name_prefix"] = (string)W817dProperty(optionsType, options, "InstanceNamePrefix")!;
                entry["prefer_warp"] = (bool)W817dProperty(optionsType, options, "PreferWarp")!;
                entry["warm_graphics"] = (bool)W817dProperty(optionsType, options, "WarmGraphics")!;
                entry["activate"] = (bool)W817dProperty(optionsType, options, "Activate")!;
                entry["install_process_hooks"] = (bool)W817dProperty(optionsType, options, "InstallProcessHooks")!;
                entry["start_perf_listener"] = (bool)W817dProperty(optionsType, options, "StartPerfListener")!;
                entry["smoke_timeout_ms"] = (long)((TimeSpan)W817dProperty(optionsType, options, "SmokeTimeout")!).TotalMilliseconds;
                result.Add(entry);
            }
            finally
            {
                foreach (var (name, value) in saved)
                {
                    Environment.SetEnvironmentVariable(name, value);
                }
            }
        }

        foreach (var node in inputs["texts"]!.AsArray())
        {
            var input = node!.AsObject();
            var kind = input["kind"]!.GetValue<string>();
            var text = kind switch
            {
                "folder_already_open" => Tr.FolderAlreadyOpenInOtherInstance,
                "app_already_running" => Tr.AppAlreadyRunningNoResponse,
                "app_title" => Tr.AppTitle,
                _ => throw new InvalidOperationException("Unknown text kind in the input: " + kind),
            };
            var entry = (JsonObject)input.DeepClone();
            entry["kind"] = "text";
            entry["text"] = text;
            result.Add(entry);
        }

        foreach (var node in inputs["constants"]!.AsArray())
        {
            var entry = (JsonObject)node!.DeepClone();
            entry["kind"] = "constants";
            entry["exit_startup_failed"] = (int)startupType.GetField("ExitStartupFailed", W817dFlags)!.GetRawConstantValue()!;
            entry["exit_smoke_timeout"] = (int)startupType.GetField("ExitSmokeTimeout", W817dFlags)!.GetRawConstantValue()!;
            entry["max_loop_faults"] = (int)startupType.GetField("MaxLoopFaults", W817dFlags)!.GetRawConstantValue()!;
            entry["explorer_prefetch_timeout_ms"] = (long)((TimeSpan)startupType.GetField("ExplorerPrefetchTimeout", W817dFlags)!.GetValue(null)!).TotalMilliseconds;
            entry["forward_coalesce_window_ms"] = (long)((TimeSpan)startupType.GetField("ForwardCoalesceWindow", W817dFlags)!.GetValue(null)!).TotalMilliseconds;
            entry["instance_prefix"] = (string)W817dType("PhotoReview.Shell.Win32.Startup.ShellInstanceNames").GetField("Prefix", W817dFlags)!.GetValue(null)!;
            result.Add(entry);
        }

        return result;
    }
}
