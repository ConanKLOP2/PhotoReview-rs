//! Port of `PhotoReview.App.Shared/ViewModels/MainViewModel.cs` lines 855 to 1085 (baseline `d92d6aae`): `ShowSettings` and its
//! reload / re-present flows, `CloseSession`, `InstanceLabel`, `UpdateFolderTitle`, `BuildTitleBarContent`, `SetFolderText`,
//! `RefreshLocalizedText` (contract `contracts/W8-08d.md`). Target: `crates/pr-app/src/viewmodels/main/session.rs`
//! (`viewmodels::main::session`).
//!
//! `PersistSession` (line 976) and `PresentedPathOfCurrentEntry` (line 944) live in `state.rs` (hoisted: 08a, 08b and 08e call them).
//! D-W8-2: `ReloadFolderAfterSettingsAsync` waits for the file-action gate with `FileActionGate::when_released` (the waiter posts
//! `MainEvent::GateReleased`), and the re-present / reload tasks (`SettingsRefreshTask`) are covered by `MainViewModel::is_idle`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::abstractions::SettingsTarget;
use pr_core::model::TitleBarFields;
use pr_core::text::dotnet_path::{get_file_name, trim_ending_directory_separator};
use pr_l10n::template::LocArg;

use super::state::{ChangeSet, MainProperty, MainViewModel};
use crate::viewmodels::format::title_bar::{TitleBarLine, format_title_bar};

impl MainViewModel {
    // ---- settings dialog ----

