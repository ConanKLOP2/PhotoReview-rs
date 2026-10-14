//! Port of the C# ActionDestinationPolicy tests (W2-08) plus the `destination-policy` golden.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use pr_core::abstractions::{FileStat, FileSystem, ReadSeek};
use pr_core::catalog::types::ArgumentError;
use pr_core::file_actions::destination_policy::{
    ActionDestinationCheck, PickedFolderCheck, validate, validate_no_escape_via_reparse_point, validate_picked_folder,
};
use pr_core::file_actions::errors::get_path_root;
use pr_core::text::dotnet_path::get_full_path;
use pr_core::text::ignore_case_key;
use pr_test_support::golden;

use ActionDestinationCheck as A;

struct ReparseFs {
    links: HashMap<String, String>,
    fail_prefix: Option<(String, io::ErrorKind)>,
}

impl ReparseFs {
    fn new() -> Self {
        Self {
            links: HashMap::new(),
            fail_prefix: None,
        }
    }
    fn link(mut self, from: &str, to: &str) -> Self {
        self.links.insert(ignore_case_key(from), to.to_owned());
        self
    }
    fn failing(mut self, prefix: &str, kind: io::ErrorKind) -> Self {
        self.fail_prefix = Some((prefix.to_owned(), kind));
        self
    }
}

impl FileSystem for ReparseFs {
    fn file_exists(&self, _: &Path) -> bool {
        unimplemented!()
    }
    fn directory_exists(&self, _: &Path) -> bool {
        unimplemented!()
    }
    fn get_file_stat(&self, _: &Path) -> Option<FileStat> {
        unimplemented!()
    }
    fn move_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn copy_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn delete(&self, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn open_read_shared(&self, _: &Path, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        unimplemented!()
    }
    fn open_append(&self, _: &Path, _: bool) -> io::Result<Box<dyn io::Write + Send>> {
        unimplemented!()
    }
    fn write_all_text_atomic(&self, _: &Path, _: &str, _: bool) -> io::Result<()> {
        unimplemented!()
    }
    fn read_all_text(&self, _: &Path) -> io::Result<String> {
        unimplemented!()
    }
    fn enumerate_files(&self, _: &Path, _: &str) -> io::Result<Vec<PathBuf>> {
        unimplemented!()
    }
    fn enumerate_directories(&self, _: &Path) -> io::Result<Vec<PathBuf>> {
        unimplemented!()
    }
    fn create_directory(&self, _: &Path) -> io::Result<()> {
        unimplemented!()
    }
    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        let text = path.to_string_lossy().into_owned();
        if let Some((prefix, kind)) = &self.fail_prefix
            && ignore_case_key(&text).starts_with(&ignore_case_key(prefix))
        {
            return Err(io::Error::new(*kind, "simulated"));
        }
        let full = get_full_path(&text).map_err(|e| io::Error::other(e.to_string()))?;
        let Some(root) = get_path_root(&full) else {
            return Ok(PathBuf::from(full));
        };
        let rest = &full[root.len()..];
        let segments: Vec<&str> = rest.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return Ok(PathBuf::from(root));
        }
        let mut current = root.trim_end_matches(['\\', '/']).to_owned();
        for seg in segments {
            let candidate = if current.is_empty() {
                seg.to_owned()
            } else {
                format!("{current}\\{seg}")
            };
            current = self
                .links
                .get(&ignore_case_key(&candidate))
                .cloned()
                .unwrap_or(candidate);
        }
        Ok(PathBuf::from(current))
    }
}

fn name_of(c: ActionDestinationCheck) -> &'static str {
    match c {
        A::Ok => "Ok",
        A::Empty => "Empty",
        A::InvalidChars => "InvalidChars",
        A::EscapesSourceFolder => "EscapesSourceFolder",
    }
}

fn picked_name(c: PickedFolderCheck) -> &'static str {
    match c {
        PickedFolderCheck::Ok => "Ok",
        PickedFolderCheck::Empty => "Empty",
        PickedFolderCheck::NotAbsolute => "NotAbsolute",
        PickedFolderCheck::SameAsPhotoFolder => "SameAsPhotoFolder",
        PickedFolderCheck::Missing => "Missing",
    }
}

