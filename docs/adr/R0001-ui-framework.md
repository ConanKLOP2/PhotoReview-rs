# R0001: UI framework

**Status:** Proposed. Decided by the Phase 0 spike, not before.

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

_To be filled after the spike with the measured numbers._
