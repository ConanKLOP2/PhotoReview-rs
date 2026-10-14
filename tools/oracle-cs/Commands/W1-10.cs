using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PhotoReview.Core.Caching;
using PhotoReview.Core.Catalog;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.FileActions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;
using PhotoReview.Core.Updates;

// W1-10 (from contract oracle-commands.cs)
static partial class Commands
{
// Paste into `static class Commands` in tools/oracle-cs/Program.cs (needs `using PhotoReview.Core.Updates;`).
// switch cases:  "app-version" => Commands.AppVersionCases(inputs),  "update-url-policy" => Commands.UpdateUrlPolicyCases(inputs),

    /// <summary>AppVersion.TryParse (every rejection reason alone), CompareTo and the four relational operators.</summary>
    public static JsonArray AppVersionCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["parse"]!.AsArray())
        {
            var text = node?.GetValue<string>();
            var item = new JsonObject { ["kind"] = "parse", ["text"] = text };
            var ok = AppVersion.TryParse(text, out var v);
            item["ok"] = ok;
            if (ok)
            {
                item["major"] = v.Major;
                item["minor"] = v.Minor;
                item["patch"] = v.Patch;
                item["prerelease"] = v.IsPrerelease;
                item["to_string"] = v.ToString();
            }
            result.Add(item);
        }
        foreach (var pair in inputs["pairs"]!.AsArray())
        {
            var a = pair![0]!.GetValue<string>();
            var b = pair[1]!.GetValue<string>();
            if (!AppVersion.TryParse(a, out var va) || !AppVersion.TryParse(b, out var vb))
                throw new InvalidOperationException($"pair not parsable: {a} / {b}");
            result.Add(new JsonObject
            {
                ["kind"] = "compare",
                ["a"] = a,
                ["b"] = b,
                ["sign"] = Math.Sign(va.CompareTo(vb)),
                ["lt"] = va < vb,
                ["gt"] = va > vb,
                ["le"] = va <= vb,
                ["ge"] = va >= vb,
                ["equal"] = va.Equals(vb),
            });
        }
        return result;
    }

    /// <summary>UpdateUrlPolicy.Validate: the normalized URL, or null.</summary>
    public static JsonArray UpdateUrlPolicyCases(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["urls"]!.AsArray())
        {
            var url = node?.GetValue<string>();
            result.Add(new JsonObject { ["url"] = url, ["result"] = UpdateUrlPolicy.Validate(url) });
        }
        return result;
    }
}
