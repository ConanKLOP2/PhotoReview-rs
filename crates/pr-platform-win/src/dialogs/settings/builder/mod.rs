//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsPageBuilder.cs` (645 lines, baseline `d92d6aae`), split at class boundaries (D-W8B5-702):
//! - [`rows`] (W8-19b, C# lines 1-74 and 124-201 and 203-240): `SettingsRow`, `SettingsSliderRow`, `ISettingsLayoutSurface`, `SettingsLayoutElement` (the base class)
//!   and the elements `SettingsRowElement`, `SettingsSliderElement`, `SettingsTextElement`, `SettingsButtonsElement`;
//! - [`groups`] (W8-19b, C# lines 76-122 and 242-307): `SettingsGroupElement`, `SettingsCheckElement`, `SettingsRadioElement`, `SettingsCheckFlowElement`;
//! - [`layout`] (W8-19c, C# lines 309-365): `SettingsKeyCell`, `SettingsKeyCellSpec`, `SettingsKeyGridElement`;
//! - [`inputs`] (W8-19c, C# lines 367-645): `SettingsPageBuilder` itself (the declaration API of a page).
//!
//! WP-35 GD3: a page DECLARES its controls with the builder in reading order (= Tab order) and gets their ids back; the frame creates the dialog template from
//! `items()`, measures and places the controls with the layout elements in dialog units, so long Vietnamese strings wrap instead of being cut.
//!
//! Lead-owned index (this file is final: it declares the four submodules only).

pub mod groups;
pub mod inputs;
pub mod layout;
pub mod rows;
