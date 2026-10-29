//! W10-R2: new pure tests of release resource serialization (no C# unit counterpart).
#[path = "../build_support/version_info.rs"]
mod version_info;

use version_info::*;
fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn dword(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn walk(bytes: &[u8], at: usize) -> usize {
    assert_eq!(at % 4, 0);
    let end = at + usize::from(word(bytes, at));
    let mut cursor = at + 6;
    while word(bytes, cursor) != 0 {
        cursor += 2;
    }
    cursor = (cursor + 2).next_multiple_of(4);
    cursor += usize::from(word(bytes, at + 2)) * if word(bytes, at + 4) == 1 { 2 } else { 1 };
    while cursor.next_multiple_of(4) < end {
        cursor = walk(bytes, cursor.next_multiple_of(4));
    }
    assert_eq!(cursor, end);
    end
}
#[test]
fn release_version_boundaries_and_ascii_semver_are_checked() {
    assert_eq!(components("0.65535.42"), Ok([0, 65535, 42]));
    for v in [
        "1.2",
        "1.2.3.0",
        "v1.2.3",
        "1.2.3\n",
        "１.2.3",
        "01.2.3",
        "0.0.0-dev",
        "1..3",
    ] {
        assert_eq!(components(v), Err(VersionError::InvalidVersion), "{v}");
    }
    assert_eq!(components("65536.0.0"), Err(VersionError::ComponentOutOfRange));
}
#[test]
fn release_resource_has_exact_fixed_values_strings_and_aligned_children() {
    let b = version_resource("12.34.56").unwrap();
    assert_eq!(walk(&b, 0), b.len());
    assert_eq!(word(&b, 2), 52);
    assert_eq!(dword(&b, 40), 0xfeef04bd);
    assert_eq!(dword(&b, 48), (12 << 16) | 34);
    assert_eq!(dword(&b, 52), 56 << 16);
    assert_eq!(dword(&b, 64), 0x3f);
    assert_eq!(dword(&b, 72), 0x40004);
    assert_eq!(dword(&b, 76), 1);
    for s in [
        "FileVersion",
        "ProductVersion",
        "12.34.56",
        "OriginalFilename",
        "PhotoReviewRs.exe",
        "040904B0",
        "Translation",
    ] {
        let needle: Vec<_> = s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        assert!(b.windows(needle.len()).any(|w| w == needle), "{s}");
    }
}
