//! Killer tests found by the `mut-newfixes` mutation run (docs/reviews/MUT-NEWFIXES-2026-10-31.md): the re-stat that
//! guards a Recycle between the Prepared line and the mutation (L3 of ST-INVMAP-2026-10-31, #463) must refuse a
//! replacement that differs in EITHER the length OR the write time. The existing tests replace the file with one that
//! differs in both, so `||` -> `&&` survived. No sleeps; in-memory disk and bin; nothing real is touched.

mod common_file_actions;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common_file_actions::{Fixture, journal_path, last_write_plus, single};
use pr_core::model::FileOperationType::Recycle;

const SOURCE: &str = r"C:\photos\a.jpg";

/// Replaces `SOURCE` with `bytes` stamped `plus_seconds` after the original write time when the Prepared line is appended.
fn replace_source_on_first_append(fx: &Fixture, bytes: &'static [u8], plus_seconds: i64) {
    let disk = fx.disk.clone();
    let done = Arc::new(AtomicUsize::new(0));
    let journal = journal_path();
    fx.hooks.set_open_append_hook(Some(Arc::new(move |path: &Path| {
        if path.to_string_lossy().eq_ignore_ascii_case(&journal) && done.fetch_add(1, Ordering::SeqCst) == 0 {
            disk.add_file_at(Path::new(SOURCE), bytes, last_write_plus(plus_seconds));
        }
        None
    })));
}

/// Runs a single Recycle (bin or permanent) whose source is swapped after Prepared and asserts nothing was touched.
fn assert_swapped_source_is_refused(bytes: &'static [u8], plus_seconds: i64, permanent: bool) {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11); // 11 bytes, original write time (plus 0)
    if permanent {
        fx.bin.set_no_bin_roots(&[r"C:\"]);
    }
    replace_source_on_first_append(&fx, bytes, plus_seconds);
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = permanent;
    let result = fx.run(&request);
    assert!(!result.succeeded, "a swapped source must be refused");
    assert!(fx.bin.recycled().is_empty(), "the replacement must not be recycled");
    assert!(
        fx.bin.permanently_deleted().is_empty(),
        "the replacement must not be deleted"
    );
    assert!(fx.file_exists(SOURCE), "the replacement must stay on disk");
}

// Same length, other write time: the length check alone cannot see it (kills `||` -> `&&` at service.rs:592).
#[test]
fn recycle_source_swapped_for_same_length_other_write_time_is_refused() {
    assert_swapped_source_is_refused(b"hello photo", 500, false);
    assert_swapped_source_is_refused(b"hello photo", 500, true);
}

// Other length, same write time: the write-time check alone cannot see it.
#[test]
fn recycle_source_swapped_for_other_length_same_write_time_is_refused() {
    assert_swapped_source_is_refused(b"a different, longer replacement", 0, false);
    assert_swapped_source_is_refused(b"a different, longer replacement", 0, true);
}
