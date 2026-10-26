//! Port of `tests/PhotoReview.Core.Tests/Fakes/FaultInjectionFileSystem.cs` (C# baseline d92d6aae; decision P-14, D26(d)):
//! the reusable fault injector of the crash-consistency matrices (journal compaction, file actions, undo, recovery).
//!
//! It wraps a [`FakeFileSystem`] ("the disk") and numbers every MUTATING call (`create_directory`, `move_file`,
//! `copy_file` / `try_copy_new*`, `delete`, `open_append`, `write_all_text_atomic`, plus the external steps a fake bin
//! reports through [`FaultInjectionFileSystem::mutate`]). One [`FaultPlan`] picks the call and what happens there. A
//! crash kind kills the "process": that call and every later one fail with [`ProcessCrashed`], so whatever the code
//! under test does in its error paths cannot change the disk any more; a restart is a new journal/service over
//! [`disk`](FaultInjectionFileSystem::disk). A fail kind fails once and the process continues. The scenarios are
//! sequential (not meant for concurrent mutations).
//! W9-R3 also ports the same-folder atomic rename classification from this C# fake at 7eaee2a2.
//!
//! Rust signatures that cannot fail answer the "dead" question instead of throwing: after a crash `file_exists` and
//! `directory_exists` are `false` and `get_file_stat` is `None` (the C# methods throw); `try_get_file_stat`, the reads
//! and the listings return the [`ProcessCrashed`] error like C#. `try_delete_empty_directory` is forwarded and not
//! counted, exactly like C#.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{AppendStream, CopyCreationProof, FileStat, FileSystem, ReadSeek};
use pr_core::text::{path_eq_ignore_case, path_key};

use crate::FakeFileSystem;

/// What is injected at the chosen mutating call (C# `FaultKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FaultKind {
    /// The process dies right before the call: no effect, and every later call (reads too) fails.
    CrashBefore,
    /// The call takes full effect, then the process dies before it can see the result.
    CrashAfter,
    /// Copy/Move: a TRUNCATED destination is left (a copy cut short; the source is untouched), then the process dies.
    /// Append: half of the bytes reach the file, then the process dies.
    CrashPartial,
    /// Move only: the destination is complete but the source was not removed, then the process dies.
    CrashMoveCopiedNotDeleted,
    /// The call fails with an I/O error and has no effect; the process lives on (transient error).
    FailOnce,
    /// Like [`CrashPartial`](Self::CrashPartial) but the process survives (disk full mid-copy / mid-append).
    FailPartial,
    /// Like [`FailOnce`](Self::FailOnce) but `PermissionDenied` (C# `UnauthorizedAccessException`).
    FailUnauthorized,
    /// The call takes full effect, then the user's cancellation is requested through the cancel hook (no error from
    /// the call itself). Not one of the matrix's default kinds.
    CancelAfter,
}

impl FaultKind {
    /// Every kind, in C# declaration order.
    pub const ALL: [FaultKind; 8] = [
        FaultKind::CrashBefore,
        FaultKind::CrashAfter,
        FaultKind::CrashPartial,
        FaultKind::CrashMoveCopiedNotDeleted,
        FaultKind::FailOnce,
        FaultKind::FailPartial,
        FaultKind::FailUnauthorized,
        FaultKind::CancelAfter,
    ];

    /// C# `Applies(kind, mutationKind)`: whether this kind can be injected at a call of `mutation_kind` (`"mkdir"`,
    /// `"move"`, `"copy"`, `"delete"`, `"append"`, `"write"` or an external kind).
    pub fn applies(self, mutation_kind: &str) -> bool {
        match self {
            FaultKind::CrashPartial | FaultKind::FailPartial => {
                matches!(mutation_kind, "copy" | "move" | "append")
            }
            FaultKind::CrashMoveCopiedNotDeleted => mutation_kind == "move",
            // A crash after an append equals a crash before the next call (the call itself flushes nothing).
            FaultKind::CrashAfter => mutation_kind != "append",
            _ => true,
        }
    }
}

/// A fault at mutating call number `at` (1-based, counted since [`FaultInjectionFileSystem::arm`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultPlan {
    pub at: usize,
    pub kind: FaultKind,
}

/// The error of every call made by a "dead" process (C# `ProcessCrashedException`, an `IOException`). Carried inside an
/// `io::Error` of kind `Other`; test it with [`is_process_crash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessCrashed;

impl fmt::Display for ProcessCrashed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("simulated process death")
    }
}

impl std::error::Error for ProcessCrashed {}

/// True when `err` is the simulated process death.
pub fn is_process_crash(err: &io::Error) -> bool {
    err.get_ref().is_some_and(|inner| inner.is::<ProcessCrashed>())
}

fn crashed() -> io::Error {
    io::Error::other(ProcessCrashed)
}

/// The message of a partial write (C#: "There is not enough space on the disk.").
pub const DISK_FULL_MESSAGE: &str = "There is not enough space on the disk.";

