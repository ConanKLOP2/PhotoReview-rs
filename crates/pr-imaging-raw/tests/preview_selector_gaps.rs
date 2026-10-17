//! Mutation-gap tests for W5-11b PreviewSelector (length-4 unknown-size sibling; partial request boxes with a negative side).
use pr_imaging::decode_types::DecodeBox;
use pr_imaging_raw::preview_selector::{select_preview, select_preview_resolved};
use pr_imaging_raw::{EmbeddedPreview, EmbeddedPreviewKind, InMemoryRawHeaderSource, PreviewColorSpace};

fn p(index: i32, offset: i64, length: i64, w: i32, h: i32) -> EmbeddedPreview {
    EmbeddedPreview::new(
        index,
        offset,
        length,
        EmbeddedPreviewKind::Jpeg,
        w,
        h,
        PreviewColorSpace::Unknown,
    )
}

/// C# ResolveDimensions: length 4 passes the `< 4` gate, WalkJpeg runs (finds no segment) and HeaderResolved becomes true
/// for a NON-chosen sibling too (the chosen one is resolved again by ResolveColorSpace, which hides the gate for it).
#[test]
fn length_exactly_four_unknown_size_sibling_is_walked_and_marked_resolved() {
    let mut src = InMemoryRawHeaderSource::new(vec![0xFF, 0xD8, 0x00, 0x00, 0, 0, 0, 0]);
    let previews = [p(0, 0, 100, 2000, 1500), p(1, 0, 4, 0, 0)];
    let sel = select_preview_resolved(&mut src, &previews, DecodeBox::UNBOUNDED, 1).unwrap();
    assert_eq!(sel.chosen.unwrap().index, 0);
    assert!(sel.resolved_previews[1].header_resolved);
}

fn pick(previews: &[EmbeddedPreview], w: i32, h: i32) -> i32 {
    let mut src = InMemoryRawHeaderSource::new(vec![0u8; 256]);
    select_preview(&mut src, previews, DecodeBox::new(w, h), 1)
        .unwrap()
        .unwrap()
        .index
}

/// Partial box, previews with a non-positive side: only the requested side is compared (C# `reqH <= 0` ignores visH).
#[test]
fn partial_box_width_only_ignores_a_negative_height() {
    let v = [p(0, 0, 100, 200, -1), p(1, 0, 50, 300, -1)];
    assert_eq!(pick(&v, 250, 0), 1);
}

#[test]
fn partial_box_height_only_ignores_a_negative_width() {
    let v = [p(0, 0, 100, -1, 200), p(1, 0, 50, -1, 300)];
    assert_eq!(pick(&v, 0, 250), 1);
}
