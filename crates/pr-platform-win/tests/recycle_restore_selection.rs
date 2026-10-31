#[path = "../src/recycle_candidate.rs"]
mod recycle_candidate;

use std::path::Path;

use pr_core::time::DateTimeUtc;
use pr_platform_win::restore_verb::{CANONICAL_VERB, CANONICAL_VERB_Z, find_index, invoke, is_restore_name};
use recycle_candidate::{RecycleCandidate, RecycleItemFailureKind, is_match, is_per_item_failure};

fn timestamp(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text).expect("test timestamp is valid").0
}

fn candidate(name: Option<&str>, folder: Option<&str>, size: i64, stamp: DateTimeUtc) -> RecycleCandidate {
    RecycleCandidate::new(folder.map(str::to_owned), name.map(str::to_owned), size, stamp, None)
}

#[test]
fn selector_requires_original_path_size_and_timestamp() {
    // C#: WindowsRecycleBinCandidateTests.SelectorRequiresOriginalPathSizeAndTimestamp.
    let path = Path::new(r"C:\photos\same.jpg");
    let stamp = timestamp("2026-09-20T01:02:03Z");
    let item = candidate(Some("same.jpg"), Some(r"C:\photos"), 42, stamp);

    assert!(is_match(&item, path, 42, stamp));
    assert!(!is_match(&item, path, 41, stamp));
    assert!(!is_match(&item, path, 42, stamp.checked_add_ticks(10_000_000).unwrap()));
    assert!(!is_match(&item, Path::new(r"C:\other\same.jpg"), 42, stamp));
}

#[test]
fn selector_tolerates_shell_whole_second_timestamps() {
    // C#: WindowsRecycleBinCandidateTests.SelectorToleratesShellWholeSecondTimestamps.
    let path = Path::new(r"C:\photos\same.jpg");
    let written = timestamp("2026-09-24T14:58:16.6190398Z");
    let shell = timestamp("2026-09-24T14:58:16Z");
    let item = candidate(Some("same.jpg"), Some(r"C:\photos"), 4, shell);

    assert!(is_match(&item, path, 4, written));
    assert!(!is_match(
        &item,
        path,
        4,
        written.checked_add_ticks(20_000_000).unwrap()
    ));
}

#[test]
fn selector_accepts_alternate_timestamp_reading() {
    // C#: WindowsRecycleBinCandidateTests.SelectorAcceptsAlternateTimestampReading.
    let path = Path::new(r"C:\photos\same.jpg");
    let written = timestamp("2026-09-24T14:58:16Z");
    let wrong_primary = written.checked_add_ticks(7 * 60 * 60 * 10_000_000).unwrap();
    let mut item = candidate(Some("same.jpg"), Some(r"C:\photos"), 4, wrong_primary);
    item.alternate_last_write_utc = Some(written);

    assert!(is_match(&item, path, 4, written));
    item.alternate_last_write_utc = None;
    assert!(!is_match(&item, path, 4, written));
}

#[test]
fn selector_does_not_accept_ambiguous_candidate_by_metadata() {
    // C#: WindowsRecycleBinCandidateTests.SelectorDoesNotAcceptAmbiguousCandidateByMetadata.
    let path = Path::new(r"C:\photos\same.jpg");
    let stamp = timestamp("2026-09-20T01:02:03Z");
    let first = candidate(Some("same.jpg"), Some(r"C:\photos"), 42, stamp);
    let second = candidate(Some("same"), Some(r"C:\photos"), 42, stamp);

    assert!(is_match(&first, path, 42, stamp));
    assert!(is_match(&second, path, 42, stamp));
    assert_eq!(
        [&first, &second]
            .into_iter()
            .filter(|c| is_match(c, path, 42, stamp))
            .count(),
        2
    );
}

