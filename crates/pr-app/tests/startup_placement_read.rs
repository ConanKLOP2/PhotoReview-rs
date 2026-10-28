//! W8-17e `read_saved_placement` (C# `ReadPlacement`), pure. Contract: `contracts/W8-17e.md`.
use pr_app::startup::placement_read::read_saved_placement;
use pr_app::windowing::contracts::{PrefetchHandle, WindowPlacementData, WindowPlacementStore};
use pr_app::windowing::placement_store::JsonWindowPlacementStore;
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog, TempRoot};
use std::io;
use std::sync::Mutex;

fn data(left: i32) -> WindowPlacementData {
    WindowPlacementData {
        length: 44,
        flags: 0,
        show_command: 1,
        min_x: -1,
        min_y: -1,
        max_x: -1,
        max_y: -1,
        normal_left: left,
        normal_top: 20,
        normal_right: left + 800,
        normal_bottom: 620,
    }
}

/// Only `take_prefetched` is used by the function under test.
struct FakeStore {
    prefetched: Mutex<Option<WindowPlacementData>>,
    takes: Mutex<usize>,
}

impl FakeStore {
    fn new(prefetched: Option<WindowPlacementData>) -> Self {
        Self {
            prefetched: Mutex::new(prefetched),
            takes: Mutex::new(0),
        }
    }
}

impl WindowPlacementStore for FakeStore {
    fn prefetch(&self, _placement_path: &str) -> PrefetchHandle {
        unreachable!("not used")
    }
    fn read(&self, _placement_path: &str) -> Option<WindowPlacementData> {
        unreachable!("not used")
    }
    fn save(&self, _placement_path: &str, _data: &WindowPlacementData) -> io::Result<()> {
        unreachable!("not used")
    }
    fn take_prefetched(&self, _placement_path: &str) -> Option<WindowPlacementData> {
        *self.takes.lock().unwrap() += 1;
        self.prefetched.lock().unwrap().take()
    }
}

#[test]
fn read_saved_placement_prefers_the_prefetched_value_then_the_file_and_logs_a_damaged_file() {
    // new: C# `ReadPlacement`: `prefetched ?? read`, a damaged file is logged and means no placement.
    let root = TempRoot::new("placement-read");
    let file = root.join("window.json").to_string_lossy().into_owned();
    let fs = FakeFileSystem::new();
    let log = RecordingLog::new();

    // 1. A prefetched value wins and the file is never read.
    let store = FakeStore::new(Some(data(10)));
    assert_eq!(read_saved_placement(&store, &fs, &file, &log), Some(data(10)));
    assert_eq!(*store.takes.lock().unwrap(), 1);

    // 2. Nothing prefetched, no file: no placement, no log.
    assert_eq!(read_saved_placement(&store, &fs, &file, &log), None);
    assert!(log.entries().is_empty());

    // 3. Nothing prefetched, a valid file: read directly (the value is not filtered by visibility).
    fs.add_file(&file, JsonWindowPlacementStore::serialize(&data(30)).as_bytes());
    assert_eq!(read_saved_placement(&store, &fs, &file, &log), Some(data(30)));
    assert!(log.entries().is_empty());

    // 4. A damaged file: logged, answer None.
    fs.add_file(&file, b"{ not json");
    assert_eq!(read_saved_placement(&store, &fs, &file, &log), None);
    assert!(log.contains(LogLevel::Error, "Could not read the window placement"));
}
