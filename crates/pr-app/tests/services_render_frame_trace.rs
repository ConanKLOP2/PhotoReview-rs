//! Tests of `pr_app::services::render_frame_trace` (contract `contracts/W8-04c.md`, rules R-RF-1..6, traps T-6).
//!
//! Port of `RenderFrameTraceTests.cs`. The C# `Handlers` list becomes the set of subscribed ids that the host keeps (`apply_steps`).
//! The tests assert exact step vectors (T-6), never sets of steps.

use std::collections::BTreeSet;

use pr_app::services::render_frame_trace::{FrameTraceStep, HandlerId, RenderFrameTrace};

/// The host side of the frame-tick source: applies `Subscribe`/`Unsubscribe` to its subscribed-id list.
fn apply_steps(subscribed: &mut BTreeSet<HandlerId>, steps: &[FrameTraceStep]) {
    for step in steps {
        match step {
            FrameTraceStep::Subscribe(id) => {
                subscribed.insert(*id);
            }
            FrameTraceStep::Unsubscribe(id) => {
                subscribed.remove(id);
            }
            FrameTraceStep::FirstFrame { .. } | FrameTraceStep::SecondFrame { .. } => {}
        }
    }
}

/// The id a `start` call subscribed (its `Subscribe` step is the last one).
fn subscribed_id(steps: &[FrameTraceStep]) -> HandlerId {
    match steps.last() {
        Some(FrameTraceStep::Subscribe(id)) => *id,
        other => panic!("start must end with Subscribe, got {other:?}"),
    }
}

// C#: RenderFrameTraceTests.Start_FiveTimesWithoutAnyTick_KeepsOneSubscription
// A minimised window never renders: handlers must not pile up.
#[test]
fn five_starts_without_tick_keep_one_subscription() {
    let mut trace = RenderFrameTrace::new();
    let mut subscribed = BTreeSet::new();

    for i in 0..5 {
        let steps = trace.start(i, "preview", 0);
        apply_steps(&mut subscribed, &steps);
        assert_eq!(subscribed.len(), 1, "after start {i}");
    }
}

// C#: RenderFrameTraceTests.Tick_AfterSecondStart_ReportsOnlyTheLatestTrace
#[test]
fn tick_after_second_start_reports_only_the_latest_trace() {
    let mut trace = RenderFrameTrace::new();
    let mut subscribed = BTreeSet::new();

    let first_steps = trace.start(1, "preview", 0);
    apply_steps(&mut subscribed, &first_steps);
    let first = subscribed_id(&first_steps);

    let second_steps = trace.start(2, "preview", 0);
    assert_eq!(
        second_steps[0],
        FrameTraceStep::Unsubscribe(first),
        "the superseded trace is unsubscribed first"
    );
    apply_steps(&mut subscribed, &second_steps);
    let second = subscribed_id(&second_steps);
    assert_ne!(first, second, "a new trace gets a new id");

    assert!(
        trace.on_frame_tick(first).is_empty(),
        "ticks of the superseded id report nothing"
    );
    assert_eq!(
        trace.on_frame_tick(second),
        vec![FrameTraceStep::FirstFrame {
            token: 2,
            kind: "preview".to_owned(),
            assigned_timestamp: 0,
        }]
    );
    assert_eq!(
        trace.on_frame_tick(second),
        vec![
            FrameTraceStep::Unsubscribe(second),
            FrameTraceStep::SecondFrame {
                token: 2,
                assigned_timestamp: 0
            }
        ]
    );
    apply_steps(&mut subscribed, &[FrameTraceStep::Unsubscribe(second)]);
    assert!(
        subscribed.is_empty(),
        "no handler stays subscribed after the second frame"
    );
}

// C#: RenderFrameTraceTests.Cancel_WhilePending_UnsubscribesWithoutReporting
#[test]
fn cancel_while_pending_unsubscribes_without_reporting() {
    let mut trace = RenderFrameTrace::new();
    let start_steps = trace.start(1, "preview", 0);
    let id = subscribed_id(&start_steps);

    assert_eq!(trace.cancel(), vec![FrameTraceStep::Unsubscribe(id)]);
    assert!(trace.on_frame_tick(id).is_empty(), "a cancelled trace reports nothing");
}

// Contract test (R-RF-3): with nothing pending, cancel returns nothing.
#[test]
fn cancel_without_pending_is_empty() {
    let mut trace = RenderFrameTrace::new();
    assert!(trace.cancel().is_empty());
}

// Contract test (R-RF-4): a tick for a handler that is not the pending one changes nothing.
#[test]
fn stale_handler_tick_is_ignored() {
    let mut trace = RenderFrameTrace::new();
    let first = subscribed_id(&trace.start(1, "preview", 0));
    let second = subscribed_id(&trace.start(2, "preview", 0));
    assert_ne!(first, second);

    assert!(trace.on_frame_tick(first).is_empty());
    assert_eq!(
        trace.on_frame_tick(second),
        vec![FrameTraceStep::FirstFrame {
            token: 2,
            kind: "preview".to_owned(),
            assigned_timestamp: 0,
        }],
        "the stale tick did not consume the first frame of the pending trace"
    );
}

// Contract test (R-RF-5): the second tick clears the pending trace, so a third tick reports nothing.
#[test]
fn second_tick_clears_pending() {
    let mut trace = RenderFrameTrace::new();
    let id = subscribed_id(&trace.start(7, "original", 42));

    assert_eq!(
        trace.on_frame_tick(id),
        vec![FrameTraceStep::FirstFrame {
            token: 7,
            kind: "original".to_owned(),
            assigned_timestamp: 42,
        }]
    );
    assert_eq!(
        trace.on_frame_tick(id),
        vec![
            FrameTraceStep::Unsubscribe(id),
            FrameTraceStep::SecondFrame {
                token: 7,
                assigned_timestamp: 42
            }
        ]
    );
    assert!(trace.on_frame_tick(id).is_empty(), "a third tick reports nothing");
    assert!(trace.cancel().is_empty(), "nothing is pending after the second frame");
}