#[test]
fn selector_matches_display_name_without_extension() {
    // C#: WindowsRecycleBinCandidateTests.SelectorMatchesDisplayNameWithoutExtension.
    let path = Path::new(r"C:\photos\IMG_0001.JPG");
    let stamp = timestamp("2026-09-25T01:02:03Z");
    let item = candidate(Some("IMG_0001"), Some(r"C:\photos"), 42, stamp);

    assert!(is_match(&item, path, 42, stamp));
    let recased_folder = candidate(Some("IMG_0001"), Some(r"c:\PHOTOS\"), 42, stamp);
    assert!(is_match(&recased_folder, path, 42, stamp));
    assert!(!is_match(
        &candidate(Some("IMG_0001"), Some(r"C:\other"), 42, stamp),
        path,
        42,
        stamp
    ));
    assert!(!is_match(&item, path, 41, stamp));
    assert!(!is_match(&item, path, 42, stamp.checked_add_ticks(50_000_000).unwrap()));
    assert!(!is_match(
        &candidate(Some("IMG_0002"), Some(r"C:\photos"), 42, stamp),
        path,
        42,
        stamp
    ));
    assert!(!is_match(
        &candidate(Some("IMG_000"), Some(r"C:\photos"), 42, stamp),
        path,
        42,
        stamp
    ));
}

#[test]
fn selector_matches_multi_dot_name_without_last_extension() {
    // C#: WindowsRecycleBinCandidateTests.SelectorMatchesMultiDotNameWithoutLastExtension.
    let path = Path::new(r"C:\photos\a.b.jpg");
    let stamp = timestamp("2026-09-25T01:02:03Z");

    assert!(is_match(
        &candidate(Some("a.b"), Some(r"C:\photos"), 1, stamp),
        path,
        1,
        stamp
    ));
    assert!(!is_match(
        &candidate(Some("a"), Some(r"C:\photos"), 1, stamp),
        path,
        1,
        stamp
    ));
}

#[test]
fn selector_requires_folder_plus_name() {
    // C#: WindowsRecycleBinCandidateTests.SelectorRequiresFolderPlusName.
    let stamp = timestamp("2026-09-20T01:02:03Z");
    assert!(!is_match(
        &candidate(None, Some(r"C:\photos\same.jpg"), 42, stamp),
        Path::new(r"C:\photos\same.jpg"),
        42,
        stamp
    ));
    assert!(!is_match(
        &candidate(Some("same.jpg"), None, 42, stamp),
        Path::new(r"C:\photos\same.jpg"),
        42,
        stamp
    ));
    assert!(is_match(
        &candidate(Some("same"), Some(r"C:\"), 42, stamp),
        Path::new(r"C:\same.jpg"),
        42,
        stamp
    ));
}

#[test]
fn per_item_failure_includes_com_cast_and_format_but_not_unrelated_errors() {
    // C#: WindowsRecycleBinCandidateTests.PerItemFailure_IncludesComCastAndFormatButNotUnrelatedErrors.
    assert!(is_per_item_failure(RecycleItemFailureKind::Com));
    assert!(is_per_item_failure(RecycleItemFailureKind::InvalidCast));
    assert!(is_per_item_failure(RecycleItemFailureKind::Format));
    assert!(!is_per_item_failure(RecycleItemFailureKind::Other));
}

#[test]
fn known_restore_labels_in_many_languages_are_recognized() {
    // C#: WindowsRecycleBinRestoreVerbTests.KnownRestoreLabelsInManyLanguagesAreRecognized.
    for label in [
        "Restore",
        "&Restore",
        "Restore the selected items",
        "&Wiederherstellen",
        "Wiederherstellen",
        "&Khôi phục",
        "Khôi phục",
        "&Restaurer",
        "&Restaurar",
        "&Ripristina",
        "He&rstellen",
        "&Gendan",
        "&Återställ",
        "&Palauta",
        "Przywró&ć",
        "Geri &yükle",
        "&Восстановить",
        "Відновит&и",
        "还原(&E)",
        "還原(&E)",
        "元に戻す(&E)",
        "복원(&E)",
        "  &restore  ",
    ] {
        assert!(is_restore_name(Some(label)), "{label}");
    }
}

#[test]
fn other_recycle_bin_verbs_are_never_taken_for_restore() {
    // C#: WindowsRecycleBinRestoreVerbTests.OtherRecycleBinVerbsAreNeverTakenForRestore.
    for label in [
        "Cut",
        "Cu&t",
        "&Delete",
        "P&roperties",
        "Open",
        "",
        "Cắt",
        "Xóa",
        "Thuộc tính",
        "Ausschneiden",
        "&Löschen",
        "Eigenschaften",
        "剪切(&T)",
        "删除(&D)",
        "属性(&R)",
        "切り取り(&T)",
        "プロパティ(&R)",
        "Вырезать",
        "Удалить",
        "Свойства",
        "&Supprimer",
        "&Couper",
        "Propriétés",
        "&Eliminar",
        "Cor&tar",
        "&Verwijderen",
        "&Knippen",
        "&Slet",
        "&Ta bort",
        "&Poista",
        "&Leikkaa",
        "&Usuń",
        "&Odstranit",
        "&Törlés",
        "&Kivágás",
        "&Sil",
        "&Kes",
        "Видалити",
        "Вирізати",
        "Διαγραφή",
        "Αποκοπή",
        "Ιδιότητες",
        "삭제(&D)",
        "잘라내기(&T)",
        "속성(&R)",
        "حذف",
        "قص",
        "خصائص",
        "מחק",
        "גזור",
        "מאפיינים",
    ] {
        assert!(!is_restore_name(Some(label)), "{label}");
    }
}

#[test]
fn null_or_blank_name_is_not_restore() {
    // C#: WindowsRecycleBinRestoreVerbTests.NullOrBlankNameIsNotRestore.
    assert!(!is_restore_name(None));
    assert!(!is_restore_name(Some("   ")));
}

#[test]
fn find_index_returns_the_first_restore_verb() {
    // C#: WindowsRecycleBinRestoreVerbTests.FindIndexReturnsTheFirstRestoreVerb.
    for (names, expected) in [
        (vec!["Restore", "Cut", "Delete", "Properties"], Some(0)),
        (vec!["Cắt", "Xóa", "&Khôi phục", "Thuộc tính"], Some(2)),
        (vec!["剪切(&T)", "删除(&D)", "还原(&E)"], Some(2)),
        (vec!["&Wiederherstellen", "Restore"], Some(0)),
        (vec!["Cut", "Delete"], None),
        (vec![], None),
    ] {
        assert_eq!(
            find_index(&names.into_iter().map(str::to_owned).collect::<Vec<_>>()),
            expected
        );
    }
}

#[test]
fn recognised_name_invokes_only_that_verb() {
    // C#: WindowsRecycleBinRestoreVerbTests.RecognisedNameInvokesOnlyThatVerb.
    let mut by_index = Vec::new();
    let mut canonical = Vec::new();
    let route = invoke(
        &["Cắt", "Khôi phục", "Xóa"].map(str::to_owned),
        |i| {
            by_index.push(i);
            Ok::<_, ()>(())
        },
        |v| {
            canonical.push(v.to_owned());
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(by_index, [1]);
    assert!(canonical.is_empty());
    assert_eq!(route, "verb:1");
}

#[test]
fn unrecognised_language_uses_canonical_verb() {
    // C#: WindowsRecycleBinRestoreVerbTests.UnrecognisedLanguageUsesCanonicalVerb.
    for names in [
        vec!["Cut", "Delete", "Properties"],
        vec!["Ausschneiden", "Löschen", "Eigenschaften"],
        vec!["Vágás", "Törlés", "Tulajdonságok"],
    ] {
        let mut by_index = Vec::new();
        let mut canonical = Vec::new();
        let route = invoke(
            &names.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            |i| {
                by_index.push(i);
                Ok::<_, ()>(())
            },
            |v| {
                canonical.push(v.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert!(by_index.is_empty());
        assert_eq!(canonical, [CANONICAL_VERB]);
        assert_eq!(route, CANONICAL_VERB);
    }
}

#[test]
fn empty_verb_list_uses_canonical_verb() {
    // C#: WindowsRecycleBinRestoreVerbTests.EmptyVerbListUsesCanonicalVerb.
    let mut canonical = Vec::new();
    invoke::<()>(
        &[],
        |_| panic!("no verb to invoke"),
        |verb| {
            canonical.push(verb.to_owned());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(canonical, [CANONICAL_VERB]);
}

#[test]
fn failing_verb_propagates_without_second_attempt() {
    // C#: WindowsRecycleBinRestoreVerbTests.FailingVerbPropagatesWithoutSecondAttempt.
    let mut canonical = Vec::new();
    let result = invoke(
        &["Restore".to_owned()],
        |_| Err("shell refused"),
        |verb| {
            canonical.push(verb.to_owned());
            Ok(())
        },
    );
    assert_eq!(result, Err("shell refused"));
    assert!(canonical.is_empty());
}

#[test]
fn canonical_verb_is_the_shell_undelete_name() {
    // C#: WindowsRecycleBinRestoreVerbTests.CanonicalVerbIsTheShellUndeleteName.
    assert_eq!(CANONICAL_VERB, "undelete");
}

#[test]
fn production_verb_bytes_are_the_nul_terminated_canonical_verb() {
    // ST-COM-RECYCLE T4: the bytes `invoke_restore` passes to the Shell come from this const, so this test covers them.
    assert_eq!(CANONICAL_VERB_Z.last(), Some(&0));
    assert_eq!(
        &CANONICAL_VERB_Z[..CANONICAL_VERB_Z.len() - 1],
        CANONICAL_VERB.as_bytes()
    );
}
