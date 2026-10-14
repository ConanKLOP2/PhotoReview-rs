//! In-memory [`FileSystem`] with Windows semantics: case-insensitive paths (OrdinalIgnoreCase), no overwrite on
//! move/copy, deleting a missing file succeeds, listings sorted like NTFS. Faults can be injected per operation and
//! path, and every operation is counted so tests can assert "no extra disk read" (AGENTS.md priority 1).

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{Clock, FileStat, FileSystem, ReadSeek};
use pr_core::text::{cmp_ignore_case, path_key};
use pr_core::time::DateTimeUtc;

use crate::FakeClock;

/// Operations that can be counted and failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FsOp {
    Stat,
    Move,
    Copy,
    Delete,
    OpenRead,
    OpenAppend,
    WriteAtomic,
    ReadText,
    EnumerateFiles,
    EnumerateDirectories,
    CreateDirectory,
}

#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    data: Vec<u8>,
    last_write: DateTimeUtc,
}

#[derive(Debug, Clone)]
struct Fault {
    op: FsOp,
    key: String,
    raw_os_error: Option<i32>,
    kind: io::ErrorKind,
    remaining: Option<usize>,
}

#[derive(Debug, Default)]
struct State {
    files: HashMap<String, Entry>,
    dirs: BTreeMap<String, PathBuf>,
    unreadable: HashMap<String, String>,
    faults: Vec<Fault>,
    counts: HashMap<FsOp, usize>,
}

/// See the module docs. Cheap to share: wrap in `Arc` and hand clones to the code under test.
#[derive(Debug)]
pub struct FakeFileSystem {
    state: Arc<Mutex<State>>,
    clock: Arc<FakeClock>,
}

/// Windows "sharing violation" (ERROR_SHARING_VIOLATION), the error a locked file gives.
pub const SHARING_VIOLATION: i32 = 32;

impl FakeFileSystem {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(FakeClock::new()))
    }

    /// New and modified files get `clock`'s time as last write.
    pub fn with_clock(clock: Arc<FakeClock>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            clock,
        }
    }

    pub fn clock(&self) -> &Arc<FakeClock> {
        &self.clock
    }

    /// Adds (or replaces) a file, creating its parent folders; last write = clock now.
    pub fn add_file(&self, path: impl AsRef<Path>, data: &[u8]) {
        let now = self.clock.utc_now();
        self.add_file_at(path, data, now);
    }

    /// Adds (or replaces) a file with an explicit last write time, creating its parent folders.
    pub fn add_file_at(&self, path: impl AsRef<Path>, data: &[u8], last_write: DateTimeUtc) {
        let path = path.as_ref();
        let mut state = self.lock();
        if let Some(parent) = path.parent() {
            add_dirs(&mut state, parent);
        }
        state.files.insert(
            key(path),
            Entry {
                path: path.to_path_buf(),
                data: data.to_vec(),
                last_write,
            },
        );
    }

    /// Adds a folder and its parents.
    pub fn add_dir(&self, path: impl AsRef<Path>) {
        add_dirs(&mut self.lock(), path.as_ref());
    }

    pub fn file_content(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        self.lock().files.get(&key(path.as_ref())).map(|e| e.data.clone())
    }

    pub fn set_last_write(&self, path: impl AsRef<Path>, value: DateTimeUtc) {
        if let Some(e) = self.lock().files.get_mut(&key(path.as_ref())) {
            e.last_write = value;
        }
    }

    /// Every file path, sorted like NTFS.
    pub fn all_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.lock().files.values().map(|e| e.path.clone()).collect();
        sort_like_ntfs(&mut files);
        files
    }

    /// `try_probe_readable` reports `reason` for `path` (a locked or access-denied file).
    pub fn set_unreadable(&self, path: impl AsRef<Path>, reason: &str) {
        self.lock().unreadable.insert(key(path.as_ref()), reason.to_owned());
    }

    /// Every `op` on `path` fails with `kind` until [`clear_faults`](Self::clear_faults).
    pub fn fail(&self, op: FsOp, path: impl AsRef<Path>, kind: io::ErrorKind) {
        self.push_fault(op, path.as_ref(), kind, None, None);
    }

    /// The next `times` calls of `op` on `path` fail with `kind`.
    pub fn fail_times(&self, op: FsOp, path: impl AsRef<Path>, kind: io::ErrorKind, times: usize) {
        self.push_fault(op, path.as_ref(), kind, None, Some(times));
    }

    /// Every `op` on `path` fails with a sharing violation (a file locked by another process).
    pub fn fail_locked(&self, op: FsOp, path: impl AsRef<Path>) {
        self.push_fault(op, path.as_ref(), io::ErrorKind::Other, Some(SHARING_VIOLATION), None);
    }

    pub fn clear_faults(&self) {
        self.lock().faults.clear();
    }

    /// How many times `op` was called (failed calls included).
    pub fn count(&self, op: FsOp) -> usize {
        self.lock().counts.get(&op).copied().unwrap_or(0)
    }

    pub fn reset_counts(&self) {
        self.lock().counts.clear();
    }

    /// Removes a file and returns its bytes and stat (used by `FakeRecycleBin`).
    pub(crate) fn take_file(&self, path: &Path) -> io::Result<(Vec<u8>, FileStat)> {
        let mut state = self.lock();
        let entry = state.files.remove(&key(path)).ok_or_else(|| not_found(path))?;
        let stat = FileStat {
            length: entry.data.len() as u64,
            last_write_utc: entry.last_write,
        };
        Ok((entry.data, stat))
    }

    fn push_fault(&self, op: FsOp, path: &Path, kind: io::ErrorKind, raw: Option<i32>, remaining: Option<usize>) {
        self.lock().faults.push(Fault {
            op,
            key: key(path),
            raw_os_error: raw,
            kind,
            remaining,
        });
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts the call and returns the injected fault for (`op`, `path`), if any.
    fn begin(&self, op: FsOp, path: &Path) -> io::Result<MutexGuard<'_, State>> {
        let mut state = self.lock();
        *state.counts.entry(op).or_insert(0) += 1;
        let k = key(path);
        if let Some(i) = state.faults.iter().position(|f| f.op == op && f.key == k) {
            let fault = state.faults[i].clone();
            if let Some(n) = fault.remaining {
                if n <= 1 {
                    state.faults.remove(i);
                } else {
                    state.faults[i].remaining = Some(n - 1);
                }
            }
            return Err(match fault.raw_os_error {
                Some(code) => io::Error::from_raw_os_error(code),
                None => io::Error::new(fault.kind, format!("injected {op:?} fault on {}", path.display())),
            });
        }
        Ok(state)
    }
}

