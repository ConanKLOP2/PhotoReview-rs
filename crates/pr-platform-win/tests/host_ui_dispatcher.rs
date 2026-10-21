//! W8-13d, the pure half (kind F): `UiQueues` (ordering and counts) and `WakeFlag` (the flag protocol). No window, no thread but the test's own.
//! The real dispatcher on a real UI thread is in `host_ui_dispatcher_native.rs`.
#![cfg(windows)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use pr_core::abstractions::UiPriority;
use pr_platform_win::host::ui_dispatcher::{UiQueues, WakeFlag};

/// Small deterministic generator (xorshift64*), the role of C# `new Random(seed)` in `InterleavedPriorities`.
fn interleaved_priorities(count: usize, seed: u64) -> Vec<UiPriority> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..count)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let value = state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;
            match value % 4 {
                0 => UiPriority::Send,
                1 => UiPriority::Normal,
                2 => UiPriority::Render,
                _ => UiPriority::Background,
            }
        })
        .collect()
}

#[test]
fn ui_queues_dequeue_high_orders_send_normal_render_fifo() {
    // C#: Win32UiDispatcherTests.Priorities_ThousandInterleavedPostsFromUiThread_RunSendNormalRenderBackgroundEachInFifoOrder (the queue half)
    let posted = interleaved_priorities(1000, 7);
    let mut queues = UiQueues::new();
    for (index, priority) in posted.iter().enumerate() {
        queues.enqueue(*priority, (*priority, index));
    }

    let mut order = Vec::new();
    while let Some(item) = queues.dequeue_high() {
        order.push(item);
    }
    while let Some(item) = queues.dequeue_background() {
        order.push(item);
    }

    let mut expected: Vec<(UiPriority, usize)> = posted.iter().copied().zip(0..).collect();
    expected.sort_by_key(|(priority, index)| (*priority as usize, *index));
    assert_eq!(order.len(), 1000);
    assert_eq!(order, expected);
    let distinct: std::collections::BTreeSet<usize> = order.iter().map(|(p, _)| *p as usize).collect();
    assert_eq!(distinct.len(), 4);
}

#[test]
fn ui_queues_background_never_comes_out_while_high_work_waits() {
    let mut queues = UiQueues::new();
    queues.enqueue(UiPriority::Background, "bg");
    assert_eq!(queues.dequeue_background(), Some("bg"), "no high work: Background runs");
    queues.enqueue(UiPriority::Background, "bg2");
    queues.enqueue(UiPriority::Render, "render");

    assert_eq!(queues.dequeue_background(), None, "high work waits");
    assert!(queues.has_background());
    assert_eq!(queues.dequeue_high(), Some("render"));
    assert_eq!(queues.dequeue_background(), Some("bg2"));
    assert_eq!(queues.dequeue_background(), None);
    assert!(!queues.has_background());
}

#[test]
fn ui_queues_high_count_tracks_enqueue_and_dequeue() {
    let mut queues = UiQueues::new();
    assert!(!queues.has_high());
    queues.enqueue(UiPriority::Background, 0);
    assert!(!queues.has_high(), "a Background enqueue does not raise the high count");
    queues.enqueue(UiPriority::Normal, 1);
    queues.enqueue(UiPriority::Send, 2);
    assert!(queues.has_high());
    assert_eq!(queues.dequeue_high(), Some(2));
    assert!(queues.has_high());
    assert_eq!(queues.dequeue_high(), Some(1));
    assert!(!queues.has_high());
    assert_eq!(queues.dequeue_high(), None);
    assert!(queues.has_background());
}

#[test]
fn ui_queues_drain_all_returns_everything_and_zeroes_counts() {
    let mut queues = UiQueues::new();
    queues.enqueue(UiPriority::Background, "bg");
    queues.enqueue(UiPriority::Render, "render");
    queues.enqueue(UiPriority::Normal, "normal-1");
    queues.enqueue(UiPriority::Normal, "normal-2");
    queues.enqueue(UiPriority::Send, "send");

    let all = queues.drain_all();

    assert_eq!(all, vec!["send", "normal-1", "normal-2", "render", "bg"]);
    assert!(!queues.has_high());
    assert!(!queues.has_background());
    assert_eq!(queues.dequeue_high(), None);
    assert_eq!(queues.dequeue_background(), None);
    // The count really is zero: a Background item comes out at once after a new Background enqueue.
    queues.enqueue(UiPriority::Background, "again");
    assert_eq!(queues.dequeue_background(), Some("again"));
}

