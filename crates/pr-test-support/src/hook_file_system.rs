//! Port of the fault-injection hooks of the C# test fake `tests/PhotoReview.Core.Tests/Fakes/InMemoryFileSystem.cs`
//! (baseline d92d6aae): `OpenReadHook`, `OpenAppendHook`, `WriteHook`, `StatHook`, `DeleteHook`, `DeleteDirectoryHook`,
//! `EnumerateFilesHook`, `MoveHook`, `CopyHook`, `CopyPartialHook`, `MoveLeavesSource`, `StampOnMove`, `CopyFailsAfterBytes`,
//! `AddReparsePoint` + `ResolveRealPathHook` and the `Events` trace (decision P-14, D26(d); contracts W3-04/05/06 "Test fakes").
//!
//! Storage, journal streams and the Windows semantics come from the wrapped [`FakeFileSystem`] (the "disk"): what is not
//! hooked is forwarded to it unchanged, so its counters and injected faults keep working. Each hook runs at the START of
//! its call, outside every lock (a hook may block, or call back into this file system, e.g. to read the journal), and a
//! returned error is the call's result before anything changed. A C# hook returning `OperationCanceledException` returns
//! `io::Error::other(pr_core::sync::Canceled)` here.
//!
//! Where a C# hook throws from a method whose Rust signature cannot fail, the Rust result is the "cannot" answer:
//! `stat_hook` makes `get_file_stat` `None` (and `try_get_file_stat` `Err`), `delete_directory_hook` makes
//! `try_delete_empty_directory` `false`.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use pr_core::abstractions::{AppendStream, Clock, CopyCreationProof, FileStat, FileSystem, ReadSeek};
use pr_core::text::{dotnet_path, path_key};
use pr_core::time::DateTimeUtc;

use crate::FakeFileSystem;

/// A hook on one path: `Some(err)` fails the call with `err`.
pub type PathHook = Arc<dyn Fn(&Path) -> Option<io::Error> + Send + Sync>;
/// A hook on (source, destination).
pub type PairHook = Arc<dyn Fn(&Path, &Path) -> Option<io::Error> + Send + Sync>;
/// C# `CopyPartialHook`: `Some((n, err))` = the copy creates the destination with the first `n` source bytes, then fails
/// with `err` WITHOUT raising the creation proof (D-01 (c): a partial copy is never claimed).
pub type CopyPartialHook = Arc<dyn Fn(&Path, &Path) -> Option<(usize, io::Error)> + Send + Sync>;
/// C# `StampOnMove`: (destination, source write time) => the write time the destination volume keeps (RV-C01).
pub type StampOnMove = Arc<dyn Fn(&Path, DateTimeUtc) -> DateTimeUtc + Send + Sync>;

/// One entry of the C# `Events` trace (IO03 ordering/thread tests): `kind` is `"move"` or `"append"`. Compare threads
/// by `thread` (a Rust `ThreadId` is never reused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEvent {
    pub kind: &'static str,
    pub durable: bool,
    pub path: PathBuf,
    pub thread: ThreadId,
}

#[derive(Default)]
struct Hooks {
    open_read: Option<PathHook>,
    open_append: Option<PathHook>,
    write: Option<PathHook>,
    stat: Option<PathHook>,
    delete: Option<PathHook>,
    delete_directory: Option<PathHook>,
    enumerate_files: Option<PathHook>,
    resolve_real_path: Option<PathHook>,
    move_hook: Option<PairHook>,
    copy: Option<PairHook>,
    copy_partial: Option<CopyPartialHook>,
    move_leaves_source: bool,
    stamp_on_move: Option<StampOnMove>,
    copy_fails_after_bytes: Option<usize>,
    /// path key of the link => its simulated real target.
    reparse_points: HashMap<String, PathBuf>,
    events: Vec<FsEvent>,
}

/// See the module docs. Share it through `Arc` like the fake it wraps.
pub struct HookFileSystem {
    disk: Arc<FakeFileSystem>,
    hooks: Mutex<Hooks>,
}

impl fmt::Debug for HookFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookFileSystem")
            .field("disk", &self.disk)
            .finish_non_exhaustive()
    }
}