    /// C# `ShowSettings()` (line 858) = `show_settings_at(SettingsTarget::Default)`.
    pub fn show_settings(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ShowSettings(SettingsTarget)` (line 860), private in C#, `pub(super)` here for `open_in_external_editor`.
    /// 1. Snapshot of the STORE's current values (`settings.store_current()`, not the override): loading mode, decoder backend, RAW
    ///    support, RAW pair mode, RAW full decode, sort mode, WebP/HEIC (the `*_or_default()` values for the `Option` settings).
    /// 2. `dialog.show_settings_at(target)`: `false` -> return (nothing changed). The call is modal and synchronous.
    /// 3. `true`: a provisional override left by a failed save is cleared; `update_folder_title(None)`; overlay refresh; `viewer.scaling_quality =
    ///    settings().scaling_quality_or_default()`; `viewer.zoom_step = keyboard_zoom_step_percent / 100.0` (Q-R41); `notify_exif_line_changed()`;
    ///    `main.CapturePairBadgeToolTip` is reported.
    /// 4. New values from the store. `reload_folder = (raw_support changed || (new raw_support && pair mode changed) || sort mode changed
    ///    || webp_heic changed) && session folder is non-empty`. `raw_full_decode_changed`.
    /// 5. When `reload_folder || loading mode changed || backend changed || raw_full_decode_changed`: `preload.cancel()`; when the backend
    ///    changed: `preview.clear_cache()`, the DISK clear on a worker (`preview.clear_disk()`, a failure is logged
    ///    `Preview disk cache clear after backend change failed`, R2-F-16), `preload.clear_preloaded_keys()`; then
    ///    `reload_folder` -> `preview.clear_cache()`, `preload.clear_preloaded_keys()`, `start_settings_reload()` (the open presents the
    ///    folder itself: NO separate present); else when `0 <= current_index < count` -> `presenter.reset_zoom_detail()` then
    ///    `represent_current_after_settings()`.
    pub(super) fn show_settings_at(&mut self, target: SettingsTarget) -> ChangeSet {
        todo!()
    }

    /// C# `RepresentCurrentAfterSettingsAsync` (line 924): `present(current_index, allow_compare: true, path_override:
    /// presented_path_of_current_entry(), include_capture_group_in_compare: compare().is_visible())`; the badge is reported after the
    /// present (the presenter's `CURRENT_PATH` bit). A failure is logged `Re-present after settings change failed`.
    pub(super) fn represent_current_after_settings(&mut self) -> ChangeSet {
        todo!()
    }

    /// Start of C# `ReloadFolderAfterSettingsAsync` (line 957): when the gate is held, `settings_reloads_waiting += 1` and
    /// `file_action_gate.when_released(waiter)`; else `continue_settings_reload` at once ("completes synchronously when no file action
    /// holds the gate").
    pub(super) fn start_settings_reload(&mut self) -> ChangeSet {
        todo!()
    }

    /// The code after the C# `while (IsHeld) await WhenReleasedAsync()` loop: when the gate is held again, wait again (the loop). Else, read AFTER
    /// the wait: `is_closed` or an empty session folder -> stop; `initial = choose_reload_path(presented_path_of_current_entry(), current
    /// entry's capture group, current entry's path, store.raw_support_enabled, store.raw_pair_mode)` (the STORE values) and
    /// `open_folder(session folder, initial)`. A failure is logged `Reloading the folder after a settings change failed`.
    pub(super) fn continue_settings_reload(&mut self) -> ChangeSet {
        todo!()
    }

    // ---- window close ----

    /// C# `CloseSession()` (line 991): `is_closed = true`, `folder_coordinator.dispose()` (an `order_gate_released` update resumes the parked
    /// commands, as C# `TrySetResult`), `session_writer.dispose()` (bounded wait inside the writer, Q-R5 / R7-3). Later opens are
    /// dropped (`open_folder` completes with `None`).
    pub fn close_session(&mut self) -> ChangeSet {
        self.is_closed = true;
        let update = self.folder_coordinator.dispose();
        let mut result = ChangeSet::default();
        // C# `Dispose` completes the order gate: the parked commands resume (O1).
        result.merge(self.resume_deferred(update));
        // The events of the active load are dropped by the disposed coordinator, so its open can never finish by itself.
        if let Some(open) = self.pending_open.take() {
            let epoch = open.epoch;
            result.merge(self.finish_open(open, Some(epoch)));
        }
        self.session_writer.dispose();
        result
    }

    // ---- title ----

    /// C# `InstanceLabel` getter.
    pub fn instance_label(&self) -> Option<&str> {
        self.instance_label.as_deref()
    }

    /// C# `InstanceLabel` setter (line 1009): stores the label and re-renders the title (`update_folder_title(None)`).
    pub fn set_instance_label(&mut self, label: Option<String>) -> ChangeSet {
        self.instance_label = label;
        self.update_folder_title(None)
    }

    /// C# `ApplyInstanceLabel(title)` (line 1020): `"[<label>] <title>"` when the label is non-empty, else the title unchanged.
    pub(super) fn apply_instance_label(&self, title: &str) -> String {
        match self.instance_label.as_deref() {
            Some(label) if !label.is_empty() => format!("[{label}] {title}"),
            _ => title.to_owned(),
        }
    }

    /// C# `UpdateFolderTitle(folder = null)` (line 1023): `folder = session folder, else the empty string, when none is given`; a blank folder -> `app.title`;
    /// else `content = build_title_bar_content(folder, settings().title_bar_fields)` and the title is `main.title.withFolderLogging`
    /// when `settings().logging_enabled`, else `main.title.withFolder` (placeholder `{folder}` = content); then `apply_instance_label`.
    /// `main.FolderTitle` is reported only when the text changed.
    pub(super) fn update_folder_title(&mut self, folder: Option<&str>) -> ChangeSet {
        let folder = folder.map(str::to_owned).unwrap_or_else(|| {
            self.current_session
                .as_ref()
                .map(|session| session.folder.clone())
                .unwrap_or_default()
        });
        let settings = self.settings.current();
        let title = if folder.trim().is_empty() {
            let app_title = self.with_ctx(|ctx| ctx.texts.get("app.title"));
            self.apply_instance_label(&app_title)
        } else {
            let content = self.build_title_bar_content(&folder, settings.title_bar_fields);
            let key = if settings.logging_enabled {
                "main.title.withFolderLogging"
            } else {
                "main.title.withFolder"
            };
            let text = self.with_ctx(|ctx| {
                ctx.texts
                    .format_with(key, &[LocArg::new("folder", content.as_str())], ctx.numbers)
            });
            self.apply_instance_label(&text)
        };
        if self.folder_title == title {
            return ChangeSet::default();
        }
        self.folder_title = title;
        ChangeSet::of(MainProperty::FolderTitle)
    }

    /// C# `BuildTitleBarContent(folder, fields)` (line 1044), from data already in memory (no disk read): `format_title_bar(fields,
    /// TitleBarLine { folder_path, index: current_index if >= 0 and count > 0, count: if > 0, file_name/width/height/exif from
    /// presenter.current_photo_info(), length/last write from the catalog entry (local time through `FormatContextSource::to_local_time`) }, ctx)`.
    /// When the content is empty the folder's last path segment (`get_file_name(trim_ending_directory_separator(folder))`) is used.
    pub(super) fn build_title_bar_content(&self, folder: &str, fields: TitleBarFields) -> String {
        let entry = self.catalog.current();
        let info = self.presenter.current_photo_info();
        let index = self.catalog.current_index();
        let count = i32::try_from(self.catalog.count()).unwrap_or(i32::MAX);
        let modified_local = entry
            .and_then(|entry| entry.last_write_utc)
            .map(|utc| self.format.to_local_time(utc));
        let line = TitleBarLine {
            folder_path: folder,
            index: (index >= 0 && count > 0).then_some(index),
            count: (count > 0).then_some(count),
            file_name: info.map(|info| info.file_name.as_str()),
            length: entry.and_then(|entry| entry.length),
            width: info.map_or(0, |info| info.width),
            height: info.map_or(0, |info| info.height),
            modified_local,
            exif: info.and_then(|info| info.exif.as_ref()),
        };
        let content = self.with_ctx(|ctx| format_title_bar(fields, &line, ctx));
        if content.is_empty() {
            get_file_name(trim_ending_directory_separator(folder)).to_owned()
        } else {
            content
        }
    }

    // ---- folder line and language switch ----

    /// C# `SetFolderText(folder, count, explorerOrderApplied)` (line 1060): stores `folder_text_folder` / `folder_text_count`, sets
    /// `explorer_order_applied` (reported only when it changed) and renders `FolderText` from the state with the plural catalog text
    /// `main.folderText.imageCountExplorer` (Explorer order applied) or `main.folderText.imageCount` (`{folder}`, `{count}`); reported
    /// only when the text changed. Never parsed back.
    pub(super) fn set_folder_text(&mut self, folder: &str, count: i32, explorer_order_applied: bool) -> ChangeSet {
        self.folder_text_folder = Some(folder.to_owned());
        self.folder_text_count = count;
        let mut result = ChangeSet::default();
        if self.explorer_order_applied != explorer_order_applied {
            self.explorer_order_applied = explorer_order_applied;
            result.main.insert(MainProperty::IsExplorerOrderApplied);
        }
        let key = if explorer_order_applied {
            "main.folderText.imageCountExplorer"
        } else {
            "main.folderText.imageCount"
        };
        let args = [LocArg::new("count", i64::from(count)), LocArg::new("folder", folder)];
        let text = self.with_ctx(|ctx| ctx.texts.format_plural_with(key, i64::from(count), &args, ctx.numbers));
        if self.folder_text != text {
            self.folder_text = text;
            result.main.insert(MainProperty::FolderText);
        }
        result
    }

    /// C# `RefreshLocalizedText()` (line 1074): after a live language switch re-render the title and, when a folder is shown, the folder
    /// line from the stored state; report `SkippedWarningText`; overlay refresh; report `CapturePairBadge`, `CapturePairBadgeToolTip`; then
    /// `notify_exif_line_changed()`. The event status text keeps its language until the next update (as C#).
    pub fn refresh_localized_text(&mut self) -> ChangeSet {
        todo!()
    }
}