impl Default for FakeFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

fn key(path: &Path) -> String {
    let k = path_key(path);
    let trimmed = k.trim_end_matches(['\\', '/']);
    // Keep "C:\" a root rather than turning it into the drive-relative "C:".
    if trimmed.ends_with(':') { k } else { trimmed.to_owned() }
}

fn parent_key(path: &Path) -> Option<String> {
    path.parent().filter(|p| !p.as_os_str().is_empty()).map(key)
}

fn add_dirs(state: &mut State, dir: &Path) {
    for ancestor in dir.ancestors().filter(|a| !a.as_os_str().is_empty()) {
        state
            .dirs
            .entry(key(ancestor))
            .or_insert_with(|| ancestor.to_path_buf());
    }
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("not found: {}", path.display()))
}

fn require_parent(state: &State, path: &Path) -> io::Result<()> {
    match parent_key(path) {
        Some(parent) if !state.dirs.contains_key(&parent) => Err(not_found(path.parent().unwrap_or(path))),
        _ => Ok(()),
    }
}

fn sort_like_ntfs(paths: &mut [PathBuf]) {
    paths.sort_by(|a, b| cmp_ignore_case(&a.to_string_lossy(), &b.to_string_lossy()));
}

/// .NET search pattern: `*` any run, `?` one char, case-insensitive. (`*.*` matches every name, as in .NET.)
fn matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "*.*" {
        return true;
    }
    fn go(n: &[char], p: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(&n[i..], rest)),
            Some(('?', rest)) => !n.is_empty() && go(&n[1..], rest),
            Some((c, rest)) => {
                n.first()
                    .is_some_and(|x| cmp_ignore_case(&x.to_string(), &c.to_string()).is_eq())
                    && go(&n[1..], rest)
            }
        }
    }
    let n: Vec<char> = name.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    go(&n, &p)
}

