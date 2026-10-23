# R0001: UI framework

**Status:** Proposed, direction chosen (P-06: Win32 + Direct2D/DirectWrite through `windows-rs`). The Phase 0 spike shows the
architecture is fast enough; one gate is still open (windowed p95, see Decision). egui and Slint were not spiked: P-06
replaced them after C# moved to Win32 + Direct2D (C# ADR 0010).

## Context

C# uses WPF (C# ADR 0002). The top priority is photo-switching and rendering speed; the UI also has ~10 windows
(Main, Settings, Recovery, ActionProfiles, BatchReview, ClickZoomCustomDialog, Diagnostics, Benchmark, ...), Vietnamese
text input, keyboard-first navigation, zoom in source pixels (C# ADR 0008), side-by-side Compare and fullscreen.

## Candidates

| Option | For | Against |
|---|---|---|
| **eframe/egui + wgpu** (favoured) | Image = GPU texture, cheap zoom/pan/compare; dialogs are fast to build; one process, no webview | Non-native look; immediate mode redraws need care to stay idle; accessibility via AccessKit is limited |
| Slint | Declarative, native-looking, good tooling | Custom zoom/pan canvas harder; less control over texture upload path |
| Tauri / webview | Familiar web UI | Pixel path through webview works against the speed priority; rejected unless spikes surprise |
| windows-rs + WinUI/Win32 | Fully native | Very slow to build ~10 windows; rejected for a fast port |

## Spike protocol (same for each candidate, ~1 day each)

1. Open a large JPEG folder (fixture paths in `work/diag/fixtures.local.json` of the C# repo), arrow-key switching
   with a 1-ahead/1-behind preload, Fit + source-pixel zoom with wheel and drag pan.
2. Measure switch time p50/p95 and first frame with the same profile as `PhotoReview.Benchmark.Cli`, same machine,
   same fixture, same viewport. Record idle CPU with the window open and nothing changing.
3. Check Vietnamese input (Unikey/Windows IME) in a text box, DPI scaling at 150%, and fullscreen on a second monitor.

## Decision

**Direction: Win32 window + Direct2D/DirectWrite via `windows-rs` (P-06). Status stays Proposed until the windowed p95 gate below is re-measured on an attended machine.**

### What the spike is

`cargo run -p pr-app --release --features ui-spike --bin r0001-spike -- <folder> [--measure N] [--interval-ms MS] [--size WxH] [--full]`
(`crates/pr-shell/src/spike`, window host `crates/pr-platform-win/src/spike_ui.rs`; both behind the `ui-spike` feature, so the
normal build and tests are unchanged; all spike `unsafe` is in `spike_ui.rs`, each block has `// SAFETY:`).
It opens a window, decodes with the production `WicDirectDecoder` (`pr-platform-win`) into `PixelBuffer`, switches with
Left/Right (1-ahead/1-behind preload on 2 worker threads), Fit + source-pixel zoom (wheel, nearest above 100%), drag pan,
`F` fit, `1` 100%, draws through a Direct3D 11 device + DXGI flip swap chain + Direct2D device context (legacy
`ID2D1HwndRenderTarget` with `PHOTOREVIEW_RS_SPIKE_BACKEND=hwnd`), with DirectWrite text. A blocking `GetMessageW` loop repaints
only after `InvalidateRect`. `--measure N` drives N switches through the same handler and prints p50/p95. `--generate <dir> <count> [WxH]`
writes synthetic photo-like JPEGs through WIC (the repo fixtures are at most 800x600). Fit uses a viewport-sized preview
(WIC decode box); the full-resolution image is decoded on the first zoom (`--full` decodes everything at full size).

### Measured (2026-10-18, this machine)

Core i7-9750H, 32 GB, Windows 11, Intel UHD 630 (default adapter) + RTX 2080 Max-Q, release build, client 1600x900,
40 JPEGs of 6000x4000 (24 MP, 8 MB), 100 switches, cyclic. "switch" = arrow-key handler start to `EndDraw` + `Present` returned.

| Measurement | Result |
|---|---|
| Switch, preloaded (250 ms between keys, 100% hit), windowed | p50 5.3 ms; p95 416..512 ms over 3 runs because 9..29% of frames stall about 510 ms inside `EndDraw` (not in `Present`) |
| Same, Direct2D target bitmap without swap chain (`BACKEND=offscreen`) | p50 0.47 ms, p95 0.66 ms (max 34..111 ms, the first switch) |
| Same, WARP software adapter | p50 about 0.7 ms |
| Same, NVIDIA adapter (Optimus) | p50 0.9 ms, stalls of about 300 ms in about 20% of frames |
| Held key (40 ms between keys, 25 keys/s) | hit rate 26..29%; a miss costs p50 150 ms, which is the decode (p50 115 ms, p95 165 ms per 24 MP image, 2 workers) |
| Preview bitmap upload | p50 0.6 ms |
| `--full` (24 MP bitmap drawn scaled to fit) | p50 77 ms per frame on the UHD 630: use a viewport-sized preview for Fit |
| First zoom (full-resolution decode + upload, synchronous) | 380..490 ms; later zoom/pan frames p50 0.1..1.7 ms |
| First frame (process start to first image drawn) | 180..240 ms (includes the synchronous decode of image 0) |
| Idle CPU (window open, nothing changing, 3 s) | 0.000 s CPU (below the 15.6 ms timer granularity, so under 0.5% of a core) |
| Repo fixtures (`wic-direct`, `turbojpeg`; tiny images, 6 corrupt files counted as errors) | switch p50 1.6..1.7 ms, same stall pattern |

Reference: the C# S3 F1 final p95 is 8.5 ms with 98.6% hit rate (C# ADR 0002). That run uses other fixtures and a WPF harness, and
`PhotoReview.Benchmark.Cli` was not run side by side, so the comparison is indicative only. `EndDraw` returns when the commands are
submitted; GPU completion is not measured.

### Reading

- UI cost is not the bottleneck: draw + submit is below 1 ms, idle CPU is zero, zoom/pan frames are a few ms. A held key is
  limited by decode throughput (the same as C#), not by the window.
- The p95 of the windowed path is not "clearly under" 8.5 ms on this machine because of the 300..510 ms stalls in `EndDraw`. They
  disappear without the swap chain (offscreen) and with WARP, and they appear with the HWND render target, the DXGI flip swap
  chain, an activated and a non-activated window, and both GPUs, so the cause is in the present/compositor path of this
  laptop (unattended, display state unknown) and not in the drawing code. This was not isolated further.
- So the numbers support the direction, not yet the status. Re-measure on an attended machine with the display on and the window in
  the foreground: `r0001-spike <24 MP folder> --measure 100 --interval-ms 250` (also with `PHOTOREVIEW_RS_SPIKE_ADAPTER=0|1|warp`).
  If the p95 stays above 10 ms, next steps are a waitable swap chain (`DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`) and a
  DirectComposition visual, still inside this architecture. Set Status to Accepted once the windowed p95 is under 8.5 ms.

### Not tested

Vietnamese text input with Unikey/Windows IME in a text box (needs manual IME; the spike only draws a Vietnamese string with
DirectWrite, not inspected visually), DPI scaling at 150% (per-monitor DPI aware v2, draws in device pixels, never run at 150%),
fullscreen on a second monitor, Compare side by side, the ~10 secondary windows, text box and menu controls (a native look needs
Win32 controls or custom drawing; this is the main risk of the choice and is unmeasured), RAW/HEIC, cold-cache disk reads, GPU
memory of a full-resolution bitmap per image, egui and Slint (not spiked).

### Consequences

- Wave 8 contracts are written for Win32 + Direct2D/DirectWrite in `pr-app`, with all `unsafe` in `pr-platform-win`
  (`spike_ui.rs` is a starting point for the window host, not production code).
- The Fit path draws a viewport-sized preview and decodes full resolution only on zoom; upload the next bitmap while idle.
- The feature-gated spike is deleted when the real viewer replaces it.
