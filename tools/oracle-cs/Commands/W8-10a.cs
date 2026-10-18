using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.App;
using PhotoReview.Core.IO;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Core.Settings;

// W8-10a golden generator (contract: contracts/W8-10a.md, "Golden package").
// NOT RUN by the writer; the lead wires and runs it.
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W8-10a (Presenters: Action Profiles and the action list JSON)
//     "action-list-json" => Commands.ActionListJsonCases(inputs),
//     "action-profiles" => Commands.ActionProfilesCases(inputs),
//     "action-profiles-io" => Commands.ActionProfilesIoCases(inputs),
//
// "vi" reads the languages folder through Commands.FormatInputsDir, which W8-02b sets in Program.cs (same folder, same rule).
// Build: tools/oracle.ps1 builds PhotoReview.App.Shared from the archive (the same way it builds Imaging.Raw, D55) and
// copies it next to oracle-cs.exe; OracleCs.csproj references it. The presenter and ActionListJson are `internal`, so they
// are reached by reflection (public static or public instance members of internal types). The C# repo is not touched.
//
// Golden semantics (deterministic, machine-independent):
//  * Language: "en" = BuiltInCatalog.EnglishLocalizer; "vi" = LanguageLoader over assets/languages (vi.json, English
//    fallback), set as the ambient Localizer for the case and restored after it (as W8-02b). Tr accessors read it.
//  * AppLog: AppLog.Enabled = false for the whole run; the failure paths log through AppLog and the machine log must not change.
//  * A JsonException is recorded as {"throws": "JsonException"}; ThrowIfInvalid also records its English message (a constant).
//  * TryImport and TryExport: the OS message is never recorded (it differs per machine); the kind is recorded instead
//    ("ok", "invalid", "failed"), plus whether the OS message names the file. Messages that come from Tr are recorded.
//  * Temporary files live in a fresh directory per case under Path.GetTempPath() and are deleted in a finally block;
//    no output contains a path.
//  * A throw other than the ones above makes the case fail on the Rust side.

static partial class Commands
{
    private static readonly Assembly SharedAssembly = Assembly.Load("PhotoReview.App.Shared");
    private static readonly Dictionary<string, Localizer> ActionLocalizers = new();

    private static Type PresenterType(string name) =>
        SharedAssembly.GetType("PhotoReview.App.Presenters." + name, throwOnError: true)!;

    private static object? Invoke(Type type, object? target, string method, params object?[] args)
    {
        var info = type.GetMethod(method, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance)
            ?? throw new MissingMethodException(type.FullName, method);
        try
        {
            return info.Invoke(target, args);
        }
        catch (TargetInvocationException ex) when (ex.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(ex.InnerException).Throw();
            throw;
        }
    }

    private static Localizer ActionLocalizer(string language)
    {
        if (ActionLocalizers.TryGetValue(language, out var known)) return known;
        var localizer = language switch
        {
            "en" => BuiltInCatalog.EnglishLocalizer,
            "vi" => new LanguageLoader(new PhysicalFileSystem(), Path.GetFullPath(Path.Combine(FormatInputsDir, "..", "..", "assets", "languages")), userDir: null).Load("vi"),
            _ => throw new ArgumentException("unknown language " + language),
        };
        ActionLocalizers[language] = localizer;
        return localizer;
    }

    private static T InLanguage<T>(string language, Func<T> body)
    {
        var previous = Localizer.Current;
        Localizer.SetCurrent(ActionLocalizer(language));
        try { return body(); }
        finally { Localizer.SetCurrent(previous); }
    }

    private static ReviewAction ReadAction(JsonNode node) => new()
    {
        Name = (string)node["Name"]!,
        Shortcut = (string)node["Shortcut"]!,
        Operation = Enum.Parse<FileOperationType>((string)node["Operation"]!),
        Destination = (string)node["Destination"]!,
        Confirm = (bool)node["Confirm"]!,
    };

    private static List<ReviewAction> ReadActions(JsonNode? node) =>
        (node?.AsArray() ?? new JsonArray()).Select(item => ReadAction(item!)).ToList();

    private static JsonArray ActionsJson(IEnumerable<ReviewAction> actions) => new(actions.Select(action => (JsonNode)new JsonObject
    {
        ["Name"] = action.Name,
        ["Shortcut"] = action.Shortcut,
        ["Operation"] = action.Operation.ToString(),
        ["Destination"] = action.Destination,
        ["Confirm"] = action.Confirm,
    }).ToArray());

    private static JsonObject Outcome(Func<object?> body)
    {
        try
        {
            return new JsonObject { ["value"] = ToJsonNode(body()) };
        }
        catch (JsonException)
        {
            // Invoke rethrows the inner exception (see Invoke), so the JsonException arrives unwrapped here.
            return new JsonObject { ["throws"] = "JsonException" };
        }
    }

