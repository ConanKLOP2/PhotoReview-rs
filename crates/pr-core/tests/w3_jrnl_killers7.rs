//! Journal mutation guards (CL-JRNL3), pure values, no storage and no clock.
//! Oracle: src/PhotoReview.Core/FileActions/OperationJournal.cs at C# 7eaee2a2.

use pr_core::file_actions::journal::JournalCompactionOutcome;

// Mutants journal.rs:117:9 as_str -> "" and -> "xyzzy".
// C#: `Outcome.ToString()` is the variant name, used in the startup log line.
#[test]
fn compaction_outcome_as_str_is_the_csharp_variant_name() {
    let expected = [
        (JournalCompactionOutcome::Compacted, "Compacted"),
        (JournalCompactionOutcome::NotSupported, "NotSupported"),
        (JournalCompactionOutcome::BelowThreshold, "BelowThreshold"),
        (JournalCompactionOutcome::NotWorthIt, "NotWorthIt"),
        (JournalCompactionOutcome::Busy, "Busy"),
        (JournalCompactionOutcome::Changed, "Changed"),
        (JournalCompactionOutcome::Failed, "Failed"),
    ];
    for (outcome, name) in expected {
        assert_eq!(outcome.as_str(), name);
    }
}
