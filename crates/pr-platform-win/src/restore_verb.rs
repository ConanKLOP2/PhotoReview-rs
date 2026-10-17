//! Port of `PhotoReview.Platform.Windows/WindowsRecycleBin.cs`: localized Shell restore-verb selection.

use pr_core::text::eq_ignore_case;

pub const CANONICAL_VERB: &str = "undelete";

const NAME_FRAGMENTS: &[&str] = &[
    "restore",
    "restaur",
    "ripristin",
    "wiederher",
    "herstel",
    "gendan",
    "återställ",
    "palauta",
    "przywr",
    "obnovi",
    "vissza",
    "geri yükle",
    "khôi",
    "восстанов",
    "відновит",
    "επαναφορ",
    "還原",
    "还原",
    "元に戻す",
    "복원",
    "استعادة",
    "שחזר",
];

/// Removes Shell accelerator markers (`&R`, `(&E)`) without changing other text.
pub fn normalize(verb_name: Option<&str>) -> String {
    let Some(name) = verb_name.filter(|name| !name.trim().is_empty()) else {
        return String::new();
    };
    name.replace("(&", "(").replace('&', "").trim().to_owned()
}

/// C# `RestoreVerb.IsRestoreName`.
pub fn is_restore_name(verb_name: Option<&str>) -> bool {
    let normalized = normalize(verb_name);
    !normalized.is_empty()
        && NAME_FRAGMENTS
            .iter()
            .any(|fragment| contains_ignore_case(&normalized, fragment))
}

/// Index of the first recognized restore label, or `None` when canonical fallback is needed.
pub fn find_index(verb_names: &[String]) -> Option<usize> {
    verb_names.iter().position(|name| is_restore_name(Some(name)))
}

/// Calls exactly one route. Errors from a recognized route propagate without canonical retry.
pub fn invoke<E>(
    verb_names: &[String],
    mut invoke_by_index: impl FnMut(usize) -> Result<(), E>,
    mut invoke_canonical: impl FnMut(&str) -> Result<(), E>,
) -> Result<String, E> {
    if let Some(index) = find_index(verb_names) {
        invoke_by_index(index)?;
        Ok(format!("verb:{index}"))
    } else {
        invoke_canonical(CANONICAL_VERB)?;
        Ok(CANONICAL_VERB.to_owned())
    }
}

fn contains_ignore_case(text: &str, fragment: &str) -> bool {
    let fragment_len = fragment.chars().count();
    if fragment_len == 0 {
        return true;
    }
    let chars: Vec<char> = text.chars().collect();
    chars.windows(fragment_len).any(|window| {
        let candidate: String = window.iter().collect();
        eq_ignore_case(&candidate, fragment)
    })
}