    private static JsonNode? ToJsonNode(object? value) => value switch
    {
        null => null,
        string text => text,
        bool flag => flag,
        int number => number,
        IEnumerable<ReviewAction> actions => ActionsJson(actions),
        _ => throw new InvalidOperationException("unexpected oracle value " + value.GetType()),
    };

    // ---- action-list-json ----

    public static JsonArray ActionListJsonCases(JsonObject inputs)
    {
        var results = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var name = (string)c["name"]!;
            var kind = (string)c["kind"]!;
            var language = (string?)c["lang"] ?? "en";
            var record = new JsonObject { ["name"] = name, ["kind"] = kind };
            InLanguage(language, () =>
            {
                var type = PresenterType("ActionListJson");
                switch (kind)
                {
                    case "serialize":
                        record["text"] = (string?)Invoke(type, null, "Serialize", ReadActions(c["actions"]));
                        break;
                    case "serialize_export":
                        record["text"] = (string?)Invoke(type, null, "SerializeForExport", ReadActions(c["actions"]));
                        break;
                    case "deserialize":
                        record["result"] = Outcome(() => Invoke(type, null, "Deserialize", (string)c["text"]!));
                        break;
                    case "deserialize_or_null":
                        record["result"] = Outcome(() => Invoke(type, null, "DeserializeOrNull", (string)c["text"]!));
                        break;
                    case "throw_if_invalid":
                        try
                        {
                            Invoke(type, null, "ThrowIfInvalid", ReadActions(c["actions"]));
                            record["ok"] = true;
                        }
                        catch (JsonException ex)
                        {
                            record["throws"] = "JsonException";
                            record["message"] = ex.Message;
                        }
                        break;
                    default:
                        throw new ArgumentException("unknown action-list-json kind " + kind);
                }
                return 0;
            });
            results.Add(record);
        }
        return results;
    }

    // ---- action-profiles ----

    private static JsonObject FormJson(object form)
    {
        var type = form.GetType();
        return new JsonObject
        {
            ["name"] = (string)type.GetProperty("Name")!.GetValue(form)!,
            ["shortcut"] = (string)type.GetProperty("Shortcut")!.GetValue(form)!,
            ["operation_index"] = (int)type.GetProperty("OperationIndex")!.GetValue(form)!,
            ["destination"] = (string)type.GetProperty("Destination")!.GetValue(form)!,
            ["confirm"] = (bool)type.GetProperty("Confirm")!.GetValue(form)!,
        };
    }

    private static object MakeForm(JsonObject fields)
    {
        var formType = PresenterType("ActionProfileForm");
        return Activator.CreateInstance(formType,
            (string)fields["name"]!, (string)fields["shortcut"]!, (int)fields["operation_index"]!,
            (string)fields["destination"]!, (bool)fields["confirm"]!)!;
    }

    public static JsonArray ActionProfilesCases(JsonObject inputs)
    {
        var results = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var name = (string)c["name"]!;
            var kind = (string)c["kind"]!;
            var language = (string?)c["lang"] ?? "en";
            var record = new JsonObject { ["name"] = name, ["kind"] = kind };
            InLanguage(language, () =>
            {
                var presenter = PresenterType("ActionProfilesPresenter");
                switch (kind)
                {
                    case "has_invalid":
                        record["value"] = (bool)Invoke(presenter, null, "HasInvalidActions", ReadActions(c["actions"]))!;
                        break;
                    case "next_free":
                        record["value"] = (string)Invoke(presenter, null, "NextFreeShortcut", ReadActions(c["actions"]))!;
                        break;
                    case "destination_problem":
                        record["value"] = (string?)Invoke(presenter, null, "FindDestinationProblem", ReadActions(c["actions"]));
                        break;
                    case "apply":
                    {
                        var instance = Activator.CreateInstance(presenter, ReadActions(c["actions"]))!;
                        var result = Invoke(presenter, instance, "Apply")!;
                        var resultType = result.GetType();
                        record["accepted"] = (bool)resultType.GetProperty("Accepted")!.GetValue(result)!;
                        record["message"] = (string?)resultType.GetProperty("Message")!.GetValue(result);
                        break;
                    }
                    case "script":
                        record["steps"] = RunScript(presenter, c);
                        break;
                    default:
                        throw new ArgumentException("unknown action-profiles kind " + kind);
                }
                return 0;
            });
            results.Add(record);
        }
        return results;
    }

    // Each op: open {index} | open_none | save {fields} | add | remove {index, selected} | replace_all {actions}.
    // Output per step: the value the call returns (form, bool, index of the added row, next index or -1, null).
    private static JsonArray RunScript(Type presenterType, JsonObject c)
    {
        var instance = Activator.CreateInstance(presenterType, ReadActions(c["actions"]))!;
        var steps = new JsonArray();
        foreach (var node in c["ops"]!.AsArray())
        {
            var op = node!.AsObject();
            var actionsProperty = presenterType.GetProperty("Actions")!;
            var list = (List<ReviewAction>)actionsProperty.GetValue(instance)!;
            switch ((string)op["op"]!)
            {
                case "open":
                {
                    var form = Invoke(presenterType, instance, "Open", list[(int)op["index"]!]);
                    steps.Add(form is null ? null : FormJson(form));
                    break;
                }
                case "open_none":
                    Invoke(presenterType, instance, "Open", (object?)null);
                    steps.Add(null);
                    break;
                case "save":
                    steps.Add((bool)Invoke(presenterType, instance, "SaveCurrent", MakeForm((JsonObject)op["fields"]!))!);
                    break;
                case "add":
                {
                    var added = (ReviewAction)Invoke(presenterType, instance, "Add")!;
                    var updated = (List<ReviewAction>)actionsProperty.GetValue(instance)!;
                    steps.Add(updated.IndexOf(added));
                    break;
                }
                case "remove":
                {
                    var target = list[(int)op["index"]!];
                    steps.Add((int)Invoke(presenterType, instance, "Remove", target, (int)op["selected"]!)!);
                    break;
                }
                case "replace_all":
                    Invoke(presenterType, instance, "ReplaceAll", ReadActions(op["actions"]));
                    steps.Add(null);
                    break;
                default:
                    throw new ArgumentException("unknown script op " + op["op"]);
            }
        }
        return new JsonArray { new JsonObject { ["steps"] = steps, ["actions"] = ActionsJson((List<ReviewAction>)presenterType.GetProperty("Actions")!.GetValue(instance)!) } };
    }

    // ---- action-profiles-io ----

    public static JsonArray ActionProfilesIoCases(JsonObject inputs)
    {
        var results = new JsonArray();
        var previousLog = AppLog.Enabled;
        AppLog.Enabled = false;
        try
        {
            foreach (var node in inputs["cases"]!.AsArray())
            {
                var c = node!.AsObject();
                var name = (string)c["name"]!;
                var kind = (string)c["kind"]!;
                var language = (string?)c["lang"] ?? "en";
                var dir = Path.Combine(Path.GetTempPath(), "w8-10a-" + Guid.NewGuid().ToString("N"));
                Directory.CreateDirectory(dir);
                try
                {
                    var record = InLanguage(language, () => kind switch
                    {
                        "try_import" => TryImportCase(c, dir),
                        "try_export" => TryExportCase(c, dir),
                        _ => throw new ArgumentException("unknown action-profiles-io kind " + kind),
                    });
                    record["name"] = name;
                    record["kind"] = kind;
                    results.Add(record);
                }
                finally
                {
                    Directory.Delete(dir, recursive: true);
                }
            }
        }
        finally
        {
            AppLog.Enabled = previousLog;
        }
        return results;
    }

    private static JsonObject TryImportCase(JsonObject c, string dir)
    {
        var fileName = (string)c["file"]!;
        var path = Path.Combine(dir, fileName);
        if (c["file_text"] is JsonNode text) File.WriteAllText(path, (string)text!);
        var presenter = PresenterType("ActionProfilesPresenter");
        // `out error` needs the same argument array to read the written value back; the params helper cannot do that.
        var info = presenter.GetMethod("TryImport", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)!;
        var args = new object?[] { path, null };
        var result = info.Invoke(null, args);
        var error = (string)args[1]!;
        if (result is not null)
        {
            var list = (List<ReviewAction>)result;
            return new JsonObject { ["result"] = "ok", ["count"] = list.Count, ["actions"] = ActionsJson(list), ["error"] = error };
        }
        var invalid = error == Tr.DialogImportActionsInvalidMessage;
        return new JsonObject
        {
            ["result"] = invalid ? "invalid" : "failed",
            ["message"] = invalid ? error : null,
            ["error_names_file"] = error.Contains(fileName, StringComparison.Ordinal),
        };
    }

    private static JsonObject TryExportCase(JsonObject c, string dir)
    {
        var target = (string)c["target"]!;
        var path = target switch
        {
            "file" => Path.Combine(dir, "actions.json"),
            "directory" => dir,
            "missing_folder" => Path.Combine(dir, "nope", "x.json"),
            _ => throw new ArgumentException("unknown export target " + target),
        };
        var presenter = PresenterType("ActionProfilesPresenter");
        var message = (string?)Invoke(presenter, null, "TryExport", path, ReadActions(c["actions"]));
        if (message is not null) return new JsonObject { ["result"] = "failed" };
        return new JsonObject { ["result"] = "ok", ["file_text"] = File.ReadAllText(path) };
    }
}
