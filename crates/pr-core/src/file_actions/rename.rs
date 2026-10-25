//! Port of `PhotoReview.Core/FileActions/{RenameNamePolicy,CaptureGroupRenamePlan,RenameRequest}.cs` at 7eaee2a2 (W9-R1).
//! `CaseOnlyRename` and `FileActionService::rename*` are W9-R2 (`case_only_rename.rs`, `rename_service.rs`).
use crate::catalog::types::{CaptureGroup, get_extension};
use crate::text::{dotnet_path, eq_ignore_case, ignore_case_key};
use std::collections::HashMap;

/// C# Dictionary(StringComparer.OrdinalIgnoreCase), retaining the first key spelling.
#[derive(Debug, Clone, Default)]
pub struct PathKeyMap {
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
}

impl PathKeyMap {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn get(&self, key: &str) -> Option<&String> {
        self.index
            .get(&ignore_case_key(key))
            .and_then(|index| self.entries.get(*index))
            .map(|(_, value)| value)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        let normalized = ignore_case_key(&key);
        if let Some(entry) = self
            .index
            .get(&normalized)
            .and_then(|index| self.entries.get_mut(*index))
        {
            return Some(std::mem::replace(&mut entry.1, value));
        }
        self.index.insert(normalized, self.entries.len());
        self.entries.push((key, value));
        None
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl FromIterator<(String, String)> for PathKeyMap {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut map = Self::new();
        for (key, value) in iter {
            map.insert(key, value);
        }
        map
    }
}

impl PartialEq for PathKeyMap {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(key, value)| other.get(key) == Some(value))
    }
}
impl Eq for PathKeyMap {}

/// The C# `RenameNameProblem` discriminants, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RenameNameProblem {
    None,
    Empty,
    InvalidCharacter,
    TrailingDotOrSpace,
    ReservedDeviceName,
    NameTooLong,
    PathTooLong,
    SidecarNotMappable,
}

/// Pure name verdict; `new_file_name` includes the original extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameNameCheck {
    pub problem: RenameNameProblem,
    pub new_file_name: String,
    /// C# `char` is one UTF-16 code unit, including an unpaired surrogate.
    pub invalid_character: Option<u16>,
    pub is_unchanged: bool,
    pub differs_only_by_case: bool,
}

impl RenameNameCheck {
    fn invalid(problem: RenameNameProblem, invalid_character: Option<u16>) -> Self {
        Self {
            problem,
            new_file_name: String::new(),
            invalid_character,
            is_unchanged: false,
            differs_only_by_case: false,
        }
    }
    /// C# `IsValid`: `Problem == None`.
    pub fn is_valid(&self) -> bool {
        self.problem == RenameNameProblem::None
    }
}

