//! W8-17e crash handling (P-15), pure half. Contract: `contracts/W8-17e.md`.
use pr_app::startup::crash::*;
use pr_app::startup::options::MAX_LOOP_FAULTS;

#[test]
fn loop_fault_counter_gives_up_at_the_fiftieth_fault() {
    // new: C# `if (++faults >= MaxLoopFaults)`.
    let mut counter = LoopFaultCounter::new();
    for i in 1..MAX_LOOP_FAULTS {
        assert_eq!(record_default(&mut counter), LoopVerdict::Continue, "fault {i}");
    }
    assert_eq!(counter.faults(), MAX_LOOP_FAULTS - 1);
    assert_eq!(record_default(&mut counter), LoopVerdict::GiveUp);
    assert_eq!(counter.faults(), MAX_LOOP_FAULTS);
}

#[test]
fn loop_fault_counter_with_a_limit_of_one_gives_up_at_once() {
    // new: boundary of `faults >= max`.
    let mut counter = LoopFaultCounter::new();
    assert_eq!(counter.record(1), LoopVerdict::GiveUp);
}

#[test]
fn loop_fault_counter_counts_faults_even_with_a_zero_limit() {
    // new: `faults += 1` happens before the comparison.
    let mut counter = LoopFaultCounter::new();
    assert_eq!(counter.record(0), LoopVerdict::GiveUp);
    assert_eq!(counter.faults(), 1);
}

#[test]
fn describe_payload_handles_str_string_and_other_payloads() {
    // new: C# `exceptionObject?.ToString() ?? "unknown exception object"`.
    let s: Box<dyn std::any::Any + Send> = Box::new("static text");
    let owned: Box<dyn std::any::Any + Send> = Box::new(String::from("owned text"));
    let other: Box<dyn std::any::Any + Send> = Box::new(42_u32);
    assert_eq!(describe_payload(&*s), "static text");
    assert_eq!(describe_payload(&*owned), "owned text");
    assert_eq!(describe_payload(&*other), "unknown exception object");
}

#[test]
fn unhandled_error_displays_its_text() {
    // new: the `Error` wrapper handed to `error_forced`.
    let error = UnhandledError("boom".to_owned());
    assert_eq!(error.to_string(), "boom");
    let dynamic: &dyn std::error::Error = &error;
    assert_eq!(dynamic.to_string(), "boom");
}
