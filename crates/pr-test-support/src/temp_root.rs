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
        let path = long_temp_dir()
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

/// `%TEMP%` in its long form. CI runners spell it with an 8.3 short name (`...\Users\RUNNER~1\...`); .NET
/// `Path.GetFullPath` (and so the ported code) expands a `~` name and normalizes the case of what exists, which would
/// make a test's expected text differ from the code's output for a reason unrelated to the code under test.
fn long_temp_dir() -> PathBuf {
    let temp = std::env::temp_dir();
    match std::fs::canonicalize(&temp) {
        Ok(canonical) => {
            let text = canonical.to_string_lossy().into_owned();
            // `canonicalize` returns the verbatim form `\\?\C:\...` on Windows; plain drive paths only, not `\\?\UNC\`.
            match text.strip_prefix(r"\\?\") {
                Some(plain) if !plain.starts_with(r"UNC\") => PathBuf::from(plain),
                _ => canonical,
            }
        }
        Err(_) => temp,
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
