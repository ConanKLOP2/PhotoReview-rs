//! DirectWrite text (`Shell.Rendering/Text/`, 398 lines, baseline `d92d6aae`), W8-12i: [`layout_cache`] is the pure, reference-counted LRU
//! cache of laid-out text (C# `TextLayoutCache.cs`, 197 lines); [`renderer`] is the DirectWrite renderer and the lease it hands out
//! (C# `DWriteTextRenderer.cs`, 201 lines).
//!
//! The renderer implements `pr_app::services::render::TextRenderer` and its lease implements `TextLayout` after the layering PR
//! (D-W8B1-1); the draw context (W8-12e) reaches the native `IDWriteTextLayout` through `TextLayout::as_any`.

pub mod layout_cache;
pub mod renderer;
