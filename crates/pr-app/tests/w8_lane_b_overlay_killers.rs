//! Killer tests for surviving mutants in the overlay Display impls (lane B).

use pr_app::overlay::animator::AnimatorError;
use pr_app::overlay::frame_delay::FrameDelayError;

#[test]
fn animator_error_display_messages() {
    assert_eq!(
        AnimatorError::InvalidTicksPerSecond(-5).to_string(),
        "ticks_per_second must be positive, got -5"
    );
    assert_eq!(AnimatorError::Disposed.to_string(), "animator is disposed");
}

#[test]
fn frame_delay_error_display_messages() {
    assert_eq!(
        FrameDelayError::InvalidTicksPerSecond(0).to_string(),
        "ticks_per_second must be positive, got 0"
    );
    assert_eq!(FrameDelayError::Disposed.to_string(), "frame delay timer is disposed");
}
