//! Frame clocks of the window host (W8-13a): the shared base (`base`, C# `FrameClockBase`) and the waitable-timer clock (`timer`,
//! C# `TimerFrameClock`). The swap-chain clock is `host::swap_chain_clock` (W8-13b). Lead-owned index.

pub mod base;
pub mod timer;

pub use base::{
    ClockEnv, ClockPolicy, FrameClockBase, FrameHandler, FrameHandlerId, FrameTick, HostFrameClock, TimestampFn,
    next_grid_point, qpc_frequency, qpc_now,
};
pub use timer::{TimerFrameClock, TimerPolicy};
