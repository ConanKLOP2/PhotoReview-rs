using System.Globalization;
using System.Text.Json.Nodes;

// G3 L6 / G15 L6 (docs/reviews/BACKLOG.md): `percent-one-decimal`. The C# expression is the one of DiagnosticsPresenter.Build
// (src/PhotoReview.App.Shared/Presenters/SecondaryDialogPresenters.cs:56): `$"{(double)hits / total:P1}"`, i.e. double.ToString("P1")
// under CultureInfo.CurrentCulture. Dispatch in Program.cs: "percent-one-decimal" => Commands.PercentOneDecimal(inputs).
// Input: tests/golden-inputs/percent-one-decimal.json { "cultures": [...], "ratios": [...] }.
static partial class Commands
{
    public static JsonArray PercentOneDecimal(JsonObject inputs)
    {
        var result = new JsonArray();
        var previous = CultureInfo.CurrentCulture;
        try
        {
            foreach (var cultureNode in inputs["cultures"]!.AsArray())
            {
                var name = cultureNode!.GetValue<string>();
                var culture = CultureInfo.GetCultureInfo(name);
                CultureInfo.CurrentCulture = culture;
                var nf = culture.NumberFormat;
                foreach (var ratioNode in inputs["ratios"]!.AsArray())
                {
                    var ratio = ratioNode!.GetValue<double>();
                    result.Add(new JsonObject
                    {
                        ["culture"] = name,
                        ["iso_language"] = culture.TwoLetterISOLanguageName,
                        ["ratio"] = ratio,
                        ["text"] = $"{ratio:P1}",
                        ["percent_symbol"] = nf.PercentSymbol,
                        ["percent_positive_pattern"] = nf.PercentPositivePattern,
                        ["percent_negative_pattern"] = nf.PercentNegativePattern,
                        ["percent_decimal_separator"] = nf.PercentDecimalSeparator,
                        ["percent_group_separator"] = nf.PercentGroupSeparator,
                    });
                }
            }
        }
        finally { CultureInfo.CurrentCulture = previous; }
        return result;
    }
}