/// C# `RenameNamePolicy`, with no file-system access.
#[derive(Debug)]
pub struct RenameNamePolicy;
impl RenameNamePolicy {
    pub const MAX_FILE_NAME_LENGTH: usize = 255;
    pub const MAX_PATH: usize = 260;
    /// C# `Check`: `typed_stem` is the user's name part.
    pub fn check(typed_stem: Option<&str>, current_full_path: &str, long_path_aware: bool) -> RenameNameCheck {
        let units = typed_stem.map(|text| text.encode_utf16().collect::<Vec<_>>());
        Self::check_utf16(units.as_deref(), current_full_path, long_path_aware)
    }
    /// Native edit-control boundary that can preserve unpaired UTF-16 units.
    pub fn check_utf16(typed_stem: Option<&[u16]>, current_full_path: &str, long_path_aware: bool) -> RenameNameCheck {
        if current_full_path.trim().is_empty() {
            // D-W9R-12: a verdict replaces the C# argument exception at the UI boundary.
            return RenameNameCheck::invalid(RenameNameProblem::Empty, None);
        }
        let current_name = dotnet_path::get_file_name(current_full_path);
        let extension = get_extension(current_name);
        let extension_units = extension.encode_utf16().count();
        let mut stem = typed_stem.unwrap_or_default();
        while stem.first().is_some_and(|unit| unit_is_whitespace(*unit)) {
            stem = &stem[1..];
        }
        while stem.last().is_some_and(|unit| unit_is_whitespace(*unit)) {
            stem = &stem[..stem.len() - 1];
        }
        if extension_units > 0 && stem.len() >= extension_units {
            let tail_start = stem.len() - extension_units;
            if String::from_utf16(&stem[tail_start..]).is_ok_and(|tail| eq_ignore_case(&tail, extension)) {
                stem = &stem[..tail_start];
                while stem.last().is_some_and(|unit| unit_is_whitespace(*unit)) {
                    stem = &stem[..stem.len() - 1];
                }
            }
        }
        if stem.is_empty() {
            return RenameNameCheck::invalid(RenameNameProblem::Empty, None);
        }
        if let Some(bad) = first_invalid_character(stem) {
            return RenameNameCheck::invalid(RenameNameProblem::InvalidCharacter, Some(bad));
        }
        if stem.last() == Some(&(b'.' as u16)) {
            return RenameNameCheck::invalid(RenameNameProblem::TrailingDotOrSpace, None);
        }
        let Ok(stem) = String::from_utf16(stem) else {
            return RenameNameCheck::invalid(RenameNameProblem::InvalidCharacter, None);
        };
        let file_name = stem + extension;
        if file_name.encode_utf16().count() > Self::MAX_FILE_NAME_LENGTH {
            return RenameNameCheck::invalid(RenameNameProblem::NameTooLong, None);
        }
        if Self::is_reserved_device_name(&file_name) {
            return RenameNameCheck::invalid(RenameNameProblem::ReservedDeviceName, None);
        }
        if Self::exceeds_path_budget(current_full_path, &file_name, long_path_aware) {
            return RenameNameCheck::invalid(RenameNameProblem::PathTooLong, None);
        }
        let unchanged = file_name == current_name;
        let case_only = !unchanged && eq_ignore_case(&file_name, current_name);
        RenameNameCheck {
            problem: RenameNameProblem::None,
            new_file_name: file_name,
            invalid_character: None,
            is_unchanged: unchanged,
            differs_only_by_case: case_only,
        }
    }
    /// C# `CheckFileName`: structural check of a complete target component.
    pub fn check_file_name(file_name: Option<&str>) -> RenameNameProblem {
        let Some(file_name) = file_name.filter(|name| !name.trim().is_empty()) else {
            return RenameNameProblem::Empty;
        };
        let units = file_name.encode_utf16().collect::<Vec<_>>();
        if first_invalid_character(&units).is_some() {
            return RenameNameProblem::InvalidCharacter;
        }
        if file_name.ends_with(['.', ' ']) {
            return RenameNameProblem::TrailingDotOrSpace;
        }
        if units.len() > Self::MAX_FILE_NAME_LENGTH {
            return RenameNameProblem::NameTooLong;
        }
        if Self::is_reserved_device_name(file_name) {
            return RenameNameProblem::ReservedDeviceName;
        }
        RenameNameProblem::None
    }
    /// C# `ExceedsPathBudget`.
    pub fn exceeds_path_budget(current_full_path: &str, new_file_name: &str, long_path_aware: bool) -> bool {
        // D-W9R-12: reject a blank source rather than panic, including long-path mode.
        if current_full_path.trim().is_empty() {
            return true;
        }
        if long_path_aware {
            return false;
        }
        let Some(folder) = dotnet_path::get_directory_name(current_full_path).filter(|folder| !folder.is_empty())
        else {
            return false;
        };
        let length = dotnet_path::trim_ending_directory_separator(&folder)
            .encode_utf16()
            .count()
            + 1
            + new_file_name.encode_utf16().count();
        length >= Self::MAX_PATH && length > current_full_path.encode_utf16().count()
    }
    /// C# `IsReservedDeviceName`.
    pub fn is_reserved_device_name(file_name: &str) -> bool {
        let head = file_name.split('.').next().unwrap_or_default().trim_end_matches(' ');
        if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
            .iter()
            .any(|device| eq_ignore_case(head, device))
        {
            return true;
        }
        let units = head.encode_utf16().collect::<Vec<_>>();
        if units.len() != 4 {
            return false;
        }
        let prefix = String::from_utf16_lossy(&units[..3]);
        (eq_ignore_case(&prefix, "COM") || eq_ignore_case(&prefix, "LPT"))
            && matches!(units.get(3), Some(0x30..=0x39 | 0xb9 | 0xb2 | 0xb3))
    }
}