#[test]
fn wake_flag_allows_exactly_one_poster_until_cleared() {
    let flag = WakeFlag::default();
    assert!(!flag.is_set());
    assert!(flag.try_set());
    assert!(flag.is_set());
    assert!(!flag.try_set());
    assert!(!flag.try_set());
    flag.clear();
    assert!(!flag.is_set());
    assert!(flag.try_set());
}

#[test]
fn wake_flag_allows_exactly_one_poster_among_eight_threads() {
    // C#: Win32UiDispatcherTests.Priorities_ThousandInterleavedPostsFromPoolThreadWhileUiBusy_RunInPriorityOrderWithOneWakeMessage (the "one wake" half)
    let flag = Arc::new(WakeFlag::default());
    let barrier = Arc::new(Barrier::new(8));
    let winners = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (flag, barrier, winners) = (Arc::clone(&flag), Arc::clone(&barrier), Arc::clone(&winners));
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..1000 {
                    if flag.try_set() {
                        winners.fetch_add(1, Ordering::SeqCst);
                    }
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(winners.load(Ordering::SeqCst), 1);
}

#[test]
fn wake_flag_clear_before_read_protocol() {
    // T4. Scripted interleaving of one drain and one poster, on the same pure pieces the dispatcher uses.
    // Correct drain: clear the flag, THEN read the queues. A poster that enqueues after the read finds the flag clear and wakes again.
    let flag = WakeFlag::default();
    let mut queues: UiQueues<u32> = UiQueues::new();
    queues.enqueue(UiPriority::Normal, 1);
    assert!(flag.try_set()); // the first post woke the UI thread

    flag.clear(); // drain starts: clear first
    while queues.dequeue_high().is_some() {} // ... then read the queues until empty
    queues.enqueue(UiPriority::Normal, 2); // a poster slips in right after the last read
    assert!(
        flag.try_set(),
        "the item enqueued after the read must post a fresh wake"
    );

    // The broken order (read, then clear) loses that wake: the poster finds the flag still set and posts nothing, then the clear erases the evidence.
    let flag = WakeFlag::default();
    let mut queues: UiQueues<u32> = UiQueues::new();
    queues.enqueue(UiPriority::Normal, 1);
    assert!(flag.try_set());
    while queues.dequeue_high().is_some() {} // read first
    queues.enqueue(UiPriority::Normal, 2);
    let woke = flag.try_set(); // poster: the flag is still set
    flag.clear(); // clear last
    assert!(
        !woke && queues.has_high() && !flag.is_set(),
        "nobody will ever wake for item 2"
    );
}

#[test]
fn post_is_normal_priority_before_background_queued_earlier() {
    // C#: Win32UiDispatcherTests.SynchronizationContextPost_RunsAtNormalPriority_BeforeBackgroundQueuedEarlier (retired as a synchronization-context
    // test; its Rust form is `UiScheduler::post` = Normal). Pure model of the enqueue path: bg (Background), then ctx (Normal), then last (Background).
    let mut queues = UiQueues::new();
    queues.enqueue(UiPriority::Background, "bg");
    queues.enqueue(UiPriority::Normal, "ctx");
    queues.enqueue(UiPriority::Background, "last");

    let mut order = Vec::new();
    while let Some(item) = queues.dequeue_high() {
        order.push(item);
    }
    while let Some(item) = queues.dequeue_background() {
        order.push(item);
    }
    assert_eq!(order, ["ctx", "bg", "last"]);
}

#[test]
fn priority_numeric_values_are_the_csharp_ones() {
    // T9: `UiQueues` indexes its levels with `priority as usize`.
    assert_eq!(UiPriority::Send as usize, 0);
    assert_eq!(UiPriority::Normal as usize, 1);
    assert_eq!(UiPriority::Render as usize, 2);
    assert_eq!(UiPriority::Background as usize, 3);
}