macro_rules! setter {
    ($(#[$doc:meta])* $name:ident, $field:ident, $ty:ty) => {
        $(#[$doc])*
        pub fn $name(&self, hook: Option<$ty>) {
            self.lock().$field = hook;
        }
    };
}

impl HookFileSystem {
    pub fn new(disk: Arc<FakeFileSystem>) -> Self {
        Self {
            disk,
            hooks: Mutex::new(Hooks::default()),
        }
    }

    /// The wrapped fake: "by hand" changes go here and never fire a hook.
    pub fn disk(&self) -> &Arc<FakeFileSystem> {
        &self.disk
    }

    setter!(
        /// C# `OpenReadHook`: `open_read_shared`, `read_all_text` and `try_probe_readable`.
        set_open_read_hook, open_read, PathHook
    );
    setter!(
        /// C# `OpenAppendHook` (after the event is recorded).
        set_open_append_hook, open_append, PathHook
    );
    setter!(
        /// C# `WriteHook`: `write_all_text_atomic`.
        set_write_hook, write, PathHook
    );
    setter!(
        /// C# `StatHook`: `get_file_stat` / `try_get_file_stat` of a non-blank path.
        set_stat_hook, stat, PathHook
    );
    setter!(
        /// C# `DeleteHook`.
        set_delete_hook, delete, PathHook
    );
    setter!(
        /// C# `DeleteDirectoryHook`: `try_delete_empty_directory` answers `false`.
        set_delete_directory_hook, delete_directory, PathHook
    );
    setter!(
        /// C# `EnumerateFilesHook`.
        set_enumerate_files_hook, enumerate_files, PathHook
    );
    setter!(
        /// C# `ResolveRealPathHook` (RV-S06: a reparse loop that cannot be resolved).
        set_resolve_real_path_hook, resolve_real_path, PathHook
    );
    setter!(
        /// C# `MoveHook` (after the event is recorded).
        set_move_hook, move_hook, PairHook
    );
    setter!(
        /// C# `CopyHook`: `copy_file` and `try_copy_new*`.
        set_copy_hook, copy, PairHook
    );
    setter!(
        /// C# `CopyPartialHook`.
        set_copy_partial_hook, copy_partial, CopyPartialHook
    );
    setter!(
        /// C# `StampOnMove`.
        set_stamp_on_move, stamp_on_move, StampOnMove
    );

    /// C# `MoveLeavesSource`: a move copies the file and leaves the source (cross-volume move that could not delete).
    /// The disk counts it as an [`FsOp::Copy`](crate::FsOp::Copy).
    pub fn set_move_leaves_source(&self, value: bool) {
        self.lock().move_leaves_source = value;
    }

    /// C# `CopyFailsAfterBytes` (RV-C03): every copy writes only the first `n` bytes, then fails (no creation proof).
    pub fn set_copy_fails_after_bytes(&self, value: Option<usize>) {
        self.lock().copy_fails_after_bytes = value;
    }

    /// C# `AddReparsePoint` (SEC-01): `path` becomes an existing folder entry whose real target is `target`; nothing
    /// is created on the real disk.
    pub fn add_reparse_point(&self, path: impl AsRef<Path>, target: impl AsRef<Path>) {
        let path = path.as_ref();
        self.disk.add_dir(path);
        self.lock()
            .reparse_points
            .insert(dir_key(path), trim_dir(target.as_ref()));
    }

    /// C# `Events`, in call order.
    pub fn events(&self) -> Vec<FsEvent> {
        self.lock().events.clone()
    }

    pub fn clear_events(&self) {
        self.lock().events.clear();
    }

    fn lock(&self) -> MutexGuard<'_, Hooks> {
        self.hooks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(&self, kind: &'static str, path: &Path, durable: bool) {
        self.lock().events.push(FsEvent {
            kind,
            durable,
            path: path.to_path_buf(),
            thread: thread::current().id(),
        });
    }

    /// Runs the path hook `pick` chooses, without holding the lock.
    fn path_hook(&self, pick: impl Fn(&Hooks) -> Option<PathHook>, path: &Path) -> io::Result<()> {
        let hook = pick(&self.lock());
        match hook.and_then(|hook| hook(path)) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn pair_hook(
        &self,
        pick: impl Fn(&Hooks) -> Option<PairHook>,
        source: &Path,
        destination: &Path,
    ) -> io::Result<()> {
        let hook = pick(&self.lock());
        match hook.and_then(|hook| hook(source, destination)) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// C# `CopyCore`: hook, then (only when a partial copy is configured) the checks of the copy and the partial write;
    /// everything else is the disk's copy.
    fn copy_core(&self, source: &Path, destination: &Path, proof: Option<&CopyCreationProof>) -> io::Result<bool> {
        self.pair_hook(|h| h.copy.clone(), source, destination)?;
        let (partial_hook, fails_after) = {
            let hooks = self.lock();
            (hooks.copy_partial.clone(), hooks.copy_fails_after_bytes)
        };
        if partial_hook.is_some() || fails_after.is_some() {
            let bytes = self.disk.file_content(source).ok_or_else(|| not_found(source))?;
            if self.disk.file_exists(destination) {
                return match proof {
                    Some(_) => Ok(false),
                    None => Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("exists: {}", destination.display()),
                    )),
                };
            }
            if let Some(parent) = destination.parent().filter(|p| p.parent().is_some())
                && !self.disk.directory_exists(parent)
            {
                return Err(not_found(parent));
            }
            let partial = partial_hook
                .and_then(|hook| hook(source, destination))
                .or_else(|| fails_after.map(|n| (n, io::Error::other("Simulated disk full during copy."))));
            if let Some((written, err)) = partial {
                let now = self.disk.clock().utc_now();
                self.disk
                    .add_file_at(destination, &bytes[..written.min(bytes.len())], now);
                return Err(err);
            }
        }
        match proof {
            Some(proof) => self.disk.try_copy_new_with_proof(source, destination, proof),
            None => self.disk.copy_file(source, destination).map(|()| true),
        }
    }
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("not found: {}", path.display()))
}

fn is_blank(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().trim().is_empty()
}

fn trim_dir(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(dotnet_path::trim_ending_directory_separator(&text))
}

fn dir_key(path: &Path) -> String {
    path_key(&trim_dir(path))
}

impl FileSystem for HookFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.disk.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.disk.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.try_get_file_stat(path).ok().flatten()
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        // C#: ThrowIfNullOrWhiteSpace first (the disk answers it), then StatHook.
        if !is_blank(path) {
            self.path_hook(|h| h.stat.clone(), path)?;
        }
        self.disk.try_get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.record("move", source, false);
        self.pair_hook(|h| h.move_hook.clone(), source, destination)?;
        let (leaves_source, stamp) = {
            let hooks = self.lock();
            (hooks.move_leaves_source, hooks.stamp_on_move.clone())
        };
        let original = self.disk.peek_stat(source);
        if leaves_source {
            self.disk.copy_file(source, destination)?;
        } else {
            self.disk.move_file(source, destination)?;
        }
        if let (Some(stamp), Some(original)) = (stamp, original) {
            self.disk
                .set_last_write(destination, stamp(destination, original.last_write_utc));
        }
        Ok(())
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.copy_core(source, destination, None).map(|_| ())
    }

    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        self.copy_core(source, destination, Some(proof))
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.path_hook(|h| h.delete.clone(), path)?;
        self.disk.delete(path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.path_hook(|h| h.open_read.clone(), path)?;
        self.disk.open_read_shared(path, buffer_size)
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.record("append", path, durable);
        self.path_hook(|h| h.open_append.clone(), path)?;
        self.disk.open_append(path, durable)
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.path_hook(|h| h.write.clone(), path)?;
        self.disk.write_all_text_atomic(path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.path_hook(|h| h.open_read.clone(), path)?;
        self.disk.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.path_hook(|h| h.enumerate_files.clone(), directory)?;
        self.disk.enumerate_files(directory, pattern)
    }

    fn get_stored_file_names(&self, path: &Path) -> io::Result<Vec<String>> {
        self.disk.get_stored_file_names(path)
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.path_hook(|h| h.open_read.clone(), path)
            .map_err(|err| err.to_string())?;
        self.disk.try_probe_readable(path)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.disk.enumerate_directories(directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.disk.create_directory(path)
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        if self.path_hook(|h| h.delete_directory.clone(), path).is_err() {
            return false;
        }
        let deleted = self.disk.try_delete_empty_directory(path);
        if deleted {
            self.lock().reparse_points.remove(&dir_key(path));
        }
        deleted
    }

    /// C# `ResolveRealPath`: walks the full path segment by segment and substitutes every segment registered with
    /// [`add_reparse_point`](HookFileSystem::add_reparse_point) by its target, like `PhysicalFileSystem` does for a real
    /// link. A bare root comes back with its separator.
    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.path_hook(|h| h.resolve_real_path.clone(), path)?;
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The path is not valid Unicode."))?;
        let full = dotnet_path::get_full_path(text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
        let root = &full[..dotnet_path::root_length(&full)];
        if root.is_empty() {
            return Ok(PathBuf::from(full));
        }
        let segments: Vec<&str> = full[root.len()..]
            .split(['\\', '/'])
            .filter(|s| !s.is_empty())
            .collect();
        if segments.is_empty() {
            return Ok(PathBuf::from(root));
        }
        let mut current = root.trim_end_matches(['\\', '/']).to_owned();
        let hooks = self.lock();
        for segment in segments {
            let candidate = if current.is_empty() {
                segment.to_owned()
            } else {
                format!("{current}\\{segment}")
            };
            current = match hooks.reparse_points.get(&dir_key(Path::new(&candidate))) {
                Some(target) => target.to_string_lossy().into_owned(),
                None => candidate,
            };
        }
        Ok(PathBuf::from(current))
    }
}
