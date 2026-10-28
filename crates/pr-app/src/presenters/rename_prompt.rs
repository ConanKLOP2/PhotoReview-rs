//! Port of `PhotoReview.App.Shared/Presenters/RenamePromptPresenter.cs` at 7eaee2a2.
use pr_core::abstractions::FileSystem;
use pr_core::abstractions::{RenamePromptRequest, RenameValidateFn};
use pr_core::file_actions::rename::{RenameNameCheck, RenameNameProblem};
use pr_core::text::dotnet_path;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;
use std::sync::Arc;

/// Pure policy callback chosen for a single file or a capture group.
pub type RenameCheck = Arc<dyn Fn(&str) -> RenameNameCheck>;
/// Whether another file owns the complete new name; commit may inspect disk.
pub type NameTaken = Arc<dyn Fn(&str, bool) -> bool>;

/// Invalid source path supplied when preparing the rename prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenamePromptSourceError;

/// Owned prompt plan passed to the shell after the view-model borrow has ended.
pub struct PreparedRenamePrompt {
    source: String,
    check: RenameCheck,
}

impl std::fmt::Debug for PreparedRenamePrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedRenamePrompt")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl PreparedRenamePrompt {
    /// Build a plan from an owned source path and policy callback.
    pub fn new(source: String, check: RenameCheck) -> Self {
        Self { source, check }
    }
    /// The selected path this prompt was prepared for.
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Create the modal request with a caller supplied live catalog/disk query.
    pub fn request(
        &self,
        listed: Arc<dyn Fn(&str) -> bool>,
        file_system: Arc<dyn FileSystem>,
    ) -> Result<RenamePromptRequest, RenamePromptSourceError> {
        let folder = dotnet_path::get_directory_name(&self.source).unwrap_or_default();
        // This request is consumed synchronously on the UI thread; the dialog abstraction uses Arc for its callback seam.
        #[allow(clippy::arc_with_non_send_sync)]
        let taken: NameTaken = Arc::new(move |file_name, is_commit| {
            let candidate = std::path::Path::new(&folder).join(file_name);
            let full_path = candidate.to_string_lossy();
            listed(&full_path) || (is_commit && file_system.file_exists(candidate.as_path()))
        });
        Ok(RenamePromptPresenter::new(self.source.clone(), Arc::clone(&self.check), Some(taken))?.to_request())
    }
}

/// Pure validation state of the rename prompt; the dialog only renders this state.
pub struct RenamePromptPresenter {
    /// Current source path (including original extension).
    pub source: String,
    /// The check chosen by the caller.
    pub check: RenameCheck,
    /// Optional catalog/disk collision question.
    pub name_taken: Option<NameTaken>,
    initial_text: String,
    extension: String,
}

impl std::fmt::Debug for RenamePromptPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenamePromptPresenter")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl RenamePromptPresenter {
    /// Construct the prompt for the currently selected path.
    pub fn new(
        source: String,
        check: RenameCheck,
        name_taken: Option<NameTaken>,
    ) -> Result<Self, RenamePromptSourceError> {
        if source.trim().is_empty() {
            return Err(RenamePromptSourceError);
        }
        let file_name = dotnet_path::get_file_name(&source);
        let initial_text = dotnet_path::get_file_name_without_extension(&source).to_owned();
        let extension = file_name.get(initial_text.len()..).unwrap_or_default().to_owned();
        Ok(Self {
            source,
            check,
            name_taken,
            initial_text,
            extension,
        })
    }
    /// The original file name without its final extension.
    pub fn initial_text(&self) -> &str {
        &self.initial_text
    }
    /// The original final extension, including its dot and preserving case.
    pub fn extension(&self) -> &str {
        &self.extension
    }
    /// C# `ToRequest`.
    pub fn to_request(&self) -> RenamePromptRequest {
        let check = Arc::clone(&self.check);
        let taken = self.name_taken.as_ref().map(Arc::clone);
        // The request API is Arc-based for parity with the dialog seam; it is consumed synchronously on the UI thread.
        #[allow(clippy::arc_with_non_send_sync)]
        let validate: RenameValidateFn = Arc::new(move |typed: &str, is_commit: bool| {
            let result = check(typed);
            let message = problem_text(result.problem);
            if message.is_some() || result.is_unchanged || result.differs_only_by_case {
                return message;
            }
            if taken
                .as_ref()
                .is_some_and(|question| question(&result.new_file_name, is_commit))
            {
                Some(Localizer::current().format(
                    "dialog.rename.error.exists",
                    &[LocArg::new("name", result.new_file_name)],
                ))
            } else {
                None
            }
        });
        RenamePromptRequest {
            initial_name: self.initial_text.clone(),
            extension: self.extension.clone(),
            validate,
        }
    }
    /// C# `TryAccept`: localized refusal text, or None when valid.
    pub fn try_accept(&self, typed_stem: Option<&str>, is_commit: bool) -> Option<String> {
        let result = (self.check)(typed_stem.unwrap_or_default());
        let message = problem_text(result.problem);
        if message.is_some() || result.is_unchanged || result.differs_only_by_case {
            return message;
        }
        self.name_taken.as_ref().and_then(|question| {
            question(&result.new_file_name, is_commit).then(|| {
                Localizer::current().format(
                    "dialog.rename.error.exists",
                    &[LocArg::new("name", result.new_file_name)],
                )
            })
        })
    }
}

fn problem_text(problem: RenameNameProblem) -> Option<String> {
    let key = match problem {
        RenameNameProblem::None => return None,
        RenameNameProblem::Empty => "dialog.rename.error.empty",
        RenameNameProblem::InvalidCharacter => "dialog.rename.error.invalidCharacter",
        RenameNameProblem::TrailingDotOrSpace => "dialog.rename.error.trailingDot",
        RenameNameProblem::ReservedDeviceName => "dialog.rename.error.reservedName",
        RenameNameProblem::NameTooLong => "dialog.rename.error.tooLong",
        RenameNameProblem::PathTooLong => "dialog.rename.error.pathTooLong",
        RenameNameProblem::SidecarNotMappable => "dialog.rename.error.sidecarNotMappable",
    };
    Some(Localizer::current().get(key))
}
