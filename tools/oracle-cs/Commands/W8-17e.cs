using System.ComponentModel;
using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.Core.Localization;

// W8-17e oracle (the F half of the unit): the English texts of the start-up boxes (W-11) and the rule of ShellStartup.CloseSecondaryWindowsUnlessCancelled. NOT compiled or run by the writer
// (the lead does). The disposal ORDER, the crash handling and the window creation need a window, a message loop and a real service graph: their expectations are hand-written from
// ShellStartupTests (see contracts/W8-17e.md).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-17e
//     "shell-startup-texts" => Commands.W817eStartupTextsCases(inputs),
// Wiring (the lead does this): none for the csproj. OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" = PhotoReview.Shell.Win32) and PhotoReview.Core. `ShellStartup` is
// `internal`; its `CloseSecondaryWindowsUnlessCancelled` is `internal static` and reached by reflection. `Tr` is public (PhotoReview.Core.Localization), English by default.
//
// Inputs (tests/golden-inputs/shell-startup-texts.json):
//   "failed":   [ { "id", "message" } ]           -> "kind": "startup_failed", "text": Tr.AppStartupFailed(message)
//   "recovery": [ { "id", "count" } ]             -> "kind": "recovery_prompt", "title": Tr.DialogStartupRecoveryFailedTitle, "message": Tr.DialogStartupRecoveryFailedMessage(count)
//   "closing":  [ { "id", "cancel": bool } ]      -> "kind": "closing", "closed": bool (did the `close` callback run)
static partial class Commands
{
    private const string W817eAssembly = "PhotoReview";
    private const BindingFlags W817eFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    public static JsonArray W817eStartupTextsCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();

        foreach (var node in inputs["failed"]!.AsArray())
        {
            var input = node!.AsObject();
            var entry = (JsonObject)input.DeepClone();
            entry["kind"] = "startup_failed";
            entry["text"] = Tr.AppStartupFailed(input["message"]!.GetValue<string>());
            result.Add(entry);
        }

        foreach (var node in inputs["recovery"]!.AsArray())
        {
            var input = node!.AsObject();
            var entry = (JsonObject)input.DeepClone();
            entry["kind"] = "recovery_prompt";
            entry["title"] = Tr.DialogStartupRecoveryFailedTitle;
            entry["message"] = Tr.DialogStartupRecoveryFailedMessage(input["count"]!.GetValue<int>());
            result.Add(entry);
        }

        var startupType = Assembly.Load(W817eAssembly).GetType("PhotoReview.Shell.Win32.Startup.ShellStartup", throwOnError: true)!;
        var closing = startupType.GetMethod("CloseSecondaryWindowsUnlessCancelled", W817eFlags)!;
        foreach (var node in inputs["closing"]!.AsArray())
        {
            var input = node!.AsObject();
            var closed = false;
            var args = new CancelEventArgs(input["cancel"]!.GetValue<bool>());
            closing.Invoke(null, [args, (Action)(() => closed = true)]);
            var entry = (JsonObject)input.DeepClone();
            entry["kind"] = "closing";
            entry["closed"] = closed;
            result.Add(entry);
        }

        return result;
    }
}
