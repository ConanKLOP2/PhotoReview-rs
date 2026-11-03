# Q1 PARITY close: viewport core cluster (2026-11-03)

Branch `parity/viewport-core`, base `origin/master` 58aad090. Rows: X-viewport-state, X-viewport-layout, X-render-geometry,
X-fit-zoom-rules, X-menu-model, X-input-core, X-keyid-mirror, X-kinetic-pan, X-viewer-state, X-coordinator-traits.
Goal of the owner: "finish PARITY". Nothing here edits `docs/PARITY.md`; the proposed status per row is at the end.

## What "Done" needs here (AGENTS.md has no single promotion paragraph)

Read from AGENTS.md "Tests" and "Mutation testing" and `docs/PARITY-CLOSEOUT.md` step 2: a proving test per row, the C# tests of the unit
ported, no native/GUI part left out, and, for pure logic with branching, a mutation verdict of the owned file in which every mutant is
caught by a test that fails for the right reason (a hand-written test; a golden catch counts only when the golden files were really found
inside the cargo-mutants copy, `tools/dev/mutants.sh` checks this), killed by a new test, or explained as equivalent. A partial run is
never a full verdict. These rows have NO `#[ignore]` test and no native, GUI or corpus part (grep of the cluster's test files and sources),
so there is no native run to do or to omit.

## Environment and method

- Baseline: `cargo test -p pr-app --lib` plus the 29 test binaries of the cluster (list below): 30 `test result: ok` lines, 394 tests, 0 failed.
  Binaries: viewport_arrow_pan viewport_convergence viewport_fit viewport_golden viewport_layout viewport_layout_props viewport_state
  viewport_types viewer_state viewer_state_golden viewer_state_kill_gap viewer_state_mutation_gap kinetic_keyboard_pan kinetic_pan_golden
  kinetic_pan_velocity kinetic_scroller glide_frame_clock key_id menu_model menus_zoom_rules coordinators_traits input_frame_clock
  input_operation_version input_primitives input_review_command input_surface input_wheel_message render_geometry services_initial_viewport.
- Mutation: `bash tools/dev/mutants.sh pr-app <file> --tests <non-golden bins>` (cargo-mutants 27.1.0, `-j 1`, `--baseline skip`, timeout 90 s,
  preflight ran the real tests each time, own `TEMP`/target dir per process, `PHOTOREVIEW_GOLDEN_ROOT` exported by the script). Two
  processes at a time, `CARGO_BUILD_JOBS=2`; the machine was shared with other agents (3 to 4 `cargo-mutants` at once, about 8 GB free
  RAM), so a mutant took 40 to 90 s of build. The test binaries named per run are hand-written ones; no run depended on a golden catch.
  Wall time of mutation: about 2.5 hours in two parallel slots (more than the 60 minute box because of the load and the re-verification of killers).
- Format: `cargo fmt -p pr-app` fails with os error 206 on long paths, so `rustfmt --edition 2024 --check` was run on the changed files (clean).
- `cargo clippy -p pr-app --all-targets --locked -- -D warnings`: clean on the final head.

## Mutation results

| File | Mutants (cargo-mutants `--list`) | Result | Notes |
|---|---:|---|---|
| `viewport/fit.rs` + `menus/zoom_rules.rs` (X-fit-zoom-rules) | 17 + 7 = 24 | 24 tested: 20 caught, 4 unviable, 0 missed, 0 timeout | bins `viewport_fit`, `viewport_convergence`, `menus_zoom_rules` (41 tests) |
| `input/frame_clock.rs`, `operation_version.rs`, `wheel_message.rs`, `review_command.rs` (X-input-core) | 6 + 6 + 12 + 7 = 31 | 31 decided: 27 caught, 4 unviable, 0 missed, 0 timeout | first run hit its budget with 30 of 31 done; the last one (`wheel_message.rs:25:23 >> -> <<`) rerun alone: caught. `input/surface.rs` has 0 mutants (trait only) |
| `viewport/state.rs` (X-viewport-state) | 48 | all 48 caught after 8 killer tests (see below) | first pass found 6 survivors (`input`, `scroll_home`, four in `update_layout` return value; one of the four, 139:98, was first recorded unviable and caught later) plus, in the second pass, 12 more (`image_origin` y, `to_image_element` x4, `image_actual_width/height` x6); `non_negative` 6 not run at first, then 3 + 3 caught |
| `viewport/arrow_pan.rs` (X-viewport-state) | 40 | 28 caught, 10 unviable, 2 equivalent survivors | 5 real survivors killed by 3 new tests |
| `coordinators/traits.rs` (X-coordinator-traits) | 37 | 37 tested: 20 caught, 8 unviable, 9 survivors; after 3 new tests 7 of them caught, 2 equivalent | the 2 equivalents are listed below |
| `menus/builder.rs` (X-menu-model) | 34 | 34 tested: 27 caught, 6 unviable, 1 survivor (`MenuBuildError` Display) killed by a new test | the earlier 2026-10-30 run (34: 28 caught, 6 unviable) is reproduced; the rerun used the golden-safe script |

Killed by new tests (all hand-written, in `crates/pr-app/tests/`):
`viewport_state.rs`: `input_returns_the_last_accepted_input_even_before_layout`, `scroll_home_requests_offset_zero_on_both_axes`,
`update_layout_return_value_reports_each_component_change_on_its_own`, `image_origin_and_to_image_element_use_both_offsets`,
`image_actual_size_is_the_laid_out_image_rect`, `scroll_request_is_max_zero_value_with_positive_zero`;
`viewport_arrow_pan.rs`: `step_length_negative_zero_fraction_is_positive_zero`, `step_toward_origin_overshoot_clamps_to_zero_and_pans`,
`step_past_the_far_edge_clamps_to_max_and_at_max_is_at_edge`;
`coordinators_traits_q1_killers.rs` (new file): `file_action_sink_on_files_renamed_default_is_false`,
`presentation_sink_take_viewer_zoom_change_default_is_no_change`, `debug_text_names_the_type_and_does_not_call_the_factory`,
`menu_build_error_display_names_the_item_and_the_missing_row_kind`.
Total 13 new tests. No production code changed.

Equivalent mutants (classified, not killed):
- `arrow_pan.rs:81:27 < -> <=` (`moved < 0.0`): `moved == 0.0` gives target 0.0 either way; the only difference is `-0.0`, which needs
  `current == -0.0` and then `|target - current| = 0 < 0.5` returns `AtEdge` with the inputs unchanged, so the sign never reaches a caller.
- `arrow_pan.rs:83:21 > -> >=` (`moved > max`): at `moved == max` the clamp returns `max`, the same value.
- `traits.rs:178 on_files_renamed -> false`: the C# default body is `false`.
- `traits.rs:278 get_viewer_decode_delay -> Default::default()`: `Duration::default()` is `Duration::ZERO`, the default body.

## Per row

### X-fit-zoom-rules
- Gaps before: no mutation verdict; fit tests waited for `is_stable_viewport` (merged, `viewport_convergence`).
- Ran: all 7 C# tests of `FitViewControllerTests`/`FitViewControllerMutationGapTests` have a Rust twin in `viewport_fit.rs` (names checked);
  full mutation 24/24 decided, 0 missed.
- Gaps after: none that a machine can close. Proposed: **Done**.

### X-input-core
- Gaps before: no mutation verdict for `frame_clock`, `operation_version`, `wheel_message`, `review_command`; `surface` mutation exists (CL-MUT-SURF4, CX-INPUTMUT).
- Ran: 31/31 decided (27 caught, 4 unviable, 0 missed); both C# `ReviewCommandAutoRepeatTests` ported (`input_review_command.rs`); `surface.rs` has no
  mutable logic (0 mutants), trait only.
- Gaps after: none. Proposed: **Done**.

### X-coordinator-traits
- Gaps before: no mutation verdict for `traits.rs` default bodies, `FileActionServiceExecutor`, `PreloadControllerAdapter`.
- Ran: 37/37 decided; 9 survivors, 4 new tests, 7 killed, 2 equivalent (above). Adapter forwarding is also covered by `coordinators_preload_adapter`.
- Gaps after: none for the pure logic; the trait definitions are interfaces whose C# members have no more tests than the Rust twins. Proposed: **Done**.

### X-menu-model
- Gaps before: `menus/model.rs` is data only (0 mutants); `builder.rs` mutated 2026-10-30 before the golden-infra fix.
- Ran: `builder.rs` rerun 34/34: 27 caught, 6 unviable, 1 Display survivor killed by a new test. Golden `menu-model` 196 cases passed in the baseline
  (the 27 catches came from the golden-backed `menu_model` and the hand-written `rename_file_menu_model`, with the golden root pinned, no
  "no tests/golden above" warning).
- Gaps after: the 27 catches of `builder.rs` rest on the golden `menu_model` test (which is the C# oracle; there is no separate hand-written
  C# test of the builder for the whole tree, `ContextMenuModelBuilderTests` is a Shell test). If the owner insists on "non-golden only", they
  are not proven. Proposed: **Done** with that remark. (Judgement call: the golden is the oracle's own output and ran inside the mutation copy.)

### X-viewport-state
- Gaps before: no mutation verdict for `state.rs`, `types.rs`, `arrow_pan.rs`, `primitives.rs`.
- Ran: `state.rs` 48/48 (all caught after 6 new tests), `arrow_pan.rs` 40/40 decided (3 new tests, 2 equivalent).
- Gaps after: NOT RUN `types.rs` 75 mutants and `input/primitives.rs` 47 mutants (122 total). K-27 `Different` (ArrowPanRule new rule, ADR owed) is
  an owner/ADR matter. Proposed: **stay Ported**, note "state.rs 48/48 and arrow_pan.rs 40/40 mutation decided 2026-11-03 (Q1); types.rs 75 and primitives.rs 47 not run".

### X-viewport-layout, X-render-geometry
- Not mutated in this run. `layout.rs` 196 mutants and `render_geometry.rs` 367 mutants NOT RUN (time box). Baseline tests (golden + `viewport_layout`,
  `viewport_layout_props`, `render_geometry`) green. Proposed: **stay Ported**.

### X-viewer-state
- Not mutated in this run: `viewer_state.rs` 293 mutants NOT RUN. Baseline green (`viewer_state`, `viewer_state_golden`, `_kill_gap`, `_mutation_gap`). Note the second PARITY
  row of this id already says `Done (W8-06a)` (line 575 region); the two rows disagree and the first (Ported) has no mutation verdict. Proposed: **stay Ported**; the lead should reconcile the duplicate rows.

### X-kinetic-pan
- Not mutated in this run: `kinetic_pan.rs` 284 mutants NOT RUN. Baseline green (4 golden binaries plus `kinetic_scroller` etc.). Proposed: **stay Ported**.

### X-keyid-mirror
- Not rerun. The first PARITY row already says Done (W8-05b, 15 tests, mutants killed); the duplicate Ported row at line 532 is stale. `key_id.rs` has 18 cargo-mutants
  mutants (the table is const data). Baseline `key_id` green. Proposed: **Done** (drop the Ported duplicate); no new evidence produced for it in this run beyond the green baseline.

## NOT RUN totals

`types.rs` 75, `primitives.rs` 47, `layout.rs` 196, `render_geometry.rs` 367, `viewer_state.rs` 293, `kinetic_pan.rs` 284, `key_id.rs` 18: 1280 mutants. No native tests exist for the cluster.
Nothing needs a person at the machine for the cluster itself.
