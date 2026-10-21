//! Device loss and teardown of `DeviceResources` (W8-12b): `EndDraw` and `Present` through the fault seams (lines 66 to 70, 177 to 194),
//! `Recreate` (196 to 203), `Dispose` (235 to 244), `ReleaseAll` (397 to 434) and `DeviceLoss` (437 to 444), baseline `d92d6aae`.
//! Contract: `contracts/W8-12b.md`.
//!
//! `DeviceLoss.IsDeviceLost` / `Describe` are NOT redefined here: they are `gfx::com::{is_device_lost, describe_hresult}` (D-W8B3G-5).
//! The three HRESULTs that mean "recreate" are `D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED` and `DXGI_ERROR_DEVICE_RESET`.

#![allow(unused_variables, dead_code)] // W8-12b stub: the porter removes this line when the last `todo!()` is gone.

use super::{DeviceResources, RenderFault};

/// What the surface does after `EndDraw` returned `hresult` (C# `D2DRenderSurface.EndDrawAndPresent` lines 133 to 145; pure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameEnd {
    /// `hresult >= 0`: go on to `Present`.
    Continue,
    /// A device-loss HRESULT: recreate the device, result `DeviceRecreated`.
    DeviceLost,
    /// Any other negative HRESULT: result `Failed` (NOT an error, no recreate).
    Failed,
}

/// Pure classification of an `EndDraw` HRESULT: device loss is checked FIRST (all three codes are negative).
#[must_use]
pub fn classify_end_draw(hresult: i32) -> FrameEnd {
    todo!("W8-12b")
}

impl DeviceResources {
    /// C# `EndDraw`: `gfx::d2d::end_draw_hresult(context)` then the `EndDrawFault` hook when set (it sees the real HRESULT and
    /// returns the one to use). `Err(Disposed)` after `dispose`. The hook runs even when the real call succeeded.
    pub fn end_draw(&self) -> Result<i32, RenderFault> {
        todo!("W8-12b")
    }

    /// C# `Present(syncInterval)`: `0` without a swap chain (the hook is NOT called then), else `gfx::dxgi::present_raw(.., sync_interval)`
    /// (flags always 0) through the `PresentFault` hook. The caller classifies with `gfx::dxgi::classify_present`.
    pub fn present(&self, sync_interval: u32) -> i32 {
        todo!("W8-12b")
    }

    /// C# `Recreate`: `Err(Disposed)` after `dispose`; otherwise `release_all`, `generation += 1`, `create_all` (same hwnd, size,
    /// DPI; the fault hooks and `live_bitmap_count` survive). If `create_all` fails the error is returned, the object holds no
    /// device and the object getters give `Err(Disposed)` (C# `_context ?? throw new ObjectDisposedException`); a later `recreate`
    /// may succeed.
    pub fn recreate(&self) -> Result<(), RenderFault> {
        todo!("W8-12b")
    }

    /// C# `Dispose`: idempotent; sets `disposed`, then `release_all`. `Drop` calls it.
    pub fn dispose(&self) {
        todo!("W8-12b")
    }

    /// C# `ReleaseAll` (lines 397 to 434), order matters (comment at C# lines 422 to 426): `release_target` (W8-12c), drop the sync
    /// probe (counted by `release_bitmap`), drop the waitable handle with the swap chain LATER, drop brush, context, device, factory,
    /// then `d3d11::clear_state_and_flush(d3d)` while the swap chain is STILL alive, then drop the swap chain, then the D3D device.
    pub(super) fn release_all(&self) {
        todo!("W8-12b")
    }
}

impl Drop for DeviceResources {
    fn drop(&mut self) {
        // The porter calls `self.dispose()`.
    }
}
