//! Mutation killers for `Hash for JournalDateTime` (survivor `journal_line.rs:124` `hash -> ()` of the GLUE-MUTANTS-2026-11-03
//! run). `Eq` compares the ticks only (the kind is ignored), so `Hash` must hash the ticks and must not depend on the kind.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use pr_core::file_actions::journal_line::JournalDateTime;

fn dt(text: &str) -> JournalDateTime {
    JournalDateTime::parse_json(text).expect("text parses")
}

fn hash_of(value: &JournalDateTime) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn equal_ticks_hash_equal_whatever_the_kind() {
    // Z => utc, no zone => unspecified: same ticks, `==` is true, so the hashes must be equal.
    assert_eq!(
        hash_of(&dt("2026-09-29T00:00:00Z")),
        hash_of(&dt("2026-09-29T00:00:00"))
    );
}

#[test]
fn different_ticks_hash_differently() {
    // A `hash` that writes nothing gives every value the same hash.
    let a = hash_of(&dt("2026-09-29T00:00:00Z"));
    let b = hash_of(&dt("2026-09-29T00:00:01Z"));
    let c = hash_of(&dt("2026-09-29T00:00:00.0000001Z"));
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
}

#[test]
fn hash_set_keeps_distinct_ticks_and_merges_equal_ones() {
    let mut set = HashSet::new();
    set.insert(dt("2026-09-29T00:00:00Z"));
    set.insert(dt("2026-09-29T00:00:00"));
    set.insert(dt("2026-09-29T00:00:01Z"));
    assert_eq!(set.len(), 2);
}
