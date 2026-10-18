using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;

// W8-05c: KeyIdMapping.FromVirtualKey, KeyIdMapping.ToVirtualKey and KeyIdMapping.TryParse (PhotoReview.App.Shared/Input/KeyIdMapping.cs).
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-05c
//     "key-id-mapping" => Commands.KeyIdMappingCases(inputs),
// Wiring (the lead does this; the writer did not run the oracle): as W8-01b. App.Shared is net10.0-windows; the three entry points are
// `public static`, so no reflection beyond the one field-name check below and no InternalsVisibleTo.
// No ambient state: no clock, no cwd, no WPF. The WPF cross-check of App.Tests (KeyInterop) is NOT replayed here: the table itself is the spec.
//
// Input: golden-inputs/key-id-mapping.json = { "virtual_keys": [int], "key_values": [int], "names": [string] (KeyId field names in
//   declaration order, aliases included; checked against reflection), "name_probes": [string] }.
// Output: a JSON array of objects with "kind":
//   "enum-fields"  { "count", "names_equal" }   names_equal: input "names" equals Enum field names (reflection) in order;
//   "from-vk"      { "vk", "extended", "key" }  key = (int)KeyIdMapping.FromVirtualKey(vk, extended);
//   "to-vk"        { "key", "vk" }              vk = KeyIdMapping.ToVirtualKey((KeyId)key);
//   "try-parse"    { "name", "ok", "key" }      key = (int)key when ok, 0 when not ok (C# sets None on failure).
// Each int is a plain JSON number (no exception path exists: every entry point returns a value).

static partial class Commands
{
    public static JsonArray KeyIdMappingCases(JsonObject inputs)
    {
        var result = new JsonArray();

        var names = inputs["names"]!.AsArray().Select(node => node!.GetValue<string>()).ToArray();
        var fields = typeof(KeyId).GetFields(BindingFlags.Public | BindingFlags.Static).Select(field => field.Name).ToArray();
        result.Add(new JsonObject
        {
            ["kind"] = "enum-fields",
            ["count"] = fields.Length,
            ["names_equal"] = names.SequenceEqual(fields),
        });

        foreach (var node in inputs["virtual_keys"]!.AsArray())
        {
            var vk = node!.GetValue<int>();
            foreach (var extended in new[] { false, true })
            {
                var key = KeyIdMapping.FromVirtualKey(vk, extended);
                result.Add(new JsonObject { ["kind"] = "from-vk", ["vk"] = vk, ["extended"] = extended, ["key"] = (int)key });
            }
        }

        foreach (var node in inputs["key_values"]!.AsArray())
        {
            var value = node!.GetValue<int>();
            result.Add(new JsonObject { ["kind"] = "to-vk", ["key"] = value, ["vk"] = KeyIdMapping.ToVirtualKey((KeyId)value) });
        }

        foreach (var node in inputs["name_probes"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            var ok = KeyIdMapping.TryParse(name, out var key);
            result.Add(new JsonObject { ["kind"] = "try-parse", ["name"] = name, ["ok"] = ok, ["key"] = ok ? (int)key : 0 });
        }

        return result;
    }
}