impl FileSystem for FakeFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.lock().files.contains_key(&key(path))
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.lock().dirs.contains_key(&key(path))
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        let state = self.begin(FsOp::Stat, path).ok()?;
        state.files.get(&key(path)).map(|e| FileStat {
            length: e.data.len() as u64,
            last_write_utc: e.last_write,
        })
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        let mut state = self.begin(FsOp::Move, source)?;
        if !state.files.contains_key(&key(source)) {
            return Err(not_found(source));
        }
        if state.files.contains_key(&key(destination)) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("exists: {}", destination.display()),
            ));
        }
        require_parent(&state, destination)?;
        let mut entry = state.files.remove(&key(source)).ok_or_else(|| not_found(source))?;
        entry.path = destination.to_path_buf();
        state.files.insert(key(destination), entry);
        Ok(())
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        let mut state = self.begin(FsOp::Copy, source)?;
        let mut entry = state
            .files
            .get(&key(source))
            .cloned()
            .ok_or_else(|| not_found(source))?;
        if state.files.contains_key(&key(destination)) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("exists: {}", destination.display()),
            ));
        }
        require_parent(&state, destination)?;
        // File.Copy keeps the source's last write time.
        entry.path = destination.to_path_buf();
        state.files.insert(key(destination), entry);
        Ok(())
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        let mut state = self.begin(FsOp::Delete, path)?;
        state.files.remove(&key(path));
        Ok(())
    }

    fn open_read_shared(&self, path: &Path, _buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        let state = self.begin(FsOp::OpenRead, path)?;
        let data = state
            .files
            .get(&key(path))
            .map(|e| e.data.clone())
            .ok_or_else(|| not_found(path))?;
        Ok(Box::new(Cursor::new(data)))
    }

    /// Creates the file when missing (FileMode.Append). Each `write` lands immediately (no buffering), stamped with
    /// the clock's time; a file deleted while the appender is open is recreated by the next write.
    fn open_append(&self, path: &Path, _durable: bool) -> io::Result<Box<dyn Write + Send>> {
        let mut state = self.begin(FsOp::OpenAppend, path)?;
        require_parent(&state, path)?;
        let now = self.clock.utc_now();
        state.files.entry(key(path)).or_insert_with(|| Entry {
            path: path.to_path_buf(),
            data: Vec::new(),
            last_write: now,
        });
        Ok(Box::new(Appender {
            state: Arc::clone(&self.state),
            clock: Arc::clone(&self.clock),
            path: path.to_path_buf(),
        }))
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, _durable: bool) -> io::Result<()> {
        let mut state = self.begin(FsOp::WriteAtomic, path)?;
        require_parent(&state, path)?;
        let now = self.clock.utc_now();
        state.files.insert(
            key(path),
            Entry {
                path: path.to_path_buf(),
                data: text.as_bytes().to_vec(),
                last_write: now,
            },
        );
        Ok(())
    }

    /// UTF-8, a leading BOM removed (as File.ReadAllText does).
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        let state = self.begin(FsOp::ReadText, path)?;
        let data = state
            .files
            .get(&key(path))
            .map(|e| e.data.clone())
            .ok_or_else(|| not_found(path))?;
        let body = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data);
        String::from_utf8(body.to_vec()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        let state = self.begin(FsOp::EnumerateFiles, directory)?;
        let dir = key(directory);
        if !state.dirs.contains_key(&dir) {
            return Err(not_found(directory));
        }
        let mut found: Vec<PathBuf> = state
            .files
            .values()
            .filter(|e| parent_key(&e.path).as_deref() == Some(dir.as_str()))
            .filter(|e| {
                e.path
                    .file_name()
                    .is_some_and(|n| matches_pattern(&n.to_string_lossy(), pattern))
            })
            .map(|e| e.path.clone())
            .collect();
        sort_like_ntfs(&mut found);
        Ok(found)
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        match self.lock().unreadable.get(&key(path)) {
            Some(reason) => Err(reason.clone()),
            None => Ok(()),
        }
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        let state = self.begin(FsOp::EnumerateDirectories, directory)?;
        let dir = key(directory);
        if !state.dirs.contains_key(&dir) {
            return Err(not_found(directory));
        }
        let mut found: Vec<PathBuf> = state
            .dirs
            .values()
            .filter(|p| parent_key(p).as_deref() == Some(dir.as_str()) && key(p) != dir)
            .cloned()
            .collect();
        sort_like_ntfs(&mut found);
        Ok(found)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        let mut state = self.begin(FsOp::CreateDirectory, path)?;
        if state.files.contains_key(&key(path)) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("a file exists: {}", path.display()),
            ));
        }
        add_dirs(&mut state, path);
        Ok(())
    }
}

struct Appender {
    state: Arc<Mutex<State>>,
    clock: Arc<FakeClock>,
    path: PathBuf,
}

