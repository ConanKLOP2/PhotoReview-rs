//! Tests for `pr_core::catalog::generation` (W1-12). Port of GenerationClockTests.cs.

use pr_core::catalog::generation::GenerationClock;

#[derive(Clone, Copy, Debug)]
enum Generation {
    Navigation,
    Folder,
    Interaction,
}

const ALL: [Generation; 3] = [Generation::Navigation, Generation::Folder, Generation::Interaction];

fn next(c: &GenerationClock, g: Generation) -> i64 {
    match g {
        Generation::Navigation => c.next_navigation(),
        Generation::Folder => c.next_folder(),
        Generation::Interaction => c.next_interaction(),
    }
}

fn current(c: &GenerationClock, g: Generation) -> i64 {
    match g {
        Generation::Navigation => c.current_navigation(),
        Generation::Folder => c.current_folder(),
        Generation::Interaction => c.current_interaction(),
    }
}

fn is_current(c: &GenerationClock, g: Generation, v: i64) -> bool {
    match g {
        Generation::Navigation => c.is_navigation_current(v),
        Generation::Folder => c.is_folder_current(v),
        Generation::Interaction => c.is_interaction_current(v),
    }
}

#[test]
fn constructor_initializes_generations_to_specified_values() {
    // C#: GenerationClockTests.Constructor_InitializesGenerationsToSpecifiedValues
    for (n, f, i) in [(0, 0, 0), (12, 34, 56)] {
        let clock = GenerationClock::new(n, f, i);
        assert_eq!(clock.current_navigation(), n);
        assert_eq!(clock.current_folder(), f);
        assert_eq!(clock.current_interaction(), i);
        assert!(clock.is_navigation_current(n));
        assert!(clock.is_folder_current(f));
        assert!(clock.is_interaction_current(i));
    }
}

#[test]
fn next_increments_monotonically_and_updates_current() {
    // C#: GenerationClockTests.Next_IncrementsMonotonicallyAndUpdatesCurrent
    for g in ALL {
        let clock = GenerationClock::default();
        assert_eq!(next(&clock, g), 1, "{g:?}");
        assert_eq!(current(&clock, g), 1);
        assert!(is_current(&clock, g, 1));
        assert!(!is_current(&clock, g, 0));
        assert_eq!(next(&clock, g), 2);
        assert_eq!(current(&clock, g), 2);
        assert!(is_current(&clock, g, 2));
        assert!(!is_current(&clock, g, 1));
    }
}

#[test]
fn stop_for_action_increments_all_three_counters() {
    // C#: GenerationClockTests.StopForAction_IncrementsAllThreeCountersSimultaneously
    let clock = GenerationClock::new(5, 10, 15);
    clock.stop_for_action();
    assert_eq!(clock.current_navigation(), 6);
    assert_eq!(clock.current_folder(), 11);
    assert_eq!(clock.current_interaction(), 16);
    assert!(clock.is_navigation_current(6));
    assert!(clock.is_folder_current(11));
    assert!(clock.is_interaction_current(16));
    assert!(!clock.is_navigation_current(5));
    assert!(!clock.is_folder_current(10));
    assert!(!clock.is_interaction_current(15));
}

#[test]
fn concurrent_increments_are_thread_safe_and_accurate() {
    // C#: GenerationClockTests.ConcurrentIncrements_AreThreadSafeAndAccurate
    let clock = GenerationClock::default();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..1250 {
                    clock.next_navigation();
                    clock.next_folder();
                    clock.next_interaction();
                }
            });
        }
    });
    assert_eq!(clock.current_navigation(), 10_000);
    assert_eq!(clock.current_folder(), 10_000);
    assert_eq!(clock.current_interaction(), 10_000);
    assert!(clock.is_navigation_current(10_000));
    assert!(clock.is_folder_current(10_000));
    assert!(clock.is_interaction_current(10_000));
}

#[test]
fn concurrent_stop_for_action_increments_all_counters_without_loss() {
    // C#: GenerationClockTests.ConcurrentStopForAction_IncrementsAllCountersWithoutLoss
    let clock = GenerationClock::default();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..625 {
                    clock.stop_for_action();
                }
            });
        }
    });
    assert_eq!(clock.current_navigation(), 5_000);
    assert_eq!(clock.current_folder(), 5_000);
    assert_eq!(clock.current_interaction(), 5_000);
    assert!(clock.is_navigation_current(5_000));
    assert!(clock.is_folder_current(5_000));
    assert!(clock.is_interaction_current(5_000));
}

#[test]
fn increment_wraps_at_i64_max_without_panic() {
    // C#: Interlocked.Increment wraps (no GenerationClockTests twin; own test)
    let clock = GenerationClock::new(i64::MAX, i64::MAX, i64::MAX);
    assert_eq!(clock.next_navigation(), i64::MIN);
    assert_eq!(clock.current_navigation(), i64::MIN);
    clock.stop_for_action();
    assert_eq!(clock.current_navigation(), i64::MIN + 1);
    assert_eq!(clock.current_folder(), i64::MIN);
    assert_eq!(clock.current_interaction(), i64::MIN);
}

#[test]
fn default_equals_new_zero() {
    // C#: new GenerationClock() (own test)
    let d = GenerationClock::default();
    assert_eq!(d.current_navigation(), 0);
    assert_eq!(d.current_folder(), 0);
    assert_eq!(d.current_interaction(), 0);
}

#[test]
fn counters_are_independent() {
    // C#: own test
    let clock = GenerationClock::new(1, 2, 3);
    clock.next_navigation();
    assert_eq!(clock.current_folder(), 2);
    assert_eq!(clock.current_interaction(), 3);
    clock.next_folder();
    assert_eq!(clock.current_navigation(), 2);
    assert_eq!(clock.current_interaction(), 3);
    clock.next_interaction();
    assert_eq!(clock.current_navigation(), 2);
    assert_eq!(clock.current_folder(), 3);
}

#[test]
fn clock_is_send_and_sync() {
    // C#: own test
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<GenerationClock>();
}

#[test]
fn stale_and_future_tokens_are_not_current() {
    // C#: own test
    let clock = GenerationClock::default();
    let old = clock.current_navigation();
    clock.next_navigation();
    assert!(!clock.is_navigation_current(old));
    assert!(!clock.is_navigation_current(clock.current_navigation() + 1));
    let f = clock.current_folder();
    clock.next_folder();
    assert!(!clock.is_folder_current(f));
    assert!(!clock.is_folder_current(clock.current_folder() + 1));
    let i = clock.current_interaction();
    clock.next_interaction();
    assert!(!clock.is_interaction_current(i));
    assert!(!clock.is_interaction_current(clock.current_interaction() + 1));
}

#[test]
fn future_tokens_are_never_current() {
    // C#: own test (contract note 4: equality, not ordering)
    for g in ALL {
        let clock = GenerationClock::new(7, 7, 7);
        assert!(is_current(&clock, g, 7), "{g:?}");
        assert!(!is_current(&clock, g, 8), "{g:?}");
        assert!(!is_current(&clock, g, 6), "{g:?}");
        assert!(!is_current(&clock, g, i64::MAX), "{g:?}");
        assert!(!is_current(&clock, g, i64::MIN), "{g:?}");
    }
}
