using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Text.Json.Nodes;

// W8-19a oracle (the F half of the unit): SettingsPageCatalog (ImplementedKeys, PendingKeys, IsComplete, CreatePages) and SettingsPresenter.PageTitle / .Pages, plus the
// PendingSettingsPage placeholder. Nothing here opens a window or touches the OS. NOT compiled or run by the writer (the lead does).
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // Commands\W8-19a
//     "settings-catalog" => Commands.W819aSettingsCatalogCases(inputs),
//
// Wiring: none beyond the line above. OracleCs.csproj already references PhotoReview.dll (Shell.Win32, assembly name "PhotoReview") and PhotoReview.App.Shared.dll; every
// type used here is `internal` and is reached by reflection (as W8-18d), so no InternalsVisibleTo and no change to the C# repo. CurrentCulture = Invariant; the default
// localizer is English (as W8-18b).
//
// Inputs (tests/golden-inputs/settings-catalog.json), "cases": [ { "name", "kind", ... } ], kinds:
//   catalog      -> the static facts of the catalog. Output fields: implemented_keys (array, the C# Dictionary order), pending_keys (array), is_complete (bool),
//                   nav_keys (array: SettingsPresenter.Pages in navigation order), created_keys (array: Key of every page CreatePages() returns, in order),
//                   created_types (array: the C# type name of each page, e.g. "GeneralSettingsPage").
//   page_title   -> input "key"; output "title" (SettingsPresenter.PageTitle(key): "" for an unknown key).
//   pending_page -> input "key"; output "page_key" (PendingSettingsPage.Key), "title" (its Title = PageTitle(key)).
static partial class Commands
{
    private const BindingFlags W819aFlags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static;

    private static Type W819aType(string assembly, string fullName) => Assembly.Load(assembly).GetType(fullName, throwOnError: true)!;

    public static JsonArray W819aSettingsCatalogCases(JsonObject inputs)
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        Type catalog = W819aType("PhotoReview", "PhotoReview.Shell.Win32.Dialogs.SettingsUi.SettingsPageCatalog");
        Type pending = W819aType("PhotoReview", "PhotoReview.Shell.Win32.Dialogs.SettingsUi.PendingSettingsPage");
        Type presenter = W819aType("PhotoReview.App.Shared", "PhotoReview.App.Presenters.SettingsPresenter");
        var outCases = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            switch (c["kind"]!.GetValue<string>())
            {
                case "catalog":
                {
                    c["implemented_keys"] = W819aStrings((IEnumerable)catalog.GetProperty("ImplementedKeys", W819aFlags)!.GetValue(null)!);
                    c["pending_keys"] = W819aStrings((IEnumerable)catalog.GetProperty("PendingKeys", W819aFlags)!.GetValue(null)!);
                    c["is_complete"] = (bool)catalog.GetProperty("IsComplete", W819aFlags)!.GetValue(null)!;
                    var nav = new JsonArray();
                    foreach (var info in (IEnumerable)presenter.GetProperty("Pages", W819aFlags)!.GetValue(null)!)
                    {
                        nav.Add((string)info.GetType().GetProperty("Key", W819aFlags)!.GetValue(info)!);
                    }

                    c["nav_keys"] = nav;
                    var keys = new JsonArray();
                    var types = new JsonArray();
                    foreach (var page in (IEnumerable)catalog.GetMethod("CreatePages", W819aFlags)!.Invoke(null, null)!)
                    {
                        keys.Add((string)page.GetType().GetProperty("Key", W819aFlags)!.GetValue(page)!);
                        types.Add(page.GetType().Name);
                    }

                    c["created_keys"] = keys;
                    c["created_types"] = types;
                    break;
                }

                case "page_title":
                    c["title"] = (string)presenter.GetMethod("PageTitle", W819aFlags)!.Invoke(null, [c["key"]!.GetValue<string>()])!;
                    break;
                case "pending_page":
                {
                    object page = pending.GetConstructors(W819aFlags)[0].Invoke([c["key"]!.GetValue<string>()]);
                    c["page_key"] = (string)pending.GetProperty("Key", W819aFlags)!.GetValue(page)!;
                    c["title"] = (string)pending.GetProperty("Title", W819aFlags)!.GetValue(page)!;
                    break;
                }

                default:
                    throw new InvalidOperationException("unknown settings-catalog kind");
            }

            outCases.Add(c.DeepClone());
        }

        return outCases;
    }

    private static JsonArray W819aStrings(IEnumerable values)
    {
        var array = new JsonArray();
        foreach (var value in values)
        {
            array.Add((string)value);
        }

        return array;
    }
}