impl Write for Appender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let now = self.clock.utc_now();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = state.files.entry(key(&self.path)).or_insert_with(|| Entry {
            path: self.path.clone(),
            data: Vec::new(),
            last_write: now,
        });
        entry.data.extend_from_slice(buf);
        entry.last_write = now;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn paths_are_case_insensitive() {
        let fs = FakeFileSystem::new();
        fs.add_file(r"C:\Photos\A.jpg", b"x");
        assert!(fs.file_exists(&p(r"c:\photos\a.JPG")));
        assert!(fs.directory_exists(&p(r"C:\PHOTOS\")));
    }

    #[test]
    fn move_refuses_to_overwrite_and_needs_the_target_folder() {
        let fs = FakeFileSystem::new();
        fs.add_file(r"C:\a\x.jpg", b"1");
        fs.add_file(r"C:\a\y.jpg", b"2");
        let err = fs.move_file(&p(r"C:\a\x.jpg"), &p(r"C:\a\Y.JPG")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let err = fs.move_file(&p(r"C:\a\x.jpg"), &p(r"C:\missing\x.jpg")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs.add_dir(r"C:\b");
        fs.move_file(&p(r"C:\a\x.jpg"), &p(r"C:\b\x.jpg")).unwrap();
        assert_eq!(fs.file_content(r"C:\b\x.jpg").as_deref(), Some(&b"1"[..]));
        assert!(!fs.file_exists(&p(r"C:\a\x.jpg")));
    }

    #[test]
    fn delete_of_missing_file_succeeds() {
        assert!(FakeFileSystem::new().delete(&p(r"C:\nothing.jpg")).is_ok());
    }

    #[test]
    fn copy_keeps_the_source_last_write_time() {
        let fs = FakeFileSystem::new();
        fs.add_file(r"C:\a\x.jpg", b"1");
        let before = fs.get_file_stat(&p(r"C:\a\x.jpg")).unwrap();
        fs.clock().advance_seconds(60);
        fs.copy_file(&p(r"C:\a\x.jpg"), &p(r"C:\a\copy.jpg")).unwrap();
        assert_eq!(fs.get_file_stat(&p(r"C:\a\copy.jpg")), Some(before));
    }

    #[test]
    fn enumerate_files_lists_direct_children_sorted_and_filtered() {
        let fs = FakeFileSystem::new();
        fs.add_file(r"C:\a\b.JPG", b"");
        fs.add_file(r"C:\a\A.jpg", b"");
        fs.add_file(r"C:\a\c.png", b"");
        fs.add_file(r"C:\a\sub\d.jpg", b"");
        let all = fs.enumerate_files(&p(r"C:\a"), "*").unwrap();
        assert_eq!(all, vec![p(r"C:\a\A.jpg"), p(r"C:\a\b.JPG"), p(r"C:\a\c.png")]);
        assert_eq!(fs.enumerate_files(&p(r"C:\a"), "*.jpg").unwrap().len(), 2);
        assert_eq!(fs.enumerate_directories(&p(r"C:\a")).unwrap(), vec![p(r"C:\a\sub")]);
    }

    #[test]
    fn injected_faults_fail_the_right_call_and_are_counted() {
        let fs = FakeFileSystem::new();
        fs.add_file(r"C:\a\x.jpg", b"1");
        fs.fail_times(FsOp::OpenRead, r"C:\A\X.JPG", io::ErrorKind::PermissionDenied, 1);
        assert_eq!(
            fs.open_read_shared(&p(r"C:\a\x.jpg"), 4096).err().map(|e| e.kind()),
            Some(io::ErrorKind::PermissionDenied)
        );
        assert!(fs.open_read_shared(&p(r"C:\a\x.jpg"), 4096).is_ok());
        fs.fail_locked(FsOp::Delete, r"C:\a\x.jpg");
        assert_eq!(
            fs.delete(&p(r"C:\a\x.jpg")).unwrap_err().raw_os_error(),
            Some(SHARING_VIOLATION)
        );
        assert_eq!(fs.count(FsOp::OpenRead), 2);
    }

    #[test]
    fn appender_writes_land_immediately() {
        let fs = FakeFileSystem::new();
        fs.add_dir(r"C:\data");
        let mut w = fs.open_append(&p(r"C:\data\j.jsonl"), true).unwrap();
        w.write_all(b"a\n").unwrap();
        w.write_all(b"b\n").unwrap();
        assert_eq!(fs.read_all_text(&p(r"C:\data\j.jsonl")).unwrap(), "a\nb\n");
    }

    #[test]
    fn readable_listing_skips_and_reports_unreadable_files() {
        let fs = FakeFileSystem::new();
        fs.add_file(r"C:\a\ok.jpg", b"");
        fs.add_file(r"C:\a\locked.jpg", b"");
        fs.set_unreadable(r"C:\a\locked.jpg", "locked");
        let mut skipped = Vec::new();
        let listed = fs
            .enumerate_readable_files_with_stat(&p(r"C:\a"), &|_| true, &mut |e| skipped.push(e))
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].reason, "locked");
    }
}