// C#: ActionDestinationPolicyTests.Validate_ClassifiesDestinations
#[test]
fn validate_classifies_destinations() {
    let rows: [(&str, A); 21] = [
        ("Loai-2", A::Ok),
        ("sub\\deeper", A::Ok),
        ("..\\x", A::EscapesSourceFolder),
        ("a\\..\\..\\x", A::EscapesSourceFolder),
        ("a/../../x", A::EscapesSourceFolder),
        ("D:\\Backup", A::Ok),
        ("\\\\server\\share\\x", A::Ok),
        ("\\photos", A::InvalidChars),
        ("/photos", A::InvalidChars),
        ("a|b", A::InvalidChars),
        ("a:b", A::InvalidChars),
        ("sel*", A::InvalidChars),
        ("a?b", A::InvalidChars),
        ("D:\\Backup\\*", A::InvalidChars),
        (r"\\?\D:\Backup", A::Ok),
        (r"\\?\UNC\server\share\x", A::Ok),
        (r"\\.\D:\Backup", A::Ok),
        (r"\\?\D:\Back*", A::InvalidChars),
        (r"\\?\D:\Back?up", A::InvalidChars),
        ("", A::Empty),
        ("  ", A::Empty),
    ];
    for (d, expected) in rows {
        assert_eq!(validate(Some(d)), expected, "{d:?}");
    }
    assert_eq!(validate(None), A::Empty);
}

// C#: ActionDestinationPolicyTests.ValidateNoEscapeViaReparsePoint_JunctionEscapesFolder_Rejected
#[test]
fn validate_no_escape_junction_escapes_folder_rejected() {
    let fs = ReparseFs::new().link(r"C:\photos\link", r"C:\outside");
    let r = validate_no_escape_via_reparse_point(r"C:\photos", r"C:\photos\link\processed", &fs);
    assert_eq!(r, Ok(A::EscapesSourceFolder));
}

// C#: ActionDestinationPolicyTests.ValidateNoEscapeViaReparsePoint_UnresolvableLink_FailsClosed
#[test]
fn validate_no_escape_unresolvable_link_fails_closed() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
        let fs = ReparseFs::new().failing(r"C:\photos\loop", kind);
        let r = validate_no_escape_via_reparse_point(r"C:\photos", r"C:\photos\loop\processed\a.jpg", &fs);
        assert_eq!(r, Ok(A::EscapesSourceFolder));
    }
}

// C#: ActionDestinationPolicyTests.ValidateNoEscapeViaReparsePoint_JunctionStaysInsideFolder_Allowed
#[test]
fn validate_no_escape_junction_stays_inside_folder_allowed() {
    let fs = ReparseFs::new().link(r"C:\photos\link", r"C:\photos\real");
    let r = validate_no_escape_via_reparse_point(r"C:\photos", r"C:\photos\link\processed", &fs);
    assert_eq!(r, Ok(A::Ok));
}

// C#: ActionDestinationPolicyTests.ValidateNoEscapeViaReparsePoint_OrdinaryDestination_Allowed
#[test]
fn validate_no_escape_ordinary_destination_allowed() {
    let r = validate_no_escape_via_reparse_point(r"C:\photos", r"C:\photos\sub\deeper", &ReparseFs::new());
    assert_eq!(r, Ok(A::Ok));
}

// C#: ActionDestinationPolicyTests.ValidateNoEscapeViaReparsePoint_DestinationFileIsSymlinkOutside_Rejected
#[test]
fn validate_no_escape_destination_file_is_symlink_outside_rejected() {
    let fs = ReparseFs::new().link(r"C:\photos\a.jpg", r"C:\outside\a.jpg");
    let r = validate_no_escape_via_reparse_point(r"C:\photos", r"C:\photos\a.jpg", &fs);
    assert_eq!(r, Ok(A::EscapesSourceFolder));
}

