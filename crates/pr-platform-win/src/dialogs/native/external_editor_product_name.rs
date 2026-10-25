//! W9-E3: port of `PhotoReview.Shell.Win32/Dialogs/Native/ExternalEditorProductName.cs` (34 lines, baseline `7eaee2a2`): the suggested menu name of an editor program =
//! the `ProductName`, else the `FileDescription`, of the executable's version resource. Contract: `contracts/W9-E3.md`.
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::core::PCWSTR;

/// C# `ExternalEditorProductName.Suggest(exePath)`: `None` for a blank path, an unreadable file or when both texts are blank (the caller then uses
/// `ExternalEditors::name_from_path`). Reads `ProductName` then `FileDescription` (`GetFileVersionInfoSizeW` / `GetFileVersionInfoW` / `VerQueryValueW`, the
/// `Win32_Storage_FileSystem` feature is already enabled; the translation is the first of `\VarFileInfo\Translation`, as `FileVersionInfo` does) and returns the
/// first non-blank one trimmed. Never panics on a bad file; all `unsafe` is here with a SAFETY comment.
pub fn suggest(exe_path: &str) -> Option<String> {
    if exe_path.trim().is_empty() || exe_path.contains('\0') {
        return None;
    }
    let path: Vec<u16> = exe_path.encode_utf16().chain(Some(0)).collect();
    // SAFETY: path is NUL-terminated and remains alive for the synchronous call; handle output is unused.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    // The API fills a DWORD-aligned opaque version block. u64 backing storage guarantees alignment.
    let words = (size as usize).div_ceil(8);
    let mut block = Vec::<u64>::new();
    block.try_reserve_exact(words).ok()?;
    block.resize(words, 0);
    // SAFETY: the aligned allocation holds at least size writable bytes; path is a live terminated string.
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, block.as_mut_ptr().cast()) }.ok()?;
    // SAFETY: the initialized allocation holds size bytes and is retained for all queries below.
    let bytes = unsafe { std::slice::from_raw_parts(block.as_ptr().cast::<u8>(), size as usize) };
    let translation = query(bytes, r"\VarFileInfo\Translation").filter(|value| value.len() >= 4);
    let (language, codepage) = translation.map_or((0x0409, 0x04b0), |value| {
        (
            u16::from_le_bytes([value[0], value[1]]),
            u16::from_le_bytes([value[2], value[3]]),
        )
    });
    let read = |name: &str| {
        let value = query(
            bytes,
            &format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\{name}"),
        )?;
        let text: Vec<u16> = value
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        Some(String::from_utf16_lossy(&text))
    };
    let product = read("ProductName");
    let description = read("FileDescription");
    first_non_blank(product.as_deref(), description.as_deref())
}

/// Queries only within the live block. Win32 reports bytes for Translation, UTF-16 units for strings.
fn query<'a>(block: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let key_wide: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let mut pointer = std::ptr::null_mut();
    let mut length = 0;
    // SAFETY: block is the aligned version block returned by GetFileVersionInfoW and remains alive;
    // key_wide is terminated, and both output pointers reference writable locals.
    if !unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(key_wide.as_ptr()),
            &mut pointer,
            &mut length,
        )
    }
    .as_bool()
    {
        return None;
    }
    let offset = (pointer as usize).checked_sub(block.as_ptr() as usize)?;
    let size = if key == r"\VarFileInfo\Translation" {
        length as usize
    } else {
        (length as usize).checked_mul(2)?
    };
    block.get(offset..offset.checked_add(size)?)
}

/// The pure choice of `suggest`, testable without a file: the first of `product_name`, `file_description` that is not blank (`trim().is_empty()`), trimmed.
pub fn first_non_blank(product_name: Option<&str>, file_description: Option<&str>) -> Option<String> {
    [product_name, file_description]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|text| !text.is_empty())
        .map(str::to_owned)
}
