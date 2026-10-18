using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;
using PhotoReview.App.Localization;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Diagnostics;
using PhotoReview.Core.IO;
using PhotoReview.Core.Localization;

// W8-04a oracle (I18N L08 picker, L09 translator modes and service, reload-problems text, LocalizationSource lookup).
// Draft written by the contract writer; NOT run. The lead wires it (see the wiring block below).
//
// Dispatch lines for tools/oracle-cs/Program.cs (inside the `command switch`):
//     "language-options" => Commands.W804aLanguageOptionsCases(inputs),
//     "translation-problems" => Commands.W804aTranslationProblemsCases(inputs),
//     "localization-service" => Commands.W804aLocalizationServiceCases(inputs),
//
// tools/oracle-cs/OracleCs.csproj: a Reference to PhotoReview.App.Shared.dll (HintPath $(CSharpAppSharedDir)\...),
// built by tools/oracle.ps1 from the baseline src/PhotoReview.App.Shared project. App.Shared already references Core,
// so PhotoReview.Core.dll and its Languages folder (copied to output by Core's csproj) are in the same build output.
//
// Shipped catalogs: LocalizationService's public constructor reads AppContext.BaseDirectory\Languages (not injectable),
// so the service is built through its INTERNAL constructor (LanguageLoader, IAppPaths, ILog) by reflection. The
// loader gets the shipped folder of the build output (`<output>\Languages`, the same bytes as assets/languages).
//
// Every command runs with CultureInfo.CurrentCulture = Invariant. No command reads the machine culture: the
// `auto` setting is never passed (InstalledUICulture is machine-dependent), only explicit codes.
static partial class Commands
{
    private static string W804aShippedDir => Path.Combine(AppContext.BaseDirectory, "Languages");

    private static Localizer W804aLocalizer(string code) =>
        new LanguageLoader(new PhysicalFileSystem(), W804aShippedDir, null).Load(code, CultureInfo.InvariantCulture);

    private static string W804aMask(string text, string root)
    {
        // The temp root differs per run: replace it. The .NET exception text inside "(...)" is not reproducible: mask it
        // (same rule as the W2-04 oracle, L10nMask).
        text = text.Replace(root, "<ROOT>", StringComparison.Ordinal);
        return System.Text.RegularExpressions.Regex.Replace(
            text, @"^(.*(?:invalid JSON|unreadable text|cannot read|cannot list files)) \(.*\)$", "$1 (*)",
            System.Text.RegularExpressions.RegexOptions.Singleline);
    }

