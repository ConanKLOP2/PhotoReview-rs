//! W10-R2 / W8-20a: linked PE data, never LoadLibrary on the executable.
//! Ports ShellAssemblyTests manifest/name checks; release version checks are new.
#![cfg(windows)]
use std::fs;

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
struct Pe {
    bytes: Vec<u8>,
    base: usize,
    sections: usize,
    count: usize,
}
impl Pe {
    fn open() -> Self {
        let path = std::env::var_os("PHOTOREVIEW_RS_RESOURCE_EXE")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_PhotoReviewRs").into());
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[..2], b"MZ");
        let pe = u32_at(&bytes, 60) as usize;
        assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
        assert_eq!(u16_at(&bytes, pe + 4), 0x8664);
        let optional = pe + 24;
        assert_eq!(u16_at(&bytes, optional), 0x20b);
        let mut result = Self {
            count: u16_at(&bytes, pe + 6) as usize,
            sections: optional + u16_at(&bytes, pe + 20) as usize,
            base: 0,
            bytes,
        };
        result.base = result.rva(u32_at(&result.bytes, optional + 128));
        result
    }
    fn rva(&self, rva: u32) -> usize {
        for i in 0..self.count {
            let s = self.sections + 40 * i;
            let va = u32_at(&self.bytes, s + 12);
            if rva >= va && rva - va < u32_at(&self.bytes, s + 16) {
                return (u32_at(&self.bytes, s + 20) + rva - va) as usize;
            }
        }
        panic!("resource RVA outside file sections")
    }
    fn entries(&self, relative: usize) -> Vec<(u32, u32)> {
        let at = self.base + relative;
        let count = u16_at(&self.bytes, at + 12) as usize + u16_at(&self.bytes, at + 14) as usize;
        (0..count)
            .map(|i| {
                (
                    u32_at(&self.bytes, at + 16 + i * 8),
                    u32_at(&self.bytes, at + 20 + i * 8),
                )
            })
            .collect()
    }
    fn resource(&self, kind: u32, id: u32) -> Option<&[u8]> {
        let (_, directory) = self.entries(0).into_iter().find(|(key, _)| *key == kind)?;
        assert_ne!(directory & 0x80000000, 0);
        let (_, directory) = self
            .entries((directory & 0x7fffffff) as usize)
            .into_iter()
            .find(|(key, _)| *key == id)?;
        assert_ne!(directory & 0x80000000, 0);
        let entries = self.entries((directory & 0x7fffffff) as usize);
        let data = self.base + entries[0].1 as usize;
        let start = self.rva(u32_at(&self.bytes, data));
        Some(&self.bytes[start..start + u32_at(&self.bytes, data + 4) as usize])
    }
}
#[test]
fn shell_assembly_is_named_photoreview_rs_and_embeds_manifest_and_icon() {
    // C#: ShellAssembly_IsNamedPhotoReview, EmbeddedManifest_DeclaresPerMonitorV2DpiAwareness,
    // EmbeddedManifest_DependsOnCommonControlsV6; R0002 changes only the product name.
    assert!(env!("CARGO_BIN_EXE_PhotoReviewRs").ends_with("PhotoReviewRs.exe"));
    let pe = Pe::open();
    let manifest = std::str::from_utf8(pe.resource(24, 1).expect("RT_MANIFEST id 1")).unwrap();
    for marker in [
        "PerMonitorV2",
        "Microsoft.Windows.Common-Controls",
        "6.0.0.0",
        "asInvoker",
    ] {
        assert!(manifest.contains(marker), "{marker}");
    }
    let icon = pe.resource(14, 32512).expect("RT_GROUP_ICON id 32512");
    assert_eq!(u16_at(icon, 2), 1);
    assert!(u16_at(icon, 4) > 0);
}
#[test]
fn exe_version_resource_matches_tag() {
    let pe = Pe::open();
    if option_env!("PHOTOREVIEW_RS_RELEASE") != Some("1") {
        assert!(pe.resource(16, 1).is_none(), "local builds retain no VERSIONINFO");
        return;
    }
    let Some(version) = option_env!("PHOTOREVIEW_RS_VERSION") else {
        panic!("release version must be supplied at build time");
    };
    let b = pe.resource(16, 1).expect("release requires RT_VERSION id 1");
    assert_eq!(u32_at(b, 40), 0xfeef04bd);
    let parts: Vec<u32> = version.split('.').map(|s| s.parse().unwrap()).collect();
    assert_eq!(u32_at(b, 48), parts[0] << 16 | parts[1]);
    assert_eq!(u32_at(b, 52), parts[2] << 16);
    assert_eq!(u32_at(b, 56), u32_at(b, 48));
    assert_eq!(u32_at(b, 60), u32_at(b, 52));
    for (key, expected) in [
        ("FileVersion", version),
        ("ProductVersion", version),
        ("OriginalFilename", "PhotoReviewRs.exe"),
    ] {
        let wide: Vec<u8> = key.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        let at = b.windows(wide.len()).position(|s| s == wide).unwrap();
        let value_at = (at + wide.len()).next_multiple_of(4);
        let length = u16_at(b, at - 4) as usize;
        let value: Vec<u16> = b[value_at..value_at + length * 2]
            .chunks_exact(2)
            .map(|s| u16_at(s, 0))
            .take(length - 1)
            .collect();
        assert_eq!(String::from_utf16(&value).unwrap(), expected);
    }
}
