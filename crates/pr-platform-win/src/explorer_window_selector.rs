//! Port of `PhotoReview.Platform.Windows/Explorer/ExplorerWindowSelector.cs` (C# baseline d92d6aae).
//!
//! Selection policy is independent of live COM. The caller adapts the W6-01a window and lazy enumeration
//! abstractions to [`ExplorerSelectionWindow`], which lets this module's tests use deterministic fakes.

use std::error::Error;
use std::fmt;
use std::time::Instant;

use pr_core::abstractions::Log;
use pr_core::catalog::explorer_reason;
use pr_core::catalog::explorer_types::{ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::catalog::snapshot_validator::{canonicalize_folder, same_path};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;

/// Error categories that the C# selector treats specially while reading `LocationURL`.
#[derive(Debug)]
pub(crate) enum LocationReadError {
    /// A window closed or does not expose the browser interface: skip without logging.
    Unavailable,
    /// An unexpected error: log and continue.
    Other(Box<dyn Error + Send + Sync>),
}

/// A single yielded window. Implementations must release their owned window reference in `release`.
pub(crate) trait ExplorerSelectionWindow {
    fn location_url(&self) -> Result<Option<String>, LocationReadError>;
    fn read(&self, folder: &str, cancellation: &CancelToken) -> Result<ExplorerViewSnapshot, SelectionReadError>;
    fn release(&self);
}

/// Error raised by the native-view read callback, retaining the exception type and HRESULT from C#.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectionReadError {
    pub(crate) type_name: String,
    pub(crate) hresult: i32,
    pub(crate) canceled: bool,
}

impl fmt::Display for SelectionReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (HRESULT 0x{:08X})", self.type_name, self.hresult as u32)
    }
}

impl Error for SelectionReadError {}

/// Walks windows lazily, releasing each yielded value exactly once.
pub(crate) fn select<I, W>(folder: &str, windows: I, log: &dyn Log, cancellation: &CancelToken) -> ExplorerViewSnapshot
where
    I: IntoIterator<Item = Result<W, Box<dyn Error + Send + Sync>>>,
    W: ExplorerSelectionWindow,
{
    let started = Instant::now();
    let mut inspected = 0usize;
    let mut first_failure = None;
    let mut windows = windows.into_iter();
    loop {
        let window = match windows.next() {
            None => break,
            Some(Ok(window)) => window,
            Some(Err(error)) => {
                log.error(
                    &format!("Explorer window enumeration failed after {inspected} window(s)"),
                    Some(error.as_ref()),
                );
                break;
            }
        };
        let result = (|| {
            inspected += 1;
            if cancellation.is_canceled() {
                return Some(unavailable(
                    folder,
                    ExplorerOrderStatus::Canceled,
                    explorer_reason::CANCELED,
                ));
            }
            let location = match window.location_url() {
                Ok(location) => location,
                Err(LocationReadError::Unavailable) => return None,
                Err(LocationReadError::Other(error)) => {
                    log.error(
                        &format!("Explorer window inspection failed after {inspected} window(s)"),
                        Some(error.as_ref()),
                    );
                    return None;
                }
            };
            let location = location?;
            let current = try_canonicalize_location(&location)?;
            if !same_path(&current, folder).unwrap_or(false) {
                return None;
            }

            let snapshot = match window.read(folder, cancellation) {
                Ok(snapshot) => snapshot,
                Err(error) if error.canceled => {
                    return Some(unavailable(
                        folder,
                        ExplorerOrderStatus::Canceled,
                        explorer_reason::CANCELED,
                    ));
                }
                Err(error) => unavailable(
                    folder,
                    ExplorerOrderStatus::Failed,
                    &format!(
                        "{}|{}|0x{:08X}",
                        explorer_reason::NATIVE_VIEW_FAILED,
                        error.type_name,
                        error.hresult as u32
                    ),
                ),
            };
            if matches!(
                snapshot.status,
                ExplorerOrderStatus::Available | ExplorerOrderStatus::Canceled
            ) {
                return Some(snapshot);
            }
            if first_failure.is_none() {
                first_failure = Some(snapshot);
            }
            None
        })();
        window.release();
        if let Some(snapshot) = result {
            return snapshot;
        }
    }
    if let Some(snapshot) = first_failure {
        return snapshot;
    }
    let count = inspected.to_string();
    let unavailable = unavailable(
        folder,
        ExplorerOrderStatus::NoMatchingWindow,
        &format!("{}|{count}", explorer_reason::NO_MATCHING_WINDOW),
    );
    log.info(&format!(
        "Explorer query-complete: status={:?}, windows={inspected}, elapsedMs={}",
        unavailable.status,
        started.elapsed().as_millis()
    ));
    unavailable
}

fn unavailable(folder: &str, status: ExplorerOrderStatus, reason: &str) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: Vec::new(),
        sort_columns: Vec::new(),
        group_state: pr_core::catalog::explorer_types::ExplorerGroupState::Unknown,
        status,
        reason: Some(reason.to_owned()),
        captured_utc: DateTimeUtc::now(),
    }
}

/// Canonicalizes an absolute file URL using URI local-path percent decoding and the shared path validator.
pub(crate) fn try_canonicalize_location(location: &str) -> Option<String> {
    if location.trim().is_empty() {
        return None;
    }
    let rest = location
        .get(..5)
        .filter(|prefix| prefix.eq_ignore_ascii_case("file:"))
        .map(|_| &location[5..])?;
    if rest.is_empty() {
        return None;
    }
    let (host, path) = if let Some(authority_path) = rest.strip_prefix("//") {
        let end = authority_path.find(['/', '?', '#']).unwrap_or(authority_path.len());
        let authority = &authority_path[..end];
        let path = &authority_path[end..];
        if authority.contains('@') || authority.contains(':') {
            return None;
        }
        (
            if authority.is_empty() || authority.eq_ignore_ascii_case("localhost") {
                None
            } else {
                Some(authority)
            },
            path,
        )
    } else {
        (None, rest)
    };
    let path = path.split(['?', '#']).next()?;
    let decoded = percent_decode(path)?;
    let local = if let Some(host) = host {
        format!(r"\\{host}{}", decoded.replace('/', r"\"))
    } else if decoded.starts_with('/') && decoded.as_bytes().get(2) == Some(&b':') {
        decoded[1..].replace('/', r"\")
    } else {
        decoded.replace('/', r"\")
    };
    canonicalize_folder(&local).ok()
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}
