//! `display_name` root handling, expected values derived from .NET `Path.TrimEndingDirectorySeparator` +
//! `Path.GetFileName` (oracle: .NET 10.0.12) with the whole input as fallback for an empty name.
use pr_app::viewmodels::info_overlay::display_name;

#[test]
fn display_name_matches_dotnet_for_roots_and_devices() {
    let cases: &[(&str, &str)] = &[
        ("", ""),
        ("abc", "abc"),
        ("ab", "ab"),
        ("1:foo", "1:foo"),
        ("C:", "C:"),
        (r"C:\", r"C:\"),
        ("C:foo", "foo"),
        (r"C:\foo", "foo"),
        ("C:/foo/", "foo"),
        (r"\", r"\"),
        (r"\foo", "foo"),
        (r"\\srv\share\dir", "dir"),
        (r"\\srv\share", r"\\srv\share"),
        (r"\\?x", r"\\?x"),
        ("//?", "//?"),
        // Extended and device prefixes.
        (r"\\?\C:\photos", "photos"),
        (r"\\?\C:\", r"\\?\C:\"),
        (r"\\?\", r"\\?\"),
        (r"\\?\a", r"\\?\a"),
        (r"\??\C:\a", "a"),
        (r"\??\x", r"\??\x"),
        ("//?/C:/a/b", "b"),
        ("//./PHYS/x", "x"),
        (r"\\.\C:\a", "a"),
        (r"//?\C:\a", "a"),
        // Device UNC: marker is case-sensitive, separator required at index 7.
        (r"\\?\UNC\srv\share\dir", "dir"),
        (r"\\?\UNC\srv\share", r"\\?\UNC\srv\share"),
        (r"\\?\UNC\", r"\\?\UNC\"),
        (r"\\?\unc\srv\dir", "dir"),
        ("//?/unc/srv", "srv"),
        ("//?/unc/UNCx", "UNCx"),
        (r"\\??\Unc\\UNCx.", "UNCx."),
        (r"\\?\UNCx\a\b", "b"),
        (r"\\?\uNc\srv\share\dir", "dir"),
    ];
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|(input, want)| {
            let got = display_name(input);
            (got != *want).then(|| format!("{input:?}: got {got:?}, want {want:?}"))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}
