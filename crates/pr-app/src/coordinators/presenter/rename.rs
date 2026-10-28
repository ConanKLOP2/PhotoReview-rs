//! Port of `ImagePresenter.RetargetPath` at C# 7eaee2a2, lines 225-295.
use super::state::{ImagePresenter, PresenterChanges};
use crate::viewmodels::format::context::FormatContext;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::text::{dotnet_path::get_file_name, ordinal_upper};

/// C# `EndsWith(oldName, OrdinalIgnoreCase)` returns a UTF-16 suffix boundary. Find the equivalent Rust UTF-8
/// boundary by comparing characters from the end, and return only a boundary from the original status string.
fn ordinal_ignore_case_suffix_start(value: &str, suffix: &str) -> Option<usize> {
    let mut value_chars = value.char_indices().rev();
    let mut value_boundary = value.len();
    for suffix_char in suffix.chars().rev() {
        let (start, value_char) = value_chars.next()?;
        if ordinal_upper(value_char) != ordinal_upper(suffix_char) {
            return None;
        }
        value_boundary = start;
    }
    Some(value_boundary)
}

impl ImagePresenter {
    /// C# `IsPresentInFlight`: a present has started and not yet finished (a superseded one counts until it notices).
    /// `retarget_path` asks the caller to present again when this is true.
    pub fn is_present_in_flight(&self) -> bool {
        self.navigation.is_some()
            || self.missing.as_ref().is_some_and(|run| run.awaited_present)
            || !self.retired_waits.is_empty()
    }
    /// C# `RetargetPath(renames)`: re-key the presented and shown-file paths, photo info, status line name and the zoom-detail
    /// target (via `ZoomDetailLoader::retarget_path`, effects applied) without presenting, decoding or touching zoom/pan.
    /// Returns `(kept, changes)`: `kept` false when Compare was visible (cleared) or a present was in flight, so the caller
    /// must present the current entry again. An empty map only re-arms the zoom detail.
    pub fn retarget_path(&mut self, renames: &PathKeyMap, format: &FormatContext<'_>) -> (bool, PresenterChanges) {
        let mut changes = PresenterChanges::NONE;
        if let Some(path) = self.presented_file_path.as_deref().and_then(|path| renames.get(path)) {
            self.presented_file_path = Some(path.clone());
        }

        let compare_visible = self.compare.is_visible();
        if compare_visible {
            self.compare.clear(format);
            changes |= PresenterChanges::COMPARE;
        }
        let mut shown = self.current_navigation_path.clone();
        if compare_visible || self.is_present_in_flight() {
            return (false, changes);
        }
        let Some(current) = shown.clone() else {
            return (true, changes);
        };
        if let Some(renamed) = renames.get(&current) {
            let old_name = get_file_name(&current);
            let new_name = get_file_name(renamed);
            self.current_navigation_path = Some(renamed.clone());
            shown = Some(renamed.clone());
            changes |= PresenterChanges::CURRENT_PATH;
            if let Some(info) = self.current_photo_info.as_mut() {
                info.file_name = new_name.to_owned();
                changes |= PresenterChanges::PHOTO_INFO;
            }
            if let Some(info) = self.info_while_original_shown.as_mut() {
                info.file_name = new_name.to_owned();
            }
            if let Some(start) = ordinal_ignore_case_suffix_start(&self.status_text, old_name) {
                self.status_text.truncate(start);
                self.status_text.push_str(new_name);
                changes |= PresenterChanges::STATUS;
            }
        }
        if let Some(shown) = shown {
            let effects = self.zoom_detail.retarget_path(self.clock.current_navigation(), &shown);
            self.apply_zoom_effects(effects);
        }
        changes |= self.take_changes();
        (true, changes)
    }
}

#[cfg(test)]
mod tests {
    use super::ordinal_ignore_case_suffix_start;

    #[test]
    fn ordinal_ignore_case_suffix_uses_exact_case_suffix_boundary_and_preserves_prefix() {
        let status = "index Ⱥ.jpg";
        let start = ordinal_ignore_case_suffix_start(status, "Ⱥ.jpg").unwrap_or_default();
        assert_eq!(&status[..start], "index ");
    }

    #[test]
    fn ordinal_ignore_case_suffix_matches_non_ascii_case_with_different_utf8_lengths() {
        // The oracle casing table pairs U+2C65 (3 UTF-8 bytes) with U+023A (2 UTF-8 bytes).
        let status = "prefix ⱥ.jpg";
        let start = ordinal_ignore_case_suffix_start(status, "Ⱥ.jpg").unwrap_or_default();
        assert_eq!(&status[..start], "prefix ");
    }

    #[test]
    fn ordinal_ignore_case_suffix_rejects_a_mismatched_suffix() {
        assert_eq!(ordinal_ignore_case_suffix_start("prefix name.jpg", "name.png"), None);
    }

    #[test]
    fn ordinal_ignore_case_suffix_preserves_the_whole_prefix() {
        let status = "123 / folder / IMG.JPG";
        let start = ordinal_ignore_case_suffix_start(status, "img.jpg").unwrap_or_default();
        assert_eq!(&status[..start], "123 / folder / ");
    }

    #[test]
    fn ordinal_ignore_case_suffix_accepts_an_empty_suffix_at_the_end() {
        let status = "prefix";
        assert_eq!(ordinal_ignore_case_suffix_start(status, ""), Some(status.len()));
    }
}
