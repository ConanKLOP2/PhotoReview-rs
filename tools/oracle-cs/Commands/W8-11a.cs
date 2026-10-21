using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;

// W8-11a: the constants and struct layouts of PhotoReview.Shell.Interop (Win32/*.cs and Shell/*.cs, baseline d92d6aae).
// Command "win32-constants": reads tests/golden-inputs/win32-constants.json and writes tests/golden/win32-constants.json.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-11a
//     "win32-constants" => Commands.W811aWin32ConstantsCases(inputs),
// Wiring: none. OracleCs.csproj already references PhotoReview.Shell.Interop.dll and tools/oracle.ps1 already builds it.
// Every type here is `internal`: reached by reflection over the assembly (no InternalsVisibleTo, no edit of the C# repo).
//
// Inputs: "const_classes" (full type names; every public const/static-readonly field is dumped, in declaration order),
//   "enums" (full names; every enum member), "layouts" (full names of structs; size and field offsets).
// Output cases (each has "kind"):
//   { "kind": "const", "class": "WindowMessages", "name": "WmNull", "type": "UInt32", "value": 0 }
//       integral values are JSON numbers (a C# `nint` constant is the signed 64-bit value, x64); a string constant is a string.
//   { "kind": "enum", "enum": "Tymed", "name": "HGlobal", "value": 1 }      value widened to a signed 64-bit number
//   { "kind": "layout", "type": "Rect", "size": 16, "fields": [ { "name": "Left", "offset": 0 }, ... ] }
//       `size` is Unsafe.SizeOf<T>() (x64); an offset is Marshal.OffsetOf, or null when the runtime cannot marshal the field
//       (a fixed buffer). `class` and `type` and `enum` are the short names (declaring type for a nested one: "ActivationContext+ActCtx").
static partial class Commands
{
    private const string W811aAssembly = "PhotoReview.Shell.Interop";
    private const string W811aNamespace = "PhotoReview.Shell.Interop.";

    private static Type W811aType(string fullName) =>
        Assembly.Load(W811aAssembly).GetType(fullName, throwOnError: true)!;

    private static string W811aShortName(Type type) =>
        type.FullName![W811aNamespace.Length..].Replace("Shell.", string.Empty, StringComparison.Ordinal);

    private static JsonNode? W811aValue(object? raw)
    {
        switch (raw)
        {
            case null:
                return null;
            case string text:
                return JsonValue.Create(text);
            case nint pointer:
                return JsonValue.Create((long)pointer);
            case nuint unsignedPointer:
                return JsonValue.Create((long)(ulong)unsignedPointer);
            default:
                return JsonValue.Create(Convert.ToInt64(raw, CultureInfo.InvariantCulture));
        }
    }

    public static JsonArray W811aWin32ConstantsCases(JsonObject inputs)
    {
        var result = new JsonArray();

        foreach (var node in inputs["const_classes"]!.AsArray())
        {
            var type = W811aType(node!.GetValue<string>());
            var fields = type.GetFields(BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly)
                .OrderBy(f => f.MetadataToken);
            foreach (var field in fields)
            {
                object? value;
                if (field.IsLiteral)
                {
                    value = field.GetRawConstantValue();
                }
                else if (field.IsInitOnly && field.FieldType != typeof(Guid))
                {
                    value = field.GetValue(null);
                }
                else
                {
                    continue; // static readonly Guid fields are not constants (ShellGuids.Iid*): the string constants carry the GUIDs
                }

                result.Add(new JsonObject
                {
                    ["kind"] = "const",
                    ["class"] = W811aShortName(type),
                    ["name"] = field.Name,
                    ["type"] = field.FieldType.Name,
                    ["value"] = W811aValue(value),
                });
            }
        }

        foreach (var node in inputs["enums"]!.AsArray())
        {
            var type = W811aType(node!.GetValue<string>());
            foreach (var field in type.GetFields(BindingFlags.Public | BindingFlags.Static))
            {
                result.Add(new JsonObject
                {
                    ["kind"] = "enum",
                    ["enum"] = W811aShortName(type),
                    ["name"] = field.Name,
                    ["value"] = W811aValue(field.GetRawConstantValue()),
                });
            }
        }

        var sizeOf = typeof(Unsafe).GetMethod(nameof(Unsafe.SizeOf))!;
        foreach (var node in inputs["layouts"]!.AsArray())
        {
            var type = W811aType(node!.GetValue<string>());
            var fields = new JsonArray();
            foreach (var field in type.GetFields(BindingFlags.Public | BindingFlags.Instance))
            {
                JsonNode? offset;
                try
                {
                    offset = JsonValue.Create(Marshal.OffsetOf(type, field.Name).ToInt32());
                }
                catch (ArgumentException)
                {
                    offset = null;
                }

                fields.Add(new JsonObject { ["name"] = field.Name, ["offset"] = offset });
            }

            result.Add(new JsonObject
            {
                ["kind"] = "layout",
                ["type"] = W811aShortName(type),
                ["size"] = (int)sizeOf.MakeGenericMethod(type).Invoke(null, null)!,
                ["fields"] = fields,
            });
        }

        return result;
    }
}