    private sealed class W804aLog : ILog
    {
        public List<string> Warnings { get; } = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) => Warnings.Add(message);
        public void Error(string message, Exception? ex = null) { }
    }

    private sealed class W804aPaths(string configFile) : IAppPaths
    {
        public string ConfigFile => configFile;
        public string JournalFile => throw new NotSupportedException();
        public string SessionsDir => throw new NotSupportedException();
        public string LogFile => throw new NotSupportedException();
        public string PreviewCacheDir => throw new NotSupportedException();
        public string ThumbnailCacheDir => throw new NotSupportedException();
        public string WindowPlacementFile => throw new NotSupportedException();
        // UserLanguagesDir: default interface member = <dir of ConfigFile>\Languages.
    }

    private static LocalizationService W804aService(string configFile, W804aLog log)
    {
        // The internal constructor (LanguageLoader, IAppPaths, ILog) takes a loader with an explicit shipped folder.
        var userDir = Path.Combine(Path.GetDirectoryName(configFile) ?? string.Empty, "Languages");
        var loader = new LanguageLoader(new PhysicalFileSystem(), W804aShippedDir, userDir);
        var ctor = typeof(LocalizationService).GetConstructor(
            BindingFlags.Instance | BindingFlags.NonPublic, null,
            [typeof(LanguageLoader), typeof(IAppPaths), typeof(ILog)], null)
            ?? throw new InvalidOperationException("LocalizationService internal constructor not found");
        return (LocalizationService)ctor.Invoke([loader, new W804aPaths(configFile), log]);
    }

    private static JsonArray W804aStrings(IEnumerable<string> items) => new(items.Select(s => (JsonNode?)s).ToArray());

    private static LanguageInfo[] W804aLanguages(JsonArray array) =>
        array.Select(n => new LanguageInfo(
            n!["code"]!.GetValue<string>(),
            n["name"]!.GetValue<string>(),
            n["nativeName"]!.GetValue<string>(),
            n["isUserProvided"]!.GetValue<bool>())).ToArray();

    /// <summary>LanguageOptions.Build / IndexOf / ToSetting, labels from the named catalog.</summary>
    public static JsonArray W804aLanguageOptionsCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            var languages = W804aLanguages(c["languages"]!.AsArray());
            var localizer = W804aLocalizer(c["localizer"]!.GetValue<string>());
            var options = LanguageOptions.Build(languages, localizer);
            var optionsJson = new JsonArray(options.Select(o => (JsonNode?)new JsonObject
            {
                ["code"] = o.Code,
                ["displayName"] = o.DisplayName,
                ["isAuto"] = o.IsAuto,
            }).ToArray());
            item["build"] = optionsJson;

            if (c["index_of"] is { } probes)
            {
                // probes: array of setting values, null allowed; out = index (-1 when none).
                item["index_of"] = new JsonArray(probes.AsArray().Select(p =>
                {
                    var setting = p?.GetValue<string?>();
                    return (JsonNode?)new JsonObject
                    {
                        ["setting"] = setting,
                        ["index"] = LanguageOptions.IndexOf(options, setting),
                    };
                }).ToArray());
            }

            if (c["to_setting"] is { } picks)
            {
                // picks: { "selected": index or null, "current": string or null }
                item["to_setting"] = new JsonArray(picks.AsArray().Select(p =>
                {
                    var selectedIndex = p!["selected"]?.GetValue<int?>();
                    var current = p["current"]?.GetValue<string?>();
                    var selected = selectedIndex is { } i ? options[i] : null;
                    return (JsonNode?)new JsonObject
                    {
                        ["selected"] = selectedIndex,
                        ["current"] = current,
                        ["out"] = LanguageOptions.ToSetting(selected, current),
                    };
                }).ToArray());
            }
            result.Add((JsonNode?)item);
        }
        return result;
    }

    /// <summary>TranslationProblems.HasProblems / Message. The ambient localizer is set first (C# Tr.* reads it).</summary>
    public static JsonArray W804aTranslationProblemsCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            var warnings = (c["warnings"]?.AsArray() ?? new JsonArray()).Select(w => w!.GetValue<string>()).ToList();
            if (c["repeat"] is { } repeat)
            {
                var prefix = repeat["prefix"]!.GetValue<string>();
                var count = repeat["count"]!.GetValue<int>();
                warnings = Enumerable.Range(1, count).Select(i => prefix + i.ToString("D2", CultureInfo.InvariantCulture)).ToList();
            }
            item["has_problems"] = TranslationProblems.HasProblems(warnings);
            Localizer.SetCurrent(W804aLocalizer(c["localizer"]!.GetValue<string>()));
            item["message"] = TranslationProblems.Message(warnings);
            item["max_listed"] = TranslationProblems.MaxListed;
            result.Add((JsonNode?)item);
        }
        return result;
    }

    /// <summary>LocalizationService: ParseMode, Load in each translator mode, Apply culture rules, blank language, Reload.</summary>
    public static JsonArray W804aLocalizationServiceCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var item = (JsonObject)c.DeepClone();
            var kind = c["kind"]!.GetValue<string>();
            var root = Path.Combine(Path.GetTempPath(), "w8-04a-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(root);
            try
            {
                var log = new W804aLog();
                var configFile = Path.Combine(root, "config.json");
                var startCulture = c["start_culture"]?.GetValue<string>() ?? "en";
                CultureInfo.CurrentUICulture = CultureInfo.GetCultureInfo(startCulture);
                switch (kind)
                {
                    case "parse_mode":
                    {
                        var args = c["args"]!.AsArray().Select(a => a!.GetValue<string>()).ToArray();
                        item["out"] = LocalizationService.ParseMode(args).ToString();
                        break;
                    }
                    case "load":
                    {
                        var service = W804aService(configFile, log);
                        service.Mode = Enum.Parse<TranslatorMode>(c["mode"]!.GetValue<string>());
                        var localizer = service.Load(c["language"]!.GetValue<string>());
                        item["out"] = new JsonObject
                        {
                            ["code"] = localizer.Code,
                            ["appTitle"] = localizer.Get("app.title"),
                        };
                        break;
                    }
                    case "apply_culture":
                    {
                        // Apply(language, Load(loadLanguage)) in a given mode; the output is the UI culture afterwards.
                        var service = W804aService(configFile, log);
                        service.Mode = Enum.Parse<TranslatorMode>(c["mode"]!.GetValue<string>());
                        var localizer = service.Load(c["load_language"]!.GetValue<string>());
                        service.Apply(c["language"]?.GetValue<string?>(), localizer);
                        item["out"] = new JsonObject
                        {
                            ["requestedLanguage"] = service.RequestedLanguage,
                            ["localizerCode"] = Localizer.Current.Code,
                            ["uiCulture"] = CultureInfo.CurrentUICulture.Name,
                            ["warnings"] = W804aStrings(log.Warnings.Select(w => W804aMask(w, root))),
                        };
                        break;
                    }
                    case "reload_problems":
                    {
                        // Switch to a language, write user files, Reload: the problems and the title afterwards.
                        var service = W804aService(configFile, log);
                        service.Switch(c["language"]!.GetValue<string>());
                        Directory.CreateDirectory(service.UserLanguagesDir);
                        foreach (var file in c["user_files"]!.AsObject())
                        {
                            File.WriteAllText(Path.Combine(service.UserLanguagesDir, file.Key), file.Value!.GetValue<string>());
                        }
                        var problems = service.Reload();
                        item["out"] = new JsonObject
                        {
                            ["problems"] = W804aStrings(problems.Select(p => W804aMask(p, root))),
                            ["appTitle"] = Localizer.Current.Get("app.title"),
                            ["requestedLanguage"] = service.RequestedLanguage,
                            ["logWarnings"] = W804aStrings(log.Warnings.Select(w => W804aMask(w, root))),
                        };
                        break;
                    }
                    case "resolve_code":
                    {
                        var service = W804aService(configFile, log);
                        item["out"] = service.ResolveCode(c["language"]?.GetValue<string?>());
                        break;
                    }
                    case "source_text":
                    {
                        Localizer.SetCurrent(W804aLocalizer(c["localizer"]!.GetValue<string>()));
                        item["out"] = LocalizationSource.Instance[c["key"]!.GetValue<string>()];
                        break;
                    }
                    default:
                        throw new InvalidOperationException("unknown W8-04a localization-service kind " + kind);
                }
            }
            finally
            {
                Directory.Delete(root, recursive: true);
            }
            result.Add((JsonNode?)item);
        }
        return result;
    }
}
