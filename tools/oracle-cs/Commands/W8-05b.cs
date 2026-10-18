using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Input;

// W8-05b: KeyId (PhotoReview.App.Shared/Input/KeyId.cs, 219 lines), the WPF-free mirror of System.Windows.Input.Key.
// Command "key-id": reads contracts/W8-05b.md golden-inputs/key-id.json (copied to tests/golden-inputs/key-id.json)
// and writes tests/golden/key-id.json.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-05b
//     "key-id" => Commands.KeyIdCases(inputs),
// Wiring (the lead does this; the writer did not run the oracle), as in W8-01b:
//   * tools/oracle-cs/OracleCs.csproj: reference PhotoReview.App.Shared.dll (HintPath $(CSharpCoreDir)\PhotoReview.App.Shared.dll,
//     or a ProjectReference to src\PhotoReview.App.Shared). App.Shared targets net10.0-windows, the oracle is net10.0-windows too.
//   * tools/oracle.ps1: add 'PhotoReview.App.Shared.dll' to $requiredDlls and build src\PhotoReview.App.Shared\PhotoReview.App.Shared.csproj
//     -c Release -o $coreDir.
// KeyId is a public enum: no reflection on internals, no InternalsVisibleTo. No clock, no cwd, no file system.
//
// Output cases (each has "kind"; the Rust test partitions them by kind):
//   { "kind": "declared", "index": i, "name": "Return", "value": 6 }
//       every declared field in GetFields order (declaration order, aliases included), 202 cases.
//   { "kind": "canonical", "value": v, "name": "Return" | null }
//       Enum.GetName((KeyId)v) for v = 0..172. The baseline runtime's answer for an alias is recorded as it is.
//   { "kind": "from_value", "input": n, "defined": bool, "name": string | null }
//       Enum.IsDefined((KeyId)n) for every input in "values"; name = Enum.GetName when defined.
//   { "kind": "from_name", "input": text, "ignore_case": bool, "found": bool, "value": int | null }
//       Enum.TryParse(text, ignoreCase) && Enum.IsDefined(parsed) for every input in "names", in both modes.
//       Numbers and comma lists are never probed (they are not names; see the Rust doc of KeyId::from_name).
static partial class Commands
{
    public static JsonArray KeyIdCases(JsonObject inputs)
    {
        var result = new JsonArray();

        var fields = typeof(KeyId).GetFields(BindingFlags.Public | BindingFlags.Static);
        for (var i = 0; i < fields.Length; i++)
        {
            result.Add(new JsonObject
            {
                ["kind"] = "declared",
                ["index"] = i,
                ["name"] = fields[i].Name,
                ["value"] = Convert.ToInt32(fields[i].GetRawConstantValue(), CultureInfo.InvariantCulture),
            });
        }

        for (var v = 0; v <= 172; v++)
        {
            var key = (KeyId)v;
            result.Add(new JsonObject
            {
                ["kind"] = "canonical",
                ["value"] = v,
                ["name"] = Enum.IsDefined(key) ? Enum.GetName(key) : null,
            });
        }

        foreach (var node in inputs["values"]!.AsArray())
        {
            var raw = node!.GetValue<int>();
            var key = (KeyId)raw;
            var defined = Enum.IsDefined(key);
            result.Add(new JsonObject
            {
                ["kind"] = "from_value",
                ["input"] = raw,
                ["defined"] = defined,
                ["name"] = defined ? Enum.GetName(key) : null,
            });
        }

        foreach (var node in inputs["names"]!.AsArray())
        {
            var text = node!.GetValue<string>();
            foreach (var ignoreCase in new[] { false, true })
            {
                var found = Enum.TryParse(text, ignoreCase, out KeyId parsed) && Enum.IsDefined(parsed);
                result.Add(new JsonObject
                {
                    ["kind"] = "from_name",
                    ["input"] = text,
                    ["ignore_case"] = ignoreCase,
                    ["found"] = found,
                    ["value"] = found ? JsonValue.Create((int)parsed) : null,
                });
            }
        }

        return result;
    }
}
