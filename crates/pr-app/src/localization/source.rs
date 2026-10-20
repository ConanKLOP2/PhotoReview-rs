//! Port of `PhotoReview.App.Shared/Localization/LocalizationSource.cs` (C# baseline d92d6aae, 24 lines): the binding
//! source of WPF `TrExtension`. Only the lookup survives: the `INotifyPropertyChanged` / `Item[]` change is a WPF
//! binding mechanism and is NOT ported (D-W8-1: the shell repaints from a `ChangeSet`, and subscribes to language
//! switches with `pr_l10n::localizer::on_current_changed`). Unit W8-04a. The singleton `Instance` becomes a free function.

use pr_l10n::localizer::Localizer;

/// C# `LocalizationSource.this[key]`: `Localizer.Current.Get(key)`, the text of `key` in the current language, or
/// `key` itself when the catalog has no such key (the `Localizer::get` rule). Reads the ambient
/// `pr_l10n::localizer::Localizer::current()`.
pub fn text(key: &str) -> String {
    Localizer::current().get(key)
}
