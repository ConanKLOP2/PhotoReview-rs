//! Port of `PhotoReview.App.Shared/Localization/LanguageOptions.cs` (C# baseline d92d6aae, 53 lines): the Settings
//! language picker maps setting values (`auto` or a language code) to entries and back. I18N L08. Unit W8-04a.
//! Pure: the labels come from the `Localizer` passed in (C# default `Localizer.Current` is the App's choice).

use pr_core::text::eq_ignore_case;
use pr_l10n::loader::{AUTO_CODE, LanguageInfo};
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

/// One picker entry: the value saved to `AppSettings.UiLanguage` and its label.
/// C# `sealed record LanguageOption(string Code, string DisplayName)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageOption {
    /// `auto` (follow Windows) or a language code, exactly as `LanguageLoader::AUTO_CODE` / `LanguageInfo::code`.
    pub code: String,
    /// The label shown in the picker.
    pub display_name: String,
}

impl LanguageOption {
    /// C# `LanguageOption.IsAuto`: `Code` equals `auto` with `OrdinalIgnoreCase` (`pr_core::text::eq_ignore_case`).
    pub fn is_auto(&self) -> bool {
        eq_ignore_case(&self.code, AUTO_CODE)
    }
}

/// C# `LanguageOptions.Build(languages, localizer)`: `auto` first with the label `settings.language.auto`, then one
/// entry per language in the given order. Native name is used, or the code when the native name is blank
/// (`trim().is_empty()`). A user-provided language gets `settings.language.userProvided` with `{name}` = that name.
/// The C# `localizer ?? Localizer.Current` default is the caller's job (the App passes `&Localizer::current()`).
pub fn build(languages: &[LanguageInfo], text: &Localizer) -> Vec<LanguageOption> {
    let mut options = Vec::with_capacity(languages.len() + 1);
    options.push(LanguageOption {
        code: AUTO_CODE.to_owned(),
        display_name: text.get("settings.language.auto"),
    });
    for language in languages {
        let label = if language.native_name.trim().is_empty() {
            language.code.clone()
        } else {
            language.native_name.clone()
        };
        let display_name = if language.is_user_provided {
            text.format("settings.language.userProvided", &[LocArg::new("name", label)])
        } else {
            label
        };
        options.push(LanguageOption {
            code: language.code.clone(),
            display_name,
        });
    }
    options
}

/// C# `LanguageOptions.IndexOf(options, uiLanguage)`: a blank or `None` value means `auto`; the value is trimmed,
/// then matched against `code` with `OrdinalIgnoreCase`. `None` (C# `-1`) when nothing matches.
pub fn index_of(options: &[LanguageOption], ui_language: Option<&str>) -> Option<usize> {
    let wanted = match ui_language {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => AUTO_CODE,
    };
    options.iter().position(|option| eq_ignore_case(&option.code, wanted))
}

/// C# `LanguageOptions.ToSetting(selected, current)`: a selected entry gives `auto` when it is the auto entry,
/// otherwise its code (untrimmed, as stored). With no selection the `current` value is returned UNCHANGED, except a
/// `None` or blank `current` gives `auto`.
pub fn to_setting(selected: Option<&LanguageOption>, current: Option<&str>) -> String {
    match selected {
        Some(option) if option.is_auto() => AUTO_CODE.to_owned(),
        Some(option) => option.code.clone(),
        None => match current {
            Some(value) if !value.trim().is_empty() => value.to_owned(),
            _ => AUTO_CODE.to_owned(),
        },
    }
}
