//! P-393 (2026-11-06): runs `single_failure` with `mutation_completed` SET, through the
//! `FileActionService::with_after_mutation_fault` seam, to kill the N1 survivor `service.rs 683:13` (delete the field
//! `permanently_deleted: run.permanent && mutation_completed`). Production never raises that fault (no seam = no behaviour
//! change); the test only proves what the result reports when the file is gone for good and a later step fails.
//! No Recycle Bin is touched: the bin is `ScenarioBin` over the in-memory disk.

mod common_file_actions;

use std::io;
use std::sync::Arc;

use common_file_actions::{Fixture, single};
use pr_core::model::FileOperationType::Recycle;

const SOURCE: &str = r"C:\photos\a.jpg";

fn fixture_with_fault() -> Fixture {
    let mut fx = Fixture::new();
    fx.service = fx
        .service
        .with_after_mutation_fault(Arc::new(|| Err(io::Error::other("fault after the mutation"))));
    fx.add_file(SOURCE, 11);
    fx
}

/// A permanent delete that completed and then failed reports `permanently_deleted` (the file IS gone for good) next to
/// `succeeded` (the C# `mutationCompleted`), with no error text.
#[test]
fn a_permanent_delete_that_completed_before_the_failure_reports_permanently_deleted() {
    let fx = fixture_with_fault();
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run(&request);
    assert!(!fx.file_exists(SOURCE), "the permanent delete ran");
    assert!(
        result.succeeded,
        "mutation_completed is the success flag of single_failure"
    );
    assert!(result.error.is_none());
    assert!(result.permanently_deleted);
}

/// The same failure after an ordinary recycle is NOT a permanent delete (`run.permanent` false).
#[test]
fn a_recycle_that_completed_before_the_failure_does_not_report_permanently_deleted() {
    let fx = fixture_with_fault();
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert_eq!(fx.bin.recycled(), vec![SOURCE.to_owned()]);
    assert!(result.succeeded);
    assert!(!result.permanently_deleted);
}
