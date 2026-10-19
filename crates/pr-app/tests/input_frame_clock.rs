//! Port of the pure part of `PhotoReview.App.Shared/Input/FrameClock.cs` tests (unit W8-05a, contract `contracts/W8-05a.md`).
//!
//! No C# test ported here: every test is new (`// new:`). The raising rules of the Win32 clock are W8-13a.
//!
//! Code under test: `input::frame_clock`. `FrameTick`'s manual `PartialEq` is `todo!()` in the stub, so the two equality
//! tests fail until the porter implements it. The fake-clock test only exercises the test's own fake and passes on the stub.

use std::cell::Cell;
use std::rc::Rc;

use pr_app::input::frame_clock::{FrameClock, FrameHandler, FrameHandlerId, FrameTick};
use pr_core::abstractions::DisplayTiming;

// new: C# record equality compares `RenderingTimeMs` with `EqualityComparer<double>.Default`: NaN equals NaN,
// and 0.0 equals -0.0 (contract Semantics 4). Derived `PartialEq` on `f64` would make the NaN tick unequal to itself.
#[test]
fn frame_tick_equality_treats_nan_timing_as_equal() {
    let nan_tick = FrameTick {
        timestamp: 5,
        rendering_time_ms: f64::NAN,
        timing: None,
    };
    assert_eq!(nan_tick, nan_tick);
    assert_eq!(
        nan_tick,
        FrameTick {
            timestamp: 5,
            rendering_time_ms: f64::NAN,
            timing: None,
        }
    );

    let zero = FrameTick {
        timestamp: 5,
        rendering_time_ms: 0.0,
        timing: None,
    };
    let negative_zero = FrameTick {
        rendering_time_ms: -0.0,
        ..zero
    };
    assert_eq!(zero, negative_zero);
    assert_ne!(nan_tick, zero);
}

// new: equality compares every field: timestamp, rendering time, and the optional display timing.
#[test]
fn frame_tick_equality_differs_on_any_field() {
    let base = FrameTick {
        timestamp: 100,
        rendering_time_ms: 16.5,
        timing: Some(DisplayTiming {
            last_vblank: 1_000,
            refresh_period: 166_667,
        }),
    };
    assert_eq!(base, base);
    assert_ne!(base, FrameTick { timestamp: 101, ..base }, "timestamp");
    assert_ne!(
        base,
        FrameTick {
            rendering_time_ms: 16.6,
            ..base
        },
        "rendering time"
    );
    assert_ne!(base, FrameTick { timing: None, ..base }, "timing present versus absent");
    assert_ne!(
        base,
        FrameTick {
            timing: Some(DisplayTiming {
                last_vblank: 1_001,
                refresh_period: 166_667,
            }),
            ..base
        },
        "timing last_vblank"
    );
    assert_ne!(
        base,
        FrameTick {
            timing: Some(DisplayTiming {
                last_vblank: 1_000,
                refresh_period: 166_668,
            }),
            ..base
        },
        "timing refresh_period"
    );
}

/// Test double of [`FrameClock`]. Keeps its subscriptions and request count visible to the test.
#[derive(Default)]
struct FakeFrameClock {
    handlers: Vec<(FrameHandlerId, FrameHandler)>,
    next_id: FrameHandlerId,
    requests: Rc<Cell<u32>>,
}

impl FakeFrameClock {
    fn subscriber_count(&self) -> usize {
        self.handlers.len()
    }
}

impl FrameClock for FakeFrameClock {
    fn subscribe(&mut self, handler: FrameHandler) -> FrameHandlerId {
        self.next_id += 1;
        self.handlers.push((self.next_id, handler));
        self.next_id
    }

    fn unsubscribe(&mut self, id: FrameHandlerId) {
        self.handlers.retain(|(existing, _)| *existing != id);
    }

    fn request_frame(&mut self) {
        self.requests.set(self.requests.get() + 1);
    }
}

// new: `FrameClock` is object safe (used as `Box<dyn FrameClock>`), subscribe returns distinct ids,
// unsubscribing an unknown id is a no-op, and request_frame reaches the clock.
#[test]
fn frame_clock_is_object_safe_with_a_fake() {
    let _boxed: Box<dyn FrameClock> = Box::new(FakeFrameClock::default());

    let mut fake = FakeFrameClock::default();
    let first;
    let second;
    {
        let clock: &mut dyn FrameClock = &mut fake;
        first = clock.subscribe(Box::new(|_tick: &FrameTick| {}));
        second = clock.subscribe(Box::new(|_tick: &FrameTick| {}));
        assert_ne!(first, second, "distinct subscriptions get distinct ids");

        clock.unsubscribe(first);
        clock.unsubscribe(FrameHandlerId::MAX); // unknown id: no-op, must not panic
        clock.request_frame();
    }
    assert_eq!(fake.subscriber_count(), 1, "only the second subscription remains");
    assert_eq!(fake.requests.get(), 1);
}
