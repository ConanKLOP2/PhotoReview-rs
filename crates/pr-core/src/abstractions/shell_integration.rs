//! Port of `Abstractions/IShellIntegration.cs` (C# baseline d92d6aae): the per-user (HKCU, no administrator rights)
//! Explorer command "Browse with PhotoReview" on folders and on the empty background inside a folder. The registry
//! lives behind [`ShellIntegration`] so Settings can be tested with a fake; [`shell_menu_command`] holds the exact
//! registry values and is pure.
//!
//! Coexistence (docs/adr/R0002): the Rust app must register its OWN verb (see [`shell_menu_command::VERB_KEY_NAME`]),
//! never overwrite the C# app's `PhotoReview` key.

/// Whether the Explorer folder command is registered, and for which executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMenuState {
    /// Neither the folder nor the folder-background command exists.
    NotRegistered,
    /// Both commands exist and launch exactly the executable asked about.
    Registered,
    /// A command exists but launches another executable (the app was moved) or only one of the two exists.
    RegisteredElsewhere,
}

/// The registry side of the integration.
pub trait ShellIntegration: Send + Sync {
    /// Reads the registration and compares it with `executable_path`.
    fn get_state(&self, executable_path: &str) -> ShellMenuState;
    /// Creates or overwrites both commands so they launch `executable_path` (also repairs a moved exe).
    fn register(&self, executable_path: &str) -> std::io::Result<()>;
    /// Removes both commands; nothing else is touched. No-op when not registered.
    fn unregister(&self) -> std::io::Result<()>;
}

/// C# `ShellMenuCommand`: the exact registry values (pure, testable without a registry).
pub mod shell_menu_command {
    use super::ShellMenuState;
    use crate::text::eq_ignore_case;

    /// The menu text Explorer shows. C#: "Browse with PhotoReview".
    pub const MENU_TEXT: &str = "Browse with PhotoReviewRs";

    /// Key name below `Directory\shell` and `Directory\Background\shell`. C#: `PhotoReview`; the Rust app uses its own
    /// key so registering one app never replaces the other's command (R0002).
    pub const VERB_KEY_NAME: &str = "PhotoReviewRs";

    /// Command for a right-click ON a folder: Explorer substitutes the folder for `%1`.
    pub fn for_folder(executable_path: &str) -> String {
        format!("\"{executable_path}\" \"%1\"")
    }

    /// Command for a right-click on the empty background INSIDE a folder: Explorer substitutes that folder for `%V`.
    pub fn for_background(executable_path: &str) -> String {
        format!("\"{executable_path}\" \"%V\"")
    }

    /// Menu icon: the executable's own first icon.
    pub fn icon(executable_path: &str) -> String {
        format!("\"{executable_path}\",0")
    }

    /// A path usable inside the quoted command: rooted (.NET `Path.IsPathRooted`), no quote, NUL, CR or LF, not blank.
    pub fn is_valid_executable_path(executable_path: Option<&str>) -> bool {
        let Some(path) = executable_path else { return false };
        !path.trim().is_empty() && is_path_rooted(path) && !path.contains(['"', '\0', '\r', '\n'])
    }

    /// Classifies what is stored (`None` = missing) against the executable the app runs from. Comparison is
    /// OrdinalIgnoreCase, as in C#.
    pub fn evaluate(
        folder_command: Option<&str>,
        background_command: Option<&str>,
        executable_path: &str,
    ) -> ShellMenuState {
        if folder_command.is_none() && background_command.is_none() {
            return ShellMenuState::NotRegistered;
        }
        let matches = |stored: Option<&str>, expected: String| stored.is_some_and(|s| eq_ignore_case(s, &expected));
        if matches(folder_command, for_folder(executable_path))
            && matches(background_command, for_background(executable_path))
        {
            ShellMenuState::Registered
        } else {
            ShellMenuState::RegisteredElsewhere
        }
    }

    /// .NET `Path.IsPathRooted` on Windows: starts with a directory separator, or with a drive letter and `:`
    /// (so the drive-relative `C:foo` counts as rooted, exactly like .NET).
    fn is_path_rooted(path: &str) -> bool {
        let b = path.as_bytes();
        matches!(b.first(), Some(b'\\' | b'/')) || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const EXE: &str = r"C:\Apps\PhotoReviewRs.exe";

        // C#: ShellMenuCommand.ForFolder / ForBackground / Icon.
        #[test]
        fn commands_quote_the_executable_and_the_placeholder() {
            assert_eq!(for_folder(EXE), r#""C:\Apps\PhotoReviewRs.exe" "%1""#);
            assert_eq!(for_background(EXE), r#""C:\Apps\PhotoReviewRs.exe" "%V""#);
            assert_eq!(icon(EXE), r#""C:\Apps\PhotoReviewRs.exe",0"#);
        }

        // C#: ShellMenuCommand.IsValidExecutablePath.
        #[test]
        fn valid_executable_path_must_be_rooted_without_quote_or_control_chars() {
            for ok in [EXE, r"\\srv\share\a.exe", "/a.exe", "C:a.exe", "c:"] {
                assert!(is_valid_executable_path(Some(ok)), "{ok}");
            }
            for bad in [
                None,
                Some(""),
                Some("   "),
                Some("a.exe"),
                Some(r#"C:\a"b.exe"#),
                Some("C:\\a\0.exe"),
            ] {
                assert!(!is_valid_executable_path(bad), "{bad:?}");
            }
            assert!(!is_valid_executable_path(Some("C:\\a\r.exe")));
            assert!(!is_valid_executable_path(Some("C:\\a\n.exe")));
            assert!(!is_valid_executable_path(Some("1:\\a.exe")), "drive must be a letter");
        }

        // C#: ShellMenuCommand.Evaluate.
        #[test]
        fn evaluate_classifies_missing_matching_and_foreign_registrations() {
            assert_eq!(evaluate(None, None, EXE), ShellMenuState::NotRegistered);
            let (f, b) = (for_folder(EXE), for_background(EXE));
            assert_eq!(evaluate(Some(&f), Some(&b), EXE), ShellMenuState::Registered);
            let (fu, bu) = (f.to_uppercase(), b.to_uppercase());
            assert_eq!(
                evaluate(Some(&fu), Some(&bu), EXE),
                ShellMenuState::Registered,
                "OrdinalIgnoreCase"
            );
            assert_eq!(evaluate(Some(&f), None, EXE), ShellMenuState::RegisteredElsewhere);
            assert_eq!(evaluate(None, Some(&b), EXE), ShellMenuState::RegisteredElsewhere);
            let moved = for_folder(r"D:\Old\PhotoReviewRs.exe");
            assert_eq!(
                evaluate(Some(&moved), Some(&b), EXE),
                ShellMenuState::RegisteredElsewhere
            );
            assert_eq!(
                evaluate(Some(&b), Some(&f), EXE),
                ShellMenuState::RegisteredElsewhere,
                "swapped commands"
            );
        }

        #[test]
        fn rust_app_never_reuses_the_csharp_verb() {
            assert!(!eq_ignore_case(VERB_KEY_NAME, "PhotoReview"));
            assert_ne!(MENU_TEXT, "Browse with PhotoReview");
        }
    }
}
