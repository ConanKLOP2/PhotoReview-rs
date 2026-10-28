//! Port of `MainViewModel` FEAT-RENAME additions at C# 7eaee2a2.
use super::commands::QueuedKind;
use super::state::{ChangeSet, MainViewModel};
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::time::DateTimeUtc;

impl MainViewModel {
    /// Queue the current photo or capture group for rename; C# `RenameCurrentAsync`.
    pub fn rename_current(&mut self, new_name: &str) -> ChangeSet {
        self.submit_queued(QueuedKind::Rename {
            new_name: new_name.to_owned(),
        })
    }
    /// Re-key session, skipped paths, cache and presenter after disk rename.
    pub fn on_files_renamed(&mut self, renames: &PathKeyMap) -> ChangeSet {
        self.apply_files_renamed(renames).1
    }

    /// Synchronous part of the C# sink callback. The lead's sink replay uses the returned keep decision before replaying
    /// its recorded side effects, because the file-action controller decides whether it must present again.
    pub(super) fn apply_files_renamed(&mut self, renames: &PathKeyMap) -> (bool, ChangeSet) {
        let mut result = ChangeSet::default();
        if !renames.is_empty() {
            if let Some(session) = self.current_session.as_mut() {
                let mut changed = false;
                if let Some(path) = session.current_path.as_deref().and_then(|path| renames.get(path)) {
                    session.current_path = Some(path.clone());
                    changed = true;
                }
                for path in &mut session.skipped {
                    if let Some(renamed) = renames.get(path) {
                        *path = renamed.clone();
                        changed = true;
                    }
                }
                if changed {
                    session.updated_utc = JournalDateTime::utc(DateTimeUtc::now());
                    self.persist_session();
                }
            }
            self.bridge.rekey_last_notified_path(renames);
            for (old_path, _) in renames.iter() {
                if let Err(error) = self.presenter.evict_cached_path(old_path) {
                    self.log
                        .warn(&format!("Could not evict '{old_path}' from the image cache: {error}"));
                }
            }
            result.catalog_changed = true;
        }
        let mut kept = true;
        let mut presenter_changes = crate::coordinators::presenter::state::PresenterChanges::NONE;
        self.format.with_context(&mut |format| {
            (kept, presenter_changes) = self.presenter.retarget_path(renames, format);
        });
        result.presenter = presenter_changes;
        if presenter_changes.contains(crate::coordinators::presenter::state::PresenterChanges::CURRENT_PATH) {
            result.merge(self.notify_navigation_state_changed());
        }
        if !renames.is_empty() {
            // Session and cache changes have no dedicated property; CatalogChanged is the externally visible event.
            result.catalog_changed = true;
        }
        (kept, result)
    }
}
