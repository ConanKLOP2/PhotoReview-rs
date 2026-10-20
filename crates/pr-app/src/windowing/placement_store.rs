//! Reading and writing `window-placement.json`, byte-compatible with the C# app.
//!
//! Port of `PhotoReview.App.Shared/Windowing/JsonWindowPlacementStore.cs` and `WindowPlacementJsonContext.cs` (baseline
//! d92d6aae). Contract: `contracts/W8-03a.md`. The file lives under `%LOCALAPPDATA%\PhotoReviewRs` (R0002): this module never
//! builds a path, callers pass `AppPaths::window_placement_file`; it never reads the C# app's `window-placement.json`.
//!
//! The reader is `System.Text.Json` with DEFAULT options (strict: no comments, no trailing commas, no numbers in strings,
//! names case-sensitive, depth 64, last duplicate wins, every occurrence of a typed property must convert), which is NOT the
//! lenient dialect of `config.json`; see the contract for how it is built on `pr_core::settings::app_settings::json`.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use pr_core::abstractions::FileSystem;
use pr_core::settings::app_settings::json::{self, JsonValue};
use pr_core::text::dotnet_path::get_directory_name;
use pr_core::text::eq_ignore_case;

use super::contracts::{PrefetchHandle, SHOW_MAXIMIZED, WindowPlacementData, WindowPlacementStore};
use super::monitor_layout::MonitorLayout;
use super::visibility;

/// Why [`JsonWindowPlacementStore::read_raw`] or [`JsonWindowPlacementStore::deserialize`] failed. C# catches
/// `IOException`, `UnauthorizedAccessException`, `JsonException` and `NotSupportedException` in `Read`/`Prefetch`; every
/// `io::Error` is the first two, [`PlacementReadError::Json`] stands for every `JsonException`.
#[derive(Debug)]
pub enum PlacementReadError {
    Io(io::Error),
    /// Syntax error, depth above 64, a typed value that cannot be converted, or a root that is neither an object nor `null`.
    /// Only the fact matters; the text is for logs.
    Json(String),
}

impl fmt::Display for PlacementReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "window placement file could not be read: {e}"),
            Self::Json(m) => write!(f, "window placement file is damaged: {m}"),
        }
    }
}

impl Error for PlacementReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(_) => None,
        }
    }
}

fn json_err(message: &str) -> PlacementReadError {
    PlacementReadError::Json(message.to_owned())
}