/// A step of [`FaultInjectionFileSystem::mutate`].
pub type Step<'a> = &'a mut dyn FnMut() -> io::Result<()>;

/// Runs on the effect of a [`FaultKind::CancelAfter`] call (C# `CancelHook`).
pub type CancelHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    count: usize,
    plan: Option<FaultPlan>,
    dead: bool,
    kinds: Vec<String>,
    cancel_hook: Option<CancelHook>,
}

/// Runs `preferred` when given, else `fallback` (C# `(partial ?? effect)()`).
fn run_either(preferred: Option<Step<'_>>, fallback: Step<'_>) -> io::Result<()> {
    match preferred {
        Some(step) => step(),
        None => fallback(),
    }
}

/// See the module docs.
pub struct FaultInjectionFileSystem {
    disk: Arc<FakeFileSystem>,
    state: Mutex<State>,
}

impl fmt::Debug for FaultInjectionFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        f.debug_struct("FaultInjectionFileSystem")
            .field("count", &state.count)
            .field("plan", &state.plan)
            .field("dead", &state.dead)
            .finish_non_exhaustive()
    }
}

impl FaultInjectionFileSystem {
    pub fn new(disk: Arc<FakeFileSystem>) -> Self {
        Self {
            disk,
            state: Mutex::new(State::default()),
        }
    }

    /// The disk: survives a crash; a restart builds new services over it.
    pub fn disk(&self) -> &Arc<FakeFileSystem> {
        &self.disk
    }

    /// C# `CancelHook`: run right after the effect of a [`FaultKind::CancelAfter`] call.
    pub fn set_cancel_hook(&self, hook: Option<CancelHook>) {
        self.lock().cancel_hook = hook;
    }

    /// Starts counting from zero and installs the plan (`None` = a healthy run that only counts); revives the process.
    pub fn arm(&self, plan: Option<FaultPlan>) {
        let mut state = self.lock();
        state.plan = plan;
        state.count = 0;
        state.kinds.clear();
        state.dead = false;
    }

    /// True once a crash kind fired.
    pub fn dead(&self) -> bool {
        self.lock().dead
    }

    /// Kinds of the mutating calls since [`arm`](Self::arm), in call order (1-based position = injection point).
    pub fn kinds(&self) -> Vec<String> {
        self.lock().kinds.clone()
    }

    /// Mutating calls since [`arm`](Self::arm).
    pub fn mutations(&self) -> usize {
        self.lock().count
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn kill(&self) -> io::Error {
        self.lock().dead = true;
        crashed()
    }

    fn check_alive(&self) -> io::Result<()> {
        if self.lock().dead { Err(crashed()) } else { Ok(()) }
    }

    /// C# `Mutate`: runs one numbered mutation. `effect` is the full effect; `partial` what a cut-short call leaves
    /// (`None` = not injectable as partial: the full effect is used); `copied_not_deleted` the "copied but source kept"
    /// state of a move. The plan decides which runs and whether the process dies. Public so a fake bin can number its
    /// external steps. The lock is never held while a step or the cancel hook runs.
    pub fn mutate(
        &self,
        kind: &str,
        effect: Step<'_>,
        partial: Option<Step<'_>>,
        copied_not_deleted: Option<Step<'_>>,
    ) -> io::Result<()> {
        let fault = {
            let mut state = self.lock();
            if state.dead {
                return Err(crashed());
            }
            state.kinds.push(kind.to_owned());
            state.count += 1;
            let n = state.count;
            state.plan.filter(|plan| plan.at == n).map(|plan| plan.kind)
        };
        let Some(fault) = fault else {
            return effect();
        };
        match fault {
            FaultKind::CrashBefore => Err(self.kill()),
            FaultKind::FailOnce => Err(io::Error::other("simulated transient failure")),
            FaultKind::FailUnauthorized => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Access to the path is denied (simulated).",
            )),
            FaultKind::CancelAfter => {
                effect()?;
                let hook = self.lock().cancel_hook.clone();
                if let Some(hook) = hook {
                    hook();
                }
                Ok(())
            }
            FaultKind::CrashAfter => {
                effect()?;
                Err(self.kill())
            }
            FaultKind::CrashPartial => {
                run_either(partial, effect)?;
                Err(self.kill())
            }
            FaultKind::FailPartial => {
                run_either(partial, effect)?;
                Err(io::Error::other(DISK_FULL_MESSAGE))
            }
            FaultKind::CrashMoveCopiedNotDeleted => {
                run_either(copied_not_deleted, effect)?;
                Err(self.kill())
            }
        }
    }

    /// C# `Truncated`: a destination holding the first half of the source appears (copy phase cut short); nothing when
    /// the destination exists or the source is missing. Keeps the source's write time.
    fn truncated(&self, source: &Path, destination: &Path) -> io::Result<()> {
        if self.disk.file_exists(destination) {
            return Ok(());
        }
        let (Some(bytes), Some(stat)) = (self.disk.file_content(source), self.disk.peek_stat(source)) else {
            return Ok(());
        };
        self.disk
            .add_file_at(destination, &bytes[..bytes.len() / 2], stat.last_write_utc);
        Ok(())
    }

    /// C# `Snapshot`: every file directly in each of `folders` (missing folders skipped), path key => content.
    pub fn snapshot(&self, folders: &[&Path]) -> BTreeMap<String, Vec<u8>> {
        let mut result = BTreeMap::new();
        for folder in folders {
            if !self.disk.directory_exists(folder) {
                continue;
            }
            for file in self.disk.all_files() {
                if file.parent().is_some_and(|parent| path_eq_ignore_case(parent, folder))
                    && let Some(content) = self.disk.file_content(&file)
                {
                    result.insert(path_key(&file), content);
                }
            }
        }
        result
    }
}