/// C# `CaptureGroupRenamePlan.Plan`: path keys use OrdinalIgnoreCase semantics when implemented.
#[derive(Debug, Clone)]
pub struct CaptureGroupRenamePlan {
    pub check: RenameNameCheck,
    pub file_names: Option<PathKeyMap>,
}
impl CaptureGroupRenamePlan {
    /// C# `Build` for JPEG, RAW and optional XMP.
    pub fn build(group: &CaptureGroup, typed_stem: Option<&str>, long_path_aware: bool) -> Self {
        let mut check = RenameNamePolicy::check(typed_stem, group.jpeg_path(), long_path_aware);
        if !check.is_valid() {
            return Self {
                check,
                file_names: None,
            };
        }
        let stem = check
            .new_file_name
            .strip_suffix(get_extension(group.jpeg_path()))
            .unwrap_or(&check.new_file_name);
        let raw_name = format!("{stem}{}", get_extension(group.raw_path()));
        if let Some(problem) = member_problem(group.raw_path(), &raw_name, long_path_aware) {
            return Self {
                check: RenameNameCheck::invalid(problem, None),
                file_names: None,
            };
        }
        let mut names = PathKeyMap::new();
        names.insert(group.jpeg_path().to_owned(), check.new_file_name.clone());
        names.insert(group.raw_path().to_owned(), raw_name.clone());
        if let Some(xmp_path) = group.xmp_path() {
            let xmp_name = dotnet_path::get_file_name(xmp_path);
            let extension = get_extension(xmp_name);
            let owner = xmp_name.strip_suffix(extension).unwrap_or(xmp_name);
            let new_xmp = if eq_ignore_case(owner, dotnet_path::get_file_name_without_extension(group.jpeg_path()))
                || eq_ignore_case(owner, dotnet_path::get_file_name_without_extension(group.raw_path()))
            {
                format!("{stem}{extension}")
            } else if eq_ignore_case(owner, dotnet_path::get_file_name(group.jpeg_path())) {
                format!("{}{extension}", check.new_file_name)
            } else if eq_ignore_case(owner, dotnet_path::get_file_name(group.raw_path())) {
                format!("{raw_name}{extension}")
            } else {
                return Self {
                    check: RenameNameCheck::invalid(RenameNameProblem::SidecarNotMappable, None),
                    file_names: None,
                };
            };
            if let Some(problem) = member_problem(xmp_path, &new_xmp, long_path_aware) {
                return Self {
                    check: RenameNameCheck::invalid(problem, None),
                    file_names: None,
                };
            }
            names.insert(xmp_path.to_owned(), new_xmp);
        }
        check.is_unchanged = names
            .iter()
            .all(|(path, name)| dotnet_path::get_file_name(path) == name);
        check.differs_only_by_case = !check.is_unchanged
            && names
                .iter()
                .all(|(path, name)| eq_ignore_case(dotnet_path::get_file_name(path), name));
        Self {
            check,
            file_names: Some(names),
        }
    }
}

fn member_problem(path: &str, name: &str, long_path_aware: bool) -> Option<RenameNameProblem> {
    let problem = RenameNamePolicy::check_file_name(Some(name));
    if problem != RenameNameProblem::None {
        Some(problem)
    } else if RenameNamePolicy::exceeds_path_budget(path, name, long_path_aware) {
        Some(RenameNameProblem::PathTooLong)
    } else {
        None
    }
}

fn unit_is_whitespace(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace)
}

fn first_invalid_character(text: &[u16]) -> Option<u16> {
    text.iter().copied().enumerate().find_map(|(index, unit)| {
        let forbidden = unit < 0x20 || matches!(unit, 0x3c | 0x3e | 0x3a | 0x22 | 0x2f | 0x5c | 0x7c | 0x3f | 0x2a);
        let lone_low = (0xdc00..=0xdfff).contains(&unit)
            && !index
                .checked_sub(1)
                .and_then(|previous| text.get(previous))
                .is_some_and(|previous| (0xd800..=0xdbff).contains(previous));
        let lone_high = (0xd800..=0xdbff).contains(&unit)
            && !text.get(index + 1).is_some_and(|next| (0xdc00..=0xdfff).contains(next));
        (forbidden || lone_low || lone_high).then_some(unit)
    })
}

/// C# `RenameRequest`: complete new file name, never a destination folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameRequest {
    pub source: String,
    pub new_file_name: String,
}