// C#: ActionDestinationRootTests.ValidateNoEscapeViaReparsePoint_SourceAtRoot_DestinationInsideIsOk
#[test]
fn validate_no_escape_source_at_root_destination_inside_is_ok() {
    let rows = [
        (r"C:\", r"C:\sel\a.jpg"),
        (r"C:\", r"C:\sel"),
        (r"\\server\share", r"\\server\share\sel\a.jpg"),
        (r"\\server\share\", r"\\server\share\sel"),
    ];
    for (s, d) in rows {
        assert_eq!(
            validate_no_escape_via_reparse_point(s, d, &ReparseFs::new()),
            Ok(A::Ok),
            "{s} {d}"
        );
    }
}

// C#: ActionDestinationRootTests.ValidateNoEscapeViaReparsePoint_SourceAtRoot_DestinationOnAnotherRootEscapes
#[test]
fn validate_no_escape_source_at_root_destination_on_another_root_escapes() {
    let rows = [
        (r"C:\", r"D:\sel\a.jpg"),
        (r"\\server\share", r"\\server\share2\sel"),
        (r"\\server\share", r"\\other\share\sel"),
    ];
    for (s, d) in rows {
        assert_eq!(
            validate_no_escape_via_reparse_point(s, d, &ReparseFs::new()),
            Ok(A::EscapesSourceFolder),
            "{s} {d}"
        );
    }
}

#[test]
fn validate_no_escape_blank_arguments_are_errors() {
    let fs = ReparseFs::new();
    assert_eq!(
        validate_no_escape_via_reparse_point(" ", r"C:\a", &fs),
        Err(ArgumentError::BlankPath {
            param_name: "sourceFolder"
        })
    );
    assert_eq!(
        validate_no_escape_via_reparse_point(r"C:\a", "", &fs),
        Err(ArgumentError::BlankPath {
            param_name: "destinationFolderOrPath"
        })
    );
}

// Mutation repair (M8): the containment prefix comparison is OrdinalIgnoreCase.
#[test]
fn validate_no_escape_prefix_comparison_is_case_insensitive() {
    for (s, d) in [(r"C:\Photos", r"c:\photos\sub"), (r"C:\", r"c:\x")] {
        assert_eq!(
            validate_no_escape_via_reparse_point(s, d, &ReparseFs::new()),
            Ok(A::Ok),
            "{s} {d}"
        );
    }
}

// Mutation repair (M11): the whole four-char `\\?\` prefix is required to strip.
#[test]
fn validate_incomplete_extended_prefix_is_invalid_chars() {
    for d in [r"\\?x", r"\\?"] {
        assert_eq!(validate(Some(d)), A::InvalidChars, "{d:?}");
    }
}

// Mutation repair (M20): a whitespace-only destination is a BlankPath error.
#[test]
fn validate_no_escape_whitespace_destination_is_error() {
    assert_eq!(
        validate_no_escape_via_reparse_point(r"C:\a", "  ", &ReparseFs::new()),
        Err(ArgumentError::BlankPath {
            param_name: "destinationFolderOrPath"
        })
    );
}

// C#: FileActionSafetyEdgeTests.Policy_PaddedParentSegment_Escapes
#[test]
fn policy_padded_parent_segment_escapes() {
    for d in [r".. \x", r"a\ .. \x", " .."] {
        assert_eq!(validate(Some(d)), A::EscapesSourceFolder, "{d:?}");
    }
}

// C#: FileActionsSmallMutationGapTests.Validate_NameStartingWithAnInvalidCharacter_IsRejected
#[test]
fn validate_name_starting_with_an_invalid_character_is_rejected() {
    for d in ["*sel", "?sel", "|sel", r"\\?\*sel", r"\\?\|sel"] {
        assert_eq!(validate(Some(d)), A::InvalidChars, "{d:?}");
    }
}

const PHOTO_FOLDER: &str = r"D:\Photos\Trip";

fn existing() -> HashSet<String> {
    [
        r"D:\Photos",
        r"D:\Photos\Trip",
        r"D:\Photos\Trip\Best",
        r"E:\Backup",
        r"\\server\share\x",
        r"E:\",
    ]
    .iter()
    .map(|s| ignore_case_key(s))
    .collect()
}

// C#: PickedFolderPolicyTests.ValidatePickedFolder_Classifies
#[test]
fn validate_picked_folder_classifies() {
    use PickedFolderCheck as P;
    let rows: [(Option<&str>, P); 18] = [
        (Some(r"E:\Backup"), P::Ok),
        (Some(r"E:\Backup\"), P::Ok),
        (Some(r"E:\"), P::Ok),
        (Some(r"D:\Photos"), P::Ok),
        (Some(r"D:\Photos\Trip\Best"), P::Ok),
        (Some(r"\\server\share\x"), P::Ok),
        (Some(r"D:\Photos\Trip"), P::SameAsPhotoFolder),
        (Some(r"d:\photos\TRIP\"), P::SameAsPhotoFolder),
        (Some(r"D:\Photos\Trip\Best\.."), P::SameAsPhotoFolder),
        (Some(r"E:\Gone"), P::Missing),
        (Some("Best"), P::NotAbsolute),
        (Some(r"..\Other"), P::NotAbsolute),
        (Some(r"\Photos"), P::NotAbsolute),
        (Some("E:Backup"), P::NotAbsolute),
        (Some(r"E:\a|b"), P::NotAbsolute),
        (Some(""), P::Empty),
        (Some("  "), P::Empty),
        (None, P::Empty),
    ];
    let set = existing();
    for (d, expected) in rows {
        let r = validate_picked_folder(d, PHOTO_FOLDER, &|p| set.contains(&ignore_case_key(p)));
        assert_eq!(r, Ok(expected), "{d:?}");
    }
}

// C#: PickedFolderPolicyTests.ValidatePickedFolder_ChecksExistenceOfTheNormalizedPath
#[test]
fn validate_picked_folder_checks_existence_of_the_normalized_path() {
    let asked = RefCell::new(Vec::<String>::new());
    let r = validate_picked_folder(Some(r"E:\Backup\"), PHOTO_FOLDER, &|p| {
        asked.borrow_mut().push(p.to_owned());
        true
    });
    assert_eq!(r, Ok(PickedFolderCheck::Ok));
    assert_eq!(*asked.borrow(), vec![r"E:\Backup".to_owned()]);
}

#[test]
fn golden_destination_policy() {
    let g = golden::load("destination-policy");
    for case in &g.cases {
        match golden::str_of(case, "kind") {
            "validate" => {
                let d = case["destination"].as_str();
                assert_eq!(name_of(validate(d)), golden::str_of(case, "result"), "{case}");
            }
            "picked" => {
                let set: HashSet<String> = case["existing"]
                    .as_array()
                    .expect("existing")
                    .iter()
                    .map(|v| ignore_case_key(v.as_str().expect("str")))
                    .collect();
                let asked = RefCell::new(Vec::<String>::new());
                let r = validate_picked_folder(
                    case["destination"].as_str(),
                    golden::str_of(case, "photo_folder"),
                    &|p| {
                        asked.borrow_mut().push(p.to_owned());
                        set.contains(&ignore_case_key(p))
                    },
                )
                .unwrap_or_else(|e| panic!("{case}: {e}"));
                assert_eq!(picked_name(r), golden::str_of(case, "result"), "{case}");
                let expected: Vec<String> = case["asked"]
                    .as_array()
                    .expect("asked")
                    .iter()
                    .map(|v| v.as_str().expect("str").to_owned())
                    .collect();
                assert_eq!(*asked.borrow(), expected, "{case}");
            }
            "no_escape" => {
                let mut fs = ReparseFs::new();
                if let Some(map) = case["reparse"].as_object() {
                    for (k, v) in map {
                        fs = fs.link(k, v.as_str().expect("target"));
                    }
                }
                if let Some(prefix) = case["throw_prefix"].as_str() {
                    let kind = match case["throw_kind"].as_str() {
                        Some("unauthorized") => io::ErrorKind::PermissionDenied,
                        _ => io::ErrorKind::Other,
                    };
                    fs = fs.failing(prefix, kind);
                }
                let r = validate_no_escape_via_reparse_point(
                    golden::str_of(case, "source"),
                    golden::str_of(case, "destination"),
                    &fs,
                );
                if case["result"]["throws"].is_string() {
                    assert!(matches!(r, Err(ArgumentError::BlankPath { .. })), "{case}");
                } else {
                    let r = r.unwrap_or_else(|e| panic!("{case}: {e}"));
                    assert_eq!(name_of(r), golden::str_of(case, "result"), "{case}");
                }
            }
            other => panic!("unknown kind {other}"),
        }
    }
}

// Mutation repair (round 2): a resolved real path keeping a trailing separator is trimmed
// before comparison, on the source side and on the destination side.
#[test]
fn validate_no_escape_resolved_path_with_trailing_separator_is_trimmed() {
    let fs = ReparseFs::new().link(r"C:\photos", "D:\\real\\photos\\");
    assert_eq!(
        validate_no_escape_via_reparse_point(r"C:\photos", r"D:\real\photos", &fs),
        Ok(A::Ok)
    );
    let fs = ReparseFs::new().link(r"C:\dest", "C:\\photos\\");
    assert_eq!(
        validate_no_escape_via_reparse_point(r"C:\photos", r"C:\dest", &fs),
        Ok(A::Ok)
    );
}
