//! Port of `tests/PhotoReview.TestSupport/TempRoot.cs`: a unique scratch folder removed when dropped. Real-OS tests
//! (`#[ignore = "native"]`) work only inside one, so they clean up after themselves.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    /// Creates `%TEMP%\pr-rs-tests\<pid>-<n>-<label>`.
    ///
    /// # Panics
    /// When the folder cannot be created.
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join("pr-rs-tests")
            .join(format!("{}-{n}-{label}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.path.join(relative)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
