using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using Microsoft.Extensions.DependencyInjection;

// W8-17c oracle (the F half of the unit): WHICH services the baseline shell composition root registers, in which order and with which lifetime. The Rust root has no DI
// container, so the golden is the checklist that `pr_shell::startup::composition_root::ShellServices` is mapped against (a test compares the list of host-provided
// registrations with a hand-written map `csharp type -> Rust field`). NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-17c
//     "shell-composition-registrations" => Commands.W817cRegistrationsCases(inputs),
// Wiring (the lead does this): none for the csproj. OracleCs.csproj already references PhotoReview.dll (assembly "PhotoReview" = PhotoReview.Shell.Win32) and
// Microsoft.Extensions.DependencyInjection (W8-18o). `ShellCompositionRoot`, `ShellHostServices`, `ShellPresentationHub` and `Win32UiDispatcher` are `internal` and reached by
// reflection (no InternalsVisibleTo, no change to the C# repo). The `Win32UiDispatcher` creates its hidden message window on the calling thread (the C# test does the same
// in its constructor); the service provider is NEVER built, so no service is resolved and no dialog, window or GPU object is created.
//
// Inputs (tests/golden-inputs/shell-composition-registrations.json): { "cases": [ { "kind": "registrations", "id": ... } ] } (one case).
// Output of the "registrations" case: "count" (int) and "descriptors": [ { "index", "service_type" (FullName of the service type; generic types in the .NET `Name`1[[..]]
// form are replaced by their `Name` before the backtick), "lifetime" ("Singleton"/"Scoped"/"Transient"), "shape" ("instance" | "factory" | "type"),
// "implementation_type" (FullName, only when shape is "type") } ] in registration order: the 12 host registrations of section 1 of `ConfigureServices` come first, then the
// registrations of `AddPhotoReviewShared`, then the two feature lines of `RegisterFeatures` (`AddShellViewing`, `AddShellOverlay`).
static partial class Commands
{
    private const string W817cAssembly = "PhotoReview";
    private const BindingFlags W817cFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W817cType(string fullName) => Assembly.Load(W817cAssembly).GetType(fullName, throwOnError: true)!;

    private static string W817cTypeName(Type? type)
    {
        if (type is null)
        {
            return string.Empty;
        }

        var name = type.FullName ?? type.Name;
        var tick = name.IndexOf('`', StringComparison.Ordinal);
        return tick < 0 ? name : name[..tick];
    }

    public static JsonArray W817cRegistrationsCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var input = node!.AsObject();
            var dispatcherType = W817cType("PhotoReview.Shell.Win32.Hosting.Win32UiDispatcher");
            // The C# test calls `new Win32UiDispatcher()`: the constructor has an optional parameter, so bind with OptionalParamBinding.
            var dispatcher = dispatcherType.InvokeMember(string.Empty,
                BindingFlags.CreateInstance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.OptionalParamBinding,
                null, null, [])!;
            try
            {
                var hostType = W817cType("PhotoReview.Shell.Win32.Startup.ShellHostServices");
                // A record also has a (private/protected) copy constructor: pick the positional one by its three parameters.
                var host = hostType.GetConstructors(W817cFlags).Single(k => !k.IsStatic && k.GetParameters().Length == 3)
                    .Invoke([dispatcher, (Func<nint>)(() => 0), null]);
                var hub = Activator.CreateInstance(W817cType("PhotoReview.Shell.Win32.Startup.ShellPresentationHub"), nonPublic: true)!;
                var services = new ServiceCollection();
                W817cType("PhotoReview.Shell.Win32.Startup.ShellCompositionRoot")
                    .GetMethod("ConfigureServices", W817cFlags)!
                    .Invoke(null, [services, host, hub]);

                var descriptors = new JsonArray();
                var index = 0;
                foreach (var descriptor in services)
                {
                    var shape = descriptor.ImplementationInstance is not null ? "instance"
                        : descriptor.ImplementationFactory is not null ? "factory"
                        : "type";
                    var item = new JsonObject
                    {
                        ["index"] = index++,
                        ["service_type"] = W817cTypeName(descriptor.ServiceType),
                        ["lifetime"] = descriptor.Lifetime.ToString(),
                        ["shape"] = shape,
                    };
                    if (shape == "type")
                    {
                        item["implementation_type"] = W817cTypeName(descriptor.ImplementationType);
                    }

                    descriptors.Add(item);
                }

                var entry = (JsonObject)input.DeepClone();
                entry["count"] = index;
                entry["descriptors"] = descriptors;
                result.Add(entry);
            }
            finally
            {
                (dispatcher as IDisposable)?.Dispose();
            }
        }

        return result;
    }
}
