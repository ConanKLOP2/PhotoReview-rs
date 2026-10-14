//! Port of `PhotoReview.Core/Catalog/CaptureGroupBuilder.cs` (C# baseline d92d6aae): finds the unambiguous
//! same-folder, same-basename JPEG+RAW captures (with their optional XMP sidecar) and collapses each pair into one
//! catalog entry (PARITY I-17, `RawPairMode`). `CaptureGroup` and `CatalogEntry` themselves live in
//! [`super::types`] (W1-07).

use std::collections::HashMap;

use crate::catalog::types::{CaptureGroup, CatalogEntry, JPEG_EXTENSIONS, RAW_EXTENSIONS, extension_in, get_extension};
use crate::model::RawPairMode;
use crate::text::dotnet_path::{get_directory_name, get_file_name, get_file_name_without_extension};
use crate::text::{eq_ignore_case, ignore_case_key};

/// `(folder, name)` compared OrdinalIgnoreCase through `ignore_case_key` (C# `CaptureKey`).
type CaptureKey = (String, String);

fn capture_key(directory: &str, name: &str) -> CaptureKey {
    (ignore_case_key(directory), ignore_case_key(name))
}

/// Only the first path and a count per kind are kept: a bucket is grouped iff it has exactly one of each.
struct PathBucket<'a> {
    key: CaptureKey,
    jpeg_path: Option<&'a str>,
    raw_path: Option<&'a str>,
    jpeg_count: u32,
    raw_count: u32,
}

/// C# `IndexSidecars`: `.xmp` paths only, by (folder, name without `.xmp`), each key keeping its paths in input order.
fn index_sidecars(sidecar_paths: Option<&[String]>) -> HashMap<CaptureKey, Vec<&str>> {
    let mut result: HashMap<CaptureKey, Vec<&str>> = HashMap::new();
    let Some(paths) = sidecar_paths else {
        return result;
    };
    for path in paths {
        if path.trim().is_empty() || !eq_ignore_case(get_extension(path), ".xmp") {
            continue;
        }
        let directory = get_directory_name(path).unwrap_or_default();
        let key = capture_key(&directory, get_file_name_without_extension(path));
        result.entry(key).or_default().push(path);
    }
    result
}

/// C# `ChooseSidecar`: `<base>.xmp` wins (two of them => none); else exactly one `<file>.<ext>.xmp` of either member.
fn choose_sidecar(
    sidecars: &HashMap<CaptureKey, Vec<&str>>,
    key: &CaptureKey,
    jpeg_path: &str,
    raw_path: &str,
) -> Option<String> {
    if sidecars.is_empty() {
        return None;
    }
    if let Some(exact) = sidecars.get(key) {
        return if exact.len() == 1 {
            Some(exact[0].to_owned())
        } else {
            None
        };
    }
    let mut found: Option<&str> = None;
    let mut count = 0usize;
    for member in [jpeg_path, raw_path] {
        let directory = get_directory_name(member).unwrap_or_default();
        let member_key = capture_key(&directory, get_file_name(member));
        if let Some(by_file) = sidecars.get(&member_key) {
            for candidate in by_file {
                count += 1;
                if found.is_none() {
                    found = Some(candidate);
                }
            }
        }
    }
    if count == 1 { found.map(str::to_owned) } else { None }
}

/// `CaptureGroupBuilder.Build(imagePaths, sidecarPaths)`.
///
/// Result order = the order in which each (folder, basename) bucket was FIRST seen in `image_paths`.
/// A bucket becomes a group only with exactly one JPEG and exactly one RAW. The sidecar is `<base>.xmp` when exactly
/// one exists (two or more => none, file-extension sidecars not consulted); otherwise exactly one
/// `<file>.<ext>.xmp` of either member; zero or several => none.
pub fn build(image_paths: &[String], sidecar_paths: Option<&[String]>) -> Vec<CaptureGroup> {
    let mut index: HashMap<CaptureKey, usize> = HashMap::new();
    let mut order: Vec<PathBucket<'_>> = Vec::new();
    for path in image_paths {
        if path.trim().is_empty() {
            continue;
        }
        let extension = get_extension(path);
        let is_jpeg = extension_in(JPEG_EXTENSIONS, extension);
        if !is_jpeg && !extension_in(RAW_EXTENSIONS, extension) {
            continue;
        }
        let directory = get_directory_name(path).unwrap_or_default();
        let key = capture_key(&directory, get_file_name_without_extension(path));
        let slot = match index.get(&key) {
            Some(&slot) => slot,
            None => {
                let slot = order.len();
                index.insert(key.clone(), slot);
                order.push(PathBucket {
                    key,
                    jpeg_path: None,
                    raw_path: None,
                    jpeg_count: 0,
                    raw_count: 0,
                });
                slot
            }
        };
        let bucket = &mut order[slot];
        if is_jpeg {
            if bucket.jpeg_count == 0 {
                bucket.jpeg_path = Some(path);
            }
            bucket.jpeg_count += 1;
        } else {
            if bucket.raw_count == 0 {
                bucket.raw_path = Some(path);
            }
            bucket.raw_count += 1;
        }
    }

    let sidecars = index_sidecars(sidecar_paths);
    let mut groups = Vec::new();
    for bucket in &order {
        if bucket.jpeg_count != 1 || bucket.raw_count != 1 {
            continue;
        }
        let (Some(jpeg), Some(raw)) = (bucket.jpeg_path, bucket.raw_path) else {
            continue;
        };
        let xmp = choose_sidecar(&sidecars, &bucket.key, jpeg, raw);
        if let Ok(group) = CaptureGroup::new(jpeg, raw, xmp) {
            groups.push(group);
        }
    }
    groups
}

/// `CaptureGroupBuilder.GroupEntries(entries, mode, sidecarPaths)`: replaces each pair's members with ONE entry,
/// placed where the first member was seen, cloned from the representative's entry with `capture_group` set.
/// `Separate` or no groups => a plain clone of `entries`.
pub fn group_entries(
    entries: &[CatalogEntry],
    mode: RawPairMode,
    sidecar_paths: Option<&[String]>,
) -> Vec<CatalogEntry> {
    if mode == RawPairMode::Separate {
        return entries.to_vec();
    }
    let paths: Vec<String> = entries.iter().map(|entry| entry.path().to_owned()).collect();
    let groups = build(&paths, sidecar_paths);
    if groups.is_empty() {
        return entries.to_vec();
    }

    let mut entry_by_path: HashMap<String, usize> = HashMap::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        entry_by_path.entry(ignore_case_key(entry.path())).or_insert(position);
    }
    let mut group_by_member: HashMap<String, usize> = HashMap::with_capacity(groups.len() * 2);
    for (position, group) in groups.iter().enumerate() {
        group_by_member.insert(ignore_case_key(group.jpeg_path()), position);
        group_by_member.insert(ignore_case_key(group.raw_path()), position);
    }

    let mut emitted = vec![false; groups.len()];
    let mut grouped = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(&group_index) = group_by_member.get(&ignore_case_key(entry.path())) else {
            grouped.push(entry.clone());
            continue;
        };
        if emitted[group_index] {
            continue;
        }
        emitted[group_index] = true;
        let group = &groups[group_index];
        let Ok(representative_path) = group.get_representative_path(mode) else {
            continue;
        };
        if let Some(&position) = entry_by_path.get(&ignore_case_key(representative_path)) {
            let mut representative = entries[position].clone();
            representative.capture_group = Some(group.clone());
            grouped.push(representative);
        }
    }
    grouped
}
