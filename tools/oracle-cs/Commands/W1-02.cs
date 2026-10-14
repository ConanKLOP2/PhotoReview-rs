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

// W1-02: lenient-enum, flag-enum-json
static partial class Commands
{
    /// <summary>
    /// W1-02. LenientEnumConverter / SettingsEnumConverter behaviour. Input: "enums" (Model enum type names), "tokens" (raw JSON
    /// texts read for EVERY enum). The generator adds, per member: name, lower, UPPER, blank-padded, near-miss (name+"x"),
    /// numeric value as number and as string, and every [JsonAlias] alias in exact/UPPER/padded/near-miss form.
    /// Output kinds: "read" (plain converter, settings converter, dictionary-key read), "custom-default" (converter built with
    /// a non-zero default), "write" (value and dictionary-key text of every member), "write-sentinel" (settings sentinel).
    /// </summary>
    public static JsonArray LenientEnum(JsonObject inputs)
    {
        var result = new JsonArray();
        var fixedTokens = inputs["tokens"]!.AsArray().Select(n => n!.GetValue<string>()).ToList();
        foreach (var node in inputs["enums"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            var type = typeof(LoadingMode).Assembly.GetType("PhotoReview.Core.Model." + name, throwOnError: true)!;
            var settingsOptions = new JsonSerializerOptions();
            settingsOptions.Converters.Add((JsonConverter)Activator.CreateInstance(typeof(SettingsEnumConverter<>).MakeGenericType(type))!);
            var fields = type.GetFields(BindingFlags.Public | BindingFlags.Static);

            var tokens = new List<string>(fixedTokens);
            foreach (var field in fields)
            {
                var member = field.Name;
                var number = Convert.ToInt32(field.GetValue(null), CultureInfo.InvariantCulture).ToString(CultureInfo.InvariantCulture);
                tokens.AddRange([Quote(member), Quote(member.ToLowerInvariant()), Quote(member.ToUpperInvariant()), Quote("  " + member + "\t"),
                                 Quote(member + "x"), number, Quote(number)]);
                foreach (var attr in field.GetCustomAttributes<JsonAliasAttribute>(inherit: false))
                    foreach (var alias in attr.Aliases)
                        tokens.AddRange([Quote(alias), Quote(alias.ToUpperInvariant()), Quote(" " + alias + " "), Quote(alias + "x")]);
            }

            foreach (var token in tokens.Distinct())
            {
                var item = new JsonObject
                {
                    ["kind"] = "read", ["enum"] = name, ["token"] = token,
                    ["plain"] = ReadEnum(type, token, null),
                    ["settings"] = ReadEnum(type, token, settingsOptions),
                };
                if (token.StartsWith('"'))
                    item["key"] = ReadEnumKey(type, token);
                result.Add(item);
            }

            // LenientEnumConverter<T>(defaultValue) with the LAST member as the default (never the zero member).
            var last = fields[^1].GetValue(null)!;
            var customOptions = new JsonSerializerOptions();
            customOptions.Converters.Add((JsonConverter)Activator.CreateInstance(typeof(LenientEnumConverter<>).MakeGenericType(type), last)!);
            foreach (var token in fixedTokens)
                result.Add(new JsonObject
                {
                    ["kind"] = "custom-default", ["enum"] = name, ["default"] = fields[^1].Name, ["token"] = token,
                    ["plain"] = ReadEnum(type, token, customOptions),
                });

            var dictType = typeof(Dictionary<,>).MakeGenericType(type, typeof(int));
            foreach (var field in fields)
            {
                var value = field.GetValue(null)!;
                var dict = (System.Collections.IDictionary)Activator.CreateInstance(dictType)!;
                dict[value] = 1;
                result.Add(new JsonObject
                {
                    ["kind"] = "write", ["enum"] = name, ["member"] = field.Name,
                    ["value_text"] = JsonSerializer.Deserialize<string>(JsonSerializer.Serialize(value, type)),
                    ["key_text"] = JsonNode.Parse(JsonSerializer.Serialize(dict, dictType))!.AsObject().First().Key,
                });
            }

            result.Add(new JsonObject
            {
                ["kind"] = "write-sentinel", ["enum"] = name,
                ["settings_text"] = JsonSerializer.Deserialize<string>(JsonSerializer.Serialize(Enum.ToObject(type, int.MinValue), type, settingsOptions)),
            });
        }
        return result;
    }

    private static string Quote(string text) => JsonSerializer.Serialize(text, Plain);

    /// <summary>Enum member name, "&lt;undefined&gt;" for a value that is no member, "&lt;error&gt;" when Deserialize throws.</summary>
    private static string ReadEnum(Type type, string token, JsonSerializerOptions? options)
    {
        try
        {
            return EnumName(type, JsonSerializer.Deserialize(token, type, options)!);
        }
        catch (Exception e) when (e is JsonException or InvalidOperationException or NotSupportedException)
        {
            return "<error>";
        }
    }

    private static string ReadEnumKey(Type type, string stringToken)
    {
        try
        {
            var dict = (System.Collections.IDictionary)JsonSerializer.Deserialize("{" + stringToken + ":1}", typeof(Dictionary<,>).MakeGenericType(type, typeof(int)))!;
            foreach (var key in dict.Keys) return EnumName(type, key);
            return "<empty>";
        }
        catch (Exception e) when (e is JsonException or InvalidOperationException or NotSupportedException)
        {
            return "<error>";
        }
    }

    private static string EnumName(Type type, object value)
    {
        var number = Convert.ToInt32(value, CultureInfo.InvariantCulture);
        return Enum.IsDefined(type, number) ? Enum.GetName(type, number)! : "<undefined>";
    }

    /// <summary>
    /// W1-02. FlagEnumJson through the public converters of ExifInfoFields and TitleBarFields.
    /// Input: "types" (flag enum names), "tokens" (raw JSON texts), "write_bits" (int values written as the enum).
    /// Output: "read" (bits, or error) and "write" (the JSON string written for the bit pattern).
    /// </summary>
    public static JsonArray FlagEnumJson(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["types"]!.AsArray())
        {
            var name = node!.GetValue<string>();
            var type = typeof(LoadingMode).Assembly.GetType("PhotoReview.Core.Model." + name, throwOnError: true)!;
            foreach (var token in inputs["tokens"]!.AsArray().Select(n => n!.GetValue<string>()))
            {
                var item = new JsonObject { ["kind"] = "read", ["type"] = name, ["token"] = token };
                try
                {
                    item["bits"] = Convert.ToInt32(JsonSerializer.Deserialize(token, type), CultureInfo.InvariantCulture);
                }
                catch (Exception e) when (e is JsonException or InvalidOperationException)
                {
                    item["error"] = true;
                }
                result.Add(item);
            }
            foreach (var bits in inputs["write_bits"]!.AsArray().Select(n => n!.GetValue<int>()))
            {
                result.Add(new JsonObject
                {
                    ["kind"] = "write", ["type"] = name, ["bits"] = bits,
                    ["json"] = JsonSerializer.Deserialize<string>(JsonSerializer.Serialize(Enum.ToObject(type, bits), type)),
                });
            }
        }
        return result;
    }
}
