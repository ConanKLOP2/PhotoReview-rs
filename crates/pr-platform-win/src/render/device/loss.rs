//! Device loss and teardown of `DeviceResources` (W8-12b): `EndDraw` and `Present` through the fault seams (lines 66 to 70, 177 to 194),
//! `Recreate` (196 to 203), `Dispose` (235 to 244), `ReleaseAll` (397 to 434) and `DeviceLoss` (437 to 444), baseline `d92d6aae`.
//! Contract: `contracts/W8-12b.md`.
//!
//! `DeviceLoss.IsDeviceLost` / `Describe` are NOT redefined here: they are `gfx::com::{is_device_lost, describe_hresult}` (D-W8B3G-5).
//! The three HRESULTs that mean "recreate" are `D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED` and `DXGI_ERROR_DEVICE_RESET`.

use super::{DeviceResources, FaultHook, RenderFault, object_disposed};
use crate::gfx::{com, d2d, d3d11, dxgi};

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
    if com::is_device_lost(hresult) {
        FrameEnd::DeviceLost
    } else if hresult < 0 {
        FrameEnd::Failed
    } else {
        FrameEnd::Continue
    }
}

/// Runs a test hook on `hresult`. The hook is taken out of its cell while it runs, so a hook that touches the seam cannot trip the
/// `RefCell`; it is put back unless the hook itself installed another one.
fn apply_hook(slot: &std::cell::RefCell<Option<FaultHook>>, hresult: i32) -> i32 {
    let taken = slot.borrow_mut().take();
    match taken {
        None => hresult,
        Some(mut hook) => {
            let result = hook(hresult);
            let mut current = slot.borrow_mut();
            if current.is_none() {
                *current = Some(hook);
            }
            result
        }
    }
}

impl DeviceResources {
    /// C# `EndDraw`: `gfx::d2d::end_draw_hresult(context)` then the `EndDrawFault` hook when set (it sees the real HRESULT and
    /// returns the one to use). `Err(Disposed)` after `dispose`. The hook runs even when the real call succeeded.
    pub fn end_draw(&self) -> Result<i32, RenderFault> {
        let context = self.context()?;
        let hresult = d2d::end_draw_hresult(&context);
        Ok(apply_hook(&self.end_draw_fault, hresult))
    }

    /// C# `Present(syncInterval)`: `0` without a swap chain (the hook is NOT called then), else `gfx::dxgi::present_raw(.., sync_interval)`
    /// (flags always 0) through the `PresentFault` hook. The caller classifies with `gfx::dxgi::classify_present`.
    pub fn present(&self, sync_interval: u32) -> i32 {
        let swap_chain = self
            .state
            .borrow()
            .swap_chain
            .as_ref()
            .map(|created| created.swap_chain.clone());
        let Some(swap_chain) = swap_chain else {
            return 0;
        };
        let hresult = dxgi::present_raw(&swap_chain, sync_interval);
        apply_hook(&self.present_fault, hresult)
    }

    /// C# `Recreate`: `Err(Disposed)` after `dispose`; otherwise `release_all`, `generation += 1`, `create_all` (same hwnd, size,
    /// DPI; the fault hooks and `live_bitmap_count` survive). If `create_all` fails the error is returned, the object holds no
    /// device and the object getters give `Err(Disposed)` (C# `_context ?? throw new ObjectDisposedException`); a later `recreate`
    /// may succeed.
    pub fn recreate(&self) -> Result<(), RenderFault> {
        if self.state.borrow().disposed {
            return Err(object_disposed());
        }
        self.release_all();
        self.state.borrow_mut().generation += 1;
        self.create_all()
    }

    /// C# `Dispose`: idempotent; sets `disposed`, then `release_all`. `Drop` calls it.
    pub fn dispose(&self) {
        {
            let mut state = self.state.borrow_mut();
            if state.disposed {
                return;
            }
            state.disposed = true;
        }
        self.release_all();
    }

    /// C# `ReleaseAll` (lines 397 to 434), order matters (comment at C# lines 422 to 426): `release_target` (W8-12c), drop the sync
    /// probe (counted by `release_bitmap`), drop the waitable handle with the swap chain LATER, drop brush, context, device, factory,
    /// then `d3d11::clear_state_and_flush(d3d)` while the swap chain is STILL alive, then drop the swap chain, then the D3D device.
    pub(super) fn release_all(&self) {
        self.release_target();
        let probe = self.state.borrow_mut().sync_probe.take();
        self.release_bitmap(probe);
        // Take every object out of `state` first (the borrow ends here), then drop them one by one in the C# order.
        let (brush, context, device, factory, swap_chain, d3d) = {
            let mut state = self.state.borrow_mut();
            (
                state.brush.take(),
                state.context.take(),
                state.device.take(),
                state.factory.take(),
                state.swap_chain.take(),
                state.d3d.take(),
            )
        };
        drop(brush);
        drop(context);
        drop(device);
        drop(factory);
        if let Some(d3d) = &d3d {
            // ClearState + Flush while the swap chain is STILL alive: flushing after it is gone hangs forever on a CI runner
            // (WARP + DWM), see the C# comment. The waitable handle closes with the swap chain below.
            d3d11::clear_state_and_flush(d3d);
        }
        drop(swap_chain);
        drop(d3d);
    }
}

impl Drop for DeviceResources {
    fn drop(&mut self) {
        self.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // C#: D2DRenderSurface.EndDrawAndPresent lines 133 to 145 (DeviceLoss.IsDeviceLost before hr < 0)
    #[test]
    fn classify_end_draw_checks_device_loss_first() {
        assert_eq!(classify_end_draw(0), FrameEnd::Continue);
        assert_eq!(classify_end_draw(1), FrameEnd::Continue);
        for lost in [0x8899_000C_u32, 0x887A_0005, 0x887A_0007] {
            assert_eq!(classify_end_draw(lost as i32), FrameEnd::DeviceLost, "{lost:#X}");
        }
        assert_eq!(classify_end_draw(0x8000_4005_u32 as i32), FrameEnd::Failed);
        assert_eq!(classify_end_draw(i32::MIN), FrameEnd::Failed);
        // DXGI_ERROR_WAS_STILL_DRAWING is a failure, not a loss
        assert_eq!(classify_end_draw(0x887A_000A_u32 as i32), FrameEnd::Failed);
    }

    // C#: DeviceResources.Recreate after Dispose
    #[test]
    fn recreate_after_dispose_is_disposed_without_com() {
        let resources = DeviceResources::new_unbuilt(0, 8, 8, 1.0, true, 1);
        resources.dispose();
        assert_eq!(resources.recreate().unwrap_err(), object_disposed());
        assert_eq!(resources.generation(), 0);
    }

    // C#: DeviceResources.Present / EndDraw seams
    #[test]
    fn apply_hook_passes_through_without_a_hook_and_keeps_the_hook() {
        let slot = std::cell::RefCell::new(None);
        assert_eq!(apply_hook(&slot, 5), 5);
        *slot.borrow_mut() = Some(Box::new(|hr| hr + 1) as FaultHook);
        assert_eq!(apply_hook(&slot, 5), 6);
        assert_eq!(apply_hook(&slot, 6), 7, "the hook stays installed");
    }
}
