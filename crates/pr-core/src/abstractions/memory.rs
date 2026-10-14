//! Port of `IMemoryProbe.cs`: RAM pressure checks that throttle preload and caches.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub load_percent: u32,
    pub available_bytes: u64,
}

pub trait MemoryProbe: Send + Sync {
    /// True when memory load is below `maximum_load` and available bytes are at least `reserve_bytes`.
    fn has_headroom(&self, maximum_load: f64, reserve_bytes: i64) -> bool;
    fn get_snapshot(&self) -> Option<MemorySnapshot>;
}