/// What `System.Text.Json` with default options rejects but `json::parse` (the `config.json` dialect) accepts:
/// comments and trailing commas outside strings.
fn strict_precheck(text: &str) -> Result<(), PlacementReadError> {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                while let Some(s) = chars.next() {
                    match s {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' => return Err(json_err("comments are not allowed")),
            ',' => {
                let next = chars.clone().find(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'));
                if matches!(next, Some('}' | ']')) {
                    return Err(json_err("trailing commas are not allowed"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A typed `int` property: only an integer number token that fits `i32` (a string, bool, null... is a conversion error).
fn int_value(value: &JsonValue) -> Result<i32, PlacementReadError> {
    match value {
        JsonValue::Number(_) => json::to_i32(value).ok_or_else(|| json_err("number does not fit an int")),
        _ => Err(json_err("expected a number")),
    }
}

/// Reads a position object into `out` (one slot per name; own defaults are whatever `out` holds, unknown names are ignored,
/// every occurrence is converted).
fn read_position(value: &JsonValue, names: &[&str], out: &mut [i32]) -> Result<(), PlacementReadError> {
    let JsonValue::Object(entries) = value else {
        return Err(json_err("expected an object"));
    };
    for (name, v) in entries {
        let name = name
            .as_deref()
            .ok_or_else(|| json_err("unpaired surrogate in a property name"))?;
        if let Some(i) = names.iter().position(|n| *n == name)
            && let Some(slot) = out.get_mut(i)
        {
            *slot = int_value(v)?;
        }
    }
    Ok(())
}

/// The prefetch slot: the path and a cell the worker thread fills when the read SUCCEEDED (a failed read leaves it empty).
#[derive(Debug)]
struct PrefetchSlot {
    path: String,
    outcome: Arc<OnceLock<Option<WindowPlacementData>>>,
}

/// `JsonWindowPlacementStore`: one shared instance per app holds the prefetch state.
pub struct JsonWindowPlacementStore {
    fs: Arc<dyn FileSystem>,
    layout: Arc<dyn MonitorLayout>,
    prefetch: Mutex<Option<PrefetchSlot>>,
}

impl fmt::Debug for JsonWindowPlacementStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonWindowPlacementStore").finish_non_exhaustive()
    }
}

impl JsonWindowPlacementStore {
    /// C# has a parameterless constructor that uses `Win32MonitorLayout.Instance`; here the composition root passes the layout
    /// (the real one from `pr_platform_win`, or `FixedMonitorLayout` in tests) and the file system.
    pub fn new(fs: Arc<dyn FileSystem>, layout: Arc<dyn MonitorLayout>) -> Self {
        Self {
            fs,
            layout,
            prefetch: Mutex::new(None),
        }
    }

    /// `ReadRaw`: the saved placement, NOT filtered by visibility. `Ok(None)` when the path is not an existing file or the
    /// document is `null`; `Err` for an unreadable file or a damaged document (so callers can log it). The text is decoded like
    /// `File.ReadAllText` (UTF-8 default; UTF-8/UTF-16/UTF-32 byte-order marks honoured, which `FileSystem::read_all_text` does).
    pub fn read_raw(
        fs: &dyn FileSystem,
        placement_path: &str,
    ) -> Result<Option<WindowPlacementData>, PlacementReadError> {
        let path = Path::new(placement_path);
        if !fs.file_exists(path) {
            return Ok(None);
        }
        let text = fs.read_all_text(path).map_err(PlacementReadError::Io)?;
        Self::deserialize(&text)
    }

    /// `IsVisible(WindowPlacementData)`: the Normal rect of `data` is visible on at least one current work area
    /// (`visibility::is_visible` over `layout.work_areas()`).
    pub fn is_visible(&self, data: &WindowPlacementData) -> bool {
        visibility::is_visible(
            data.normal_left,
            data.normal_top,
            data.normal_right,
            data.normal_bottom,
            &self.layout.work_areas(),
        )
    }

    /// `Serialize`: the exact text C# writes: 2-space indent, `\r\n` line ends, no trailing newline, properties in the order
    /// `Length, Flags, ShowCommand, MinPosition{X,Y}, MaxPosition{X,Y}, NormalPosition{Left,Top,Right,Bottom}`.
    pub fn serialize(data: &WindowPlacementData) -> String {
        let mut s = String::new();
        s.push_str("{\r\n");
        s.push_str(&format!("  \"Length\": {},\r\n", data.length));
        s.push_str(&format!("  \"Flags\": {},\r\n", data.flags));
        s.push_str(&format!("  \"ShowCommand\": {},\r\n", data.show_command));
        s.push_str(&format!(
            "  \"MinPosition\": {{\r\n    \"X\": {},\r\n    \"Y\": {}\r\n  }},\r\n",
            data.min_x, data.min_y
        ));
        s.push_str(&format!(
            "  \"MaxPosition\": {{\r\n    \"X\": {},\r\n    \"Y\": {}\r\n  }},\r\n",
            data.max_x, data.max_y
        ));
        s.push_str(&format!(
            "  \"NormalPosition\": {{\r\n    \"Left\": {},\r\n    \"Top\": {},\r\n    \"Right\": {},\r\n    \"Bottom\": {}\r\n  }}\r\n}}",
            data.normal_left, data.normal_top, data.normal_right, data.normal_bottom
        ));
        s
    }

    /// `Deserialize`: `Ok(None)` for the document `null`; a missing `ShowCommand` is 3 (`SHOW_MAXIMIZED`), every other
    /// missing value is 0. See the contract for the exact accept/reject rules.
    pub fn deserialize(json_text: &str) -> Result<Option<WindowPlacementData>, PlacementReadError> {
        strict_precheck(json_text)?;
        let root = json::parse(json_text).map_err(|e| PlacementReadError::Json(e.to_string()))?;
        let entries = match root {
            JsonValue::Null => return Ok(None),
            JsonValue::Object(entries) => entries,
            _ => return Err(json_err("the root must be an object or null")),
        };
        let (mut length, mut flags, mut show_command) = (0, 0, SHOW_MAXIMIZED);
        let mut min = [0; 2];
        let mut max = [0; 2];
        let mut normal = [0; 4];
        for (name, value) in &entries {
            let name = name
                .as_deref()
                .ok_or_else(|| json_err("unpaired surrogate in a property name"))?;
            match name {
                "Length" => length = int_value(value)?,
                "Flags" => flags = int_value(value)?,
                "ShowCommand" => show_command = int_value(value)?,
                // A repeated position replaces the earlier one as a whole.
                "MinPosition" => {
                    min = [0; 2];
                    read_position(value, &["X", "Y"], &mut min)?;
                }
                "MaxPosition" => {
                    max = [0; 2];
                    read_position(value, &["X", "Y"], &mut max)?;
                }
                "NormalPosition" => {
                    normal = [0; 4];
                    read_position(value, &["Left", "Top", "Right", "Bottom"], &mut normal)?;
                }
                _ => {}
            }
        }
        Ok(Some(WindowPlacementData {
            length,
            flags,
            show_command,
            min_x: min[0],
            min_y: min[1],
            max_x: max[0],
            max_y: max[1],
            normal_left: normal[0],
            normal_top: normal[1],
            normal_right: normal[2],
            normal_bottom: normal[3],
        }))
    }

    /// `WriteAtomically(path, content)`: writes `content` to `<path>.<32 hex digits>.tmp` and replaces `path` with it; on any
    /// failure the temp file is deleted (best effort) and the original error is returned. Uses
    /// `FileSystem::write_all_text_atomic(path, content, durable = false)`, which is that algorithm (contract, note 19).
    pub fn write_atomically(fs: &dyn FileSystem, path: &str, content: &str) -> io::Result<()> {
        fs.write_all_text_atomic(Path::new(path), content, false)
    }

    fn slot(&self) -> MutexGuard<'_, Option<PrefetchSlot>> {
        // A poisoned lock only means a panic elsewhere; the slot data is still consistent.
        self.prefetch.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl WindowPlacementStore for JsonWindowPlacementStore {
    fn prefetch(&self, placement_path: &str) -> PrefetchHandle {
        let outcome: Arc<OnceLock<Option<WindowPlacementData>>> = Arc::new(OnceLock::new());
        *self.slot() = Some(PrefetchSlot {
            path: placement_path.to_owned(),
            outcome: Arc::clone(&outcome),
        });
        let fs = Arc::clone(&self.fs);
        let path = placement_path.to_owned();
        let spawned = std::thread::Builder::new()
            .name("window-placement-prefetch".to_owned())
            .spawn(move || match Self::read_raw(fs.as_ref(), &path) {
                Ok(value) => {
                    let _ = outcome.set(value);
                    value
                }
                Err(_) => None,
            });
        PrefetchHandle { handle: spawned.ok() }
    }

    fn read(&self, placement_path: &str) -> Option<WindowPlacementData> {
        match Self::read_raw(self.fs.as_ref(), placement_path) {
            Ok(Some(data)) if self.is_visible(&data) => Some(data),
            _ => None,
        }
    }

    fn save(&self, placement_path: &str, data: &WindowPlacementData) -> io::Result<()> {
        let dir = match get_directory_name(placement_path) {
            Some(dir) if !dir.is_empty() => dir,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the placement path has no directory part",
                ));
            }
        };
        self.fs.create_directory(Path::new(&dir))?;
        Self::write_atomically(self.fs.as_ref(), placement_path, &Self::serialize(data))
    }

    fn take_prefetched(&self, placement_path: &str) -> Option<WindowPlacementData> {
        let slot = {
            let mut guard = self.slot();
            match guard.as_ref() {
                Some(s) if eq_ignore_case(&s.path, placement_path) => guard.take(),
                _ => None,
            }
        }?;
        // Non-blocking: a read that has not finished or failed leaves the cell empty.
        slot.outcome.get().copied().flatten()
    }
}
