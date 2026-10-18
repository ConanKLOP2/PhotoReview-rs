//! Port of `PhotoReview.App.Shared/Localization/TranslationProblems.cs` (C# baseline d92d6aae, 28 lines): the text of
//! the message shown after "Reload translations". The problem lines stay in English (file, key, reason), the rest
//! of the message is translated. Unit W8-04a.

#![allow(unused_variables, dead_code)] // D11: todo!() bodies; the porter removes this line with the last todo!()

use pr_l10n::localizer::Localizer;

/// C# `TranslationProblems.MaxListed`: problems listed in the message; the rest are counted (all are in the log).
pub const MAX_LISTED: usize = 15;

/// C# `TranslationProblems.HasProblems(warnings)`: `true` when at least one warning exists.
pub fn has_problems(warnings: &[String]) -> bool {
    todo!()
}

/// C# `TranslationProblems.Message(warnings)`, with the translated texts taken from `text` (the C# `Tr.*` accessors
/// read the ambient localizer; the Rust port passes it explicitly, so a golden can pin each language).
///
/// - no warnings: `dialog.reloadTranslations.ok`;
/// - otherwise the first `MAX_LISTED` as `"• " + warning`, joined by `"\n"`; when there are more, `"\n"` and
///   `dialog.reloadTranslations.more` with `{count}` = the number not listed;
/// - the result is `dialog.reloadTranslations.problems` (plural by the total count: `.one` / `.other`) with `{count}`
///   = total count and `{details}` = the list text. `{count}` must be passed as an argument as well as the plural
///   selector (`Localizer::format_plural` does not inject it).
pub fn message(warnings: &[String], text: &Localizer) -> String {
    todo!()
}
