//! W10-R2; release-only Win32 VERSIONINFO. Port of the release version resource
//! configured by PhotoReview.Shell.Win32.csproj and Directory.Build.targets.
//! Layout/constants checked against Windows SDK 10.0.26100.0 um/verrsrc.h.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    InvalidVersion,
    ComponentOutOfRange,
    LengthOverflow,
}

pub fn components(version: &str) -> Result<[u16; 3], VersionError> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(VersionError::InvalidVersion);
    }
    let mut result = [0; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || (part.len() > 1 && part.starts_with('0')) {
            return Err(VersionError::InvalidVersion);
        }
        result[i] = part.parse().map_err(|_| VersionError::ComponentOutOfRange)?;
    }
    Ok(result)
}

fn align(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}
fn block(key: &str, value: &[u8], text: bool, children: &[Vec<u8>]) -> Result<Vec<u8>, VersionError> {
    let value_len = if text { value.len() / 2 } else { value.len() };
    let value_len = u16::try_from(value_len).map_err(|_| VersionError::LengthOverflow)?;
    let mut out = vec![0, 0];
    out.extend(value_len.to_le_bytes());
    out.extend(u16::from(text).to_le_bytes());
    out.extend(wide(key));
    align(&mut out);
    out.extend(value);
    for child in children {
        align(&mut out);
        out.extend(child);
    }
    let len = u16::try_from(out.len()).map_err(|_| VersionError::LengthOverflow)?;
    out[..2].copy_from_slice(&len.to_le_bytes());
    Ok(out)
}

pub fn version_resource(version: &str) -> Result<Vec<u8>, VersionError> {
    let [major, minor, patch] = components(version)?;
    let ms = (u32::from(major) << 16) | u32::from(minor);
    let ls = u32::from(patch) << 16;
    // VS_FFI_SIGNATURE, VS_FFI_STRUCVERSION, FILEFLAGSMASK, VOS_NT_WINDOWS32, VFT_APP.
    let fixed: Vec<u8> = [0xfeef04bd, 0x10000, ms, ls, ms, ls, 0x3f, 0, 0x40004, 1, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut strings = Vec::new();
    // No unverified company/copyright claim. The application name is project-owned (R0002).
    for (key, value) in [
        ("FileDescription", "PhotoReviewRs"),
        ("ProductName", "PhotoReviewRs"),
        ("ProductVersion", version),
        ("FileVersion", version),
        ("OriginalFilename", "PhotoReviewRs.exe"),
    ] {
        strings.push(block(key, &wide(value), true, &[])?);
    }
    let table = block("040904B0", &[], true, &strings)?;
    let string_info = block("StringFileInfo", &[], true, &[table])?;
    let translation = block("Translation", &[0x09, 0x04, 0xb0, 0x04], false, &[])?;
    let var_info = block("VarFileInfo", &[], true, &[translation])?;
    block("VS_VERSION_INFO", &fixed, false, &[string_info, var_info])
}
