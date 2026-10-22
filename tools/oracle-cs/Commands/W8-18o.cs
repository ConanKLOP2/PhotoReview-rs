using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using Microsoft.Extensions.DependencyInjection;

// W8-18o oracle (the F half of the secondary window port): which windows the Win32 port and the router claim to run. NOT compiled or run by the writer (the lead
// does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-18o
//     "secondary-window-supports" => Commands.W818oSupportsCases(inputs),
//
// Wiring (the lead does this): OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" = PhotoReview.Shell.Win32) and
// Microsoft.Extensions.DependencyInjection (the PackageReference added for App.Shared). The port, the router and the enum are `internal` and reached by
// reflection: no InternalsVisibleTo and no change to the C# repo.
//
// Nothing here creates a window or shows a dialog: the port is constructed with an empty service provider and only `Supports`, `OpenCount`, `IsBenchmarkOpen` and
// `SettingsEnabled` are read.
//
// Output fields per kind: supports / router_supports -> value (bool); initial -> open_count (int), is_benchmark_open (bool), settings_enabled_value (bool);
// kinds -> names (array of the SecondaryWindowKind names in declaration order), values (array of their ints).

static partial class Commands
{
    private const string W818oAssembly = "PhotoReview";
    private const BindingFlags W818oFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W818oType(string fullName) => Assembly.Load(W818oAssembly).GetType(fullName, throwOnError: true)!;

    private static object W818oPort(string settingsEnabled)
    {
        Type port = W818oType("PhotoReview.Shell.Win32.Dialogs.Native.Win32SecondaryWindowPort");
        ConstructorInfo ctor = port.GetConstructors(W818oFlags).Single(k => !k.IsStatic);
        object?[] args = new object?[ctor.GetParameters().Length];
        args[0] = new ServiceCollection().BuildServiceProvider();
        int index = Array.FindIndex(ctor.GetParameters(), p => p.Name == "settingsEnabled");
        args[index] = settingsEnabled switch
        {
            "true" => new Func<bool>(() => true),
            "false" => new Func<bool>(() => false),
            _ => null,
        };
        return ctor.Invoke(args);
    }

    public static JsonArray W818oSupportsCases(JsonObject inputs)
    {
        Type kindType = W818oType("PhotoReview.Shell.Win32.Dialogs.SecondaryWindowKind");
        Type port = W818oType("PhotoReview.Shell.Win32.Dialogs.Native.Win32SecondaryWindowPort");
        Type router = W818oType("PhotoReview.Shell.Win32.Dialogs.Native.SecondaryWindowRouter");
        Type portInterface = W818oType("PhotoReview.Shell.Win32.Dialogs.ISecondaryWindowPort");

        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "supports":
                {
                    object instance = W818oPort(c["settings_enabled"]!.GetValue<string>());
                    object kind = Enum.Parse(kindType, c["window"]!.GetValue<string>());
                    c["value"] = (bool)port.GetMethod("Supports", W818oFlags)!.Invoke(instance, [kind])!;
                    break;
                }

                case "router_supports":
                {
                    object native = W818oPort(c["settings_enabled"]!.GetValue<string>());
                    object instance = router.GetConstructors(W818oFlags).Single(k => !k.IsStatic).Invoke([native, null]);
                    object kind = Enum.Parse(kindType, c["window"]!.GetValue<string>());
                    c["value"] = (bool)router.GetMethod("Supports", W818oFlags)!.Invoke(instance, [kind])!;
                    _ = portInterface;
                    break;
                }

                case "initial":
                {
                    object instance = W818oPort(c["settings_enabled"]!.GetValue<string>());
                    c["open_count"] = (int)port.GetProperty("OpenCount", W818oFlags)!.GetValue(instance)!;
                    c["is_benchmark_open"] = (bool)port.GetProperty("IsBenchmarkOpen", W818oFlags)!.GetValue(instance)!;
                    c["settings_enabled_value"] = (bool)port.GetProperty("SettingsEnabled", W818oFlags)!.GetValue(instance)!;
                    break;
                }

                case "kinds":
                {
                    var names = new JsonArray();
                    var values = new JsonArray();
                    foreach (var value in Enum.GetValues(kindType))
                    {
                        names.Add(value.ToString());
                        values.Add(Convert.ToInt32(value, CultureInfo.InvariantCulture));
                    }

                    c["names"] = names;
                    c["values"] = values;
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