impl FileSystem for FaultInjectionFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.check_alive().is_ok() && self.disk.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.check_alive().is_ok() && self.disk.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.try_get_file_stat(path).ok().flatten()
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        self.check_alive()?;
        self.disk.try_get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        // C# 7eaee2a2: a same-folder move is an atomic rename, so cross-volume partial/copy faults do not apply.
        let source_folder = pr_core::text::dotnet_path::get_directory_name(&source.to_string_lossy());
        let destination_folder = pr_core::text::dotnet_path::get_directory_name(&destination.to_string_lossy());
        let rename = match (source_folder, destination_folder) {
            (Some(source), Some(destination)) => pr_core::text::eq_ignore_case(&source, &destination),
            (None, None) => true,
            _ => false,
        };
        self.mutate(
            if rename { "rename" } else { "move" },
            &mut || self.disk.move_file(source, destination),
            Some(&mut || self.truncated(source, destination)),
            Some(&mut || self.disk.copy_file(source, destination)),
        )
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.mutate(
            "copy",
            &mut || self.disk.copy_file(source, destination),
            Some(&mut || self.truncated(source, destination)),
            None,
        )
    }

    /// One numbered "copy"; a partial copy is unclaimed (no proof), like `PhysicalFileSystem`.
    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        let created = Cell::new(false);
        self.mutate(
            "copy",
            &mut || {
                created.set(self.disk.try_copy_new_with_proof(source, destination, proof)?);
                Ok(())
            },
            Some(&mut || self.truncated(source, destination)),
            None,
        )?;
        Ok(created.get())
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.mutate("delete", &mut || self.disk.delete(path), None, None)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.check_alive()?;
        self.disk.open_read_shared(path, buffer_size)
    }

    /// One numbered "append". A partial kind opens the stream and then fails the call (C#: `Mutate` throws after the
    /// partial step), so the torn stream below is never handed out.
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        let stream: RefCell<Option<Box<dyn AppendStream>>> = RefCell::new(None);
        let torn = Cell::new(false);
        self.mutate(
            "append",
            &mut || {
                *stream.borrow_mut() = Some(self.disk.open_append(path, durable)?);
                Ok(())
            },
            Some(&mut || {
                *stream.borrow_mut() = Some(self.disk.open_append(path, durable)?);
                torn.set(true);
                Ok(())
            }),
            None,
        )?;
        let stream = stream
            .into_inner()
            .ok_or_else(|| io::Error::other("append stream was not opened"))?;
        Ok(if torn.get() {
            Box::new(TornStream { inner: stream })
        } else {
            stream
        })
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.mutate(
            "write",
            &mut || self.disk.write_all_text_atomic(path, text, durable),
            None,
            None,
        )
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.check_alive()?;
        self.disk.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.check_alive()?;
        self.disk.enumerate_files(directory, pattern)
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.check_alive().map_err(|err| err.to_string())?;
        self.disk.try_probe_readable(path)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.check_alive()?;
        self.disk.enumerate_directories(directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.mutate("mkdir", &mut || self.disk.create_directory(path), None, None)
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        self.disk.try_delete_empty_directory(path)
    }

    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.check_alive()?;
        self.disk.resolve_real_path(path)
    }
}

/// C# `TornStream`: writes the first half of the buffer, flushes it to the "disk", then fails like a full disk / power
/// loss. Not a real file (`as_file_mut` is `None`). Kept for parity: as in C# (baseline d92d6aae), `mutate` fails a
/// partial `open_append` AFTER the partial open, so no caller receives this stream today (the matrix's "half of the
/// bytes" case is in fact "opened, nothing written"); pinned by `partial_append_opens_the_file_then_fails_without_writing`.
struct TornStream {
    inner: Box<dyn AppendStream>,
}

impl Write for TornStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write_all(&buf[..buf.len() / 2])?;
        self.inner.flush()?;
        Err(io::Error::other(DISK_FULL_MESSAGE))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl AppendStream for TornStream {}
