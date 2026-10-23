#![allow(dead_code)]

#[path = "common/dialogs_secondary_fixture.rs"]
mod fixture;

use fixture::FakeMeasure;
use pr_platform_win::dialogs::{diagnostics, zoom_prompt};

// C#: ZoomPromptDialogTests.ClientWidth_IsTwoHundredDialogUnits_OfTheBodyFont
#[test]
fn zoom_layout_matches_the_worked_example() {
    let mut m = FakeMeasure { avg_char_width: 6, line_height: 15, ..Default::default() };
    m.item_heights.insert(zoom_prompt::LABEL_ID, 30);
    m.item_heights.insert(zoom_prompt::ERROR_ID, 28);
    assert_eq!(zoom_prompt::preferred_client_size(&m, false), (300, 147));
    let p = zoom_prompt::layout_plan(&m, 300, false);
    assert_eq!(p.placements, vec![
        zoom_prompt::Placement::new(100, 18, 19, 264, 30),
        zoom_prompt::Placement::new(101, 18, 57, 264, 26),
    ]);
    assert_eq!(p.button_row, zoom_prompt::ButtonRowPlan { client_width: 300, right_margin: 18, top: 102, height: 26, ids: vec![2, 1] });
    assert_eq!(zoom_prompt::preferred_client_size(&m, true), (300, 183));
    assert_eq!(zoom_prompt::layout_plan(&m, 300, true).button_row.top, 138);
}

// C#: DiagnosticsAndListDialogTests.Diagnostics_ShowsTheFifteenLabelValuePairs_InTheOrderOfTheWpfWindow
#[test]
fn diagnostics_layout_matches_the_worked_example() {
    let mut m = FakeMeasure { avg_char_width: 6, line_height: 15, ..Default::default() };
    m.item_heights.insert(diagnostics::HEADER_ID, 20);
    m.item_heights.insert(diagnostics::NOTE_ID, 40);
    for r in 0..15 { m.item_heights.insert(diagnostics::FIRST_LABEL_ID + r, 15); m.item_heights.insert(diagnostics::FIRST_VALUE_ID + r, 30); }
    assert_eq!(diagnostics::preferred_client_size(&m), (600, 712));
    let p = diagnostics::layout_plan(&m, 600);
    assert_eq!(p.placements[0], zoom_prompt::Placement::new(200, 21, 23, 558, 20));
    assert_eq!(p.placements[1], zoom_prompt::Placement::new(201, 21, 49, 558, 40));
    assert_eq!(p.placements[2], zoom_prompt::Placement::new(210, 21, 108, 188, 30));
    assert_eq!(p.placements[3], zoom_prompt::Placement::new(240, 218, 108, 361, 30));
    assert_eq!(p.button_row.top, 663);
}

#[test]
fn diagnostics_rows_out_of_range_return_none() {
    assert_eq!(diagnostics::label_text(15), None);
}
