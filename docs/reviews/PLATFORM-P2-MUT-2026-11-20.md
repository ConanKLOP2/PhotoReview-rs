Status: evidence report (2026-11-20). Whole-file mutation run of the nine `pr-platform-win` `shell_viewing` files (`frame_log.rs`, `frame_stats.rs`, `pacer.rs`, `composer.rs`, `viewport_controller.rs`, `keep_warm.rs`, `prefetcher.rs`, `compare.rs`, `crossfade.rs`) and the verdict on its 7 survivors. Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=600`, `--budget 30000`.

# `pr-platform-win` P2 (`shell_viewing`): mutation verdict

## Result

Whole-file run: 186 caught, 11 unviable, 7 not caught. The 7 were rerun with the test binaries `shell_viewing_compare_crossfade`, `shell_viewing_composer`, `shell_viewing_golden`, `shell_viewing_composer_twins_native` (91 tests): 3 are caught by existing tests (they were the whole-file timeouts, a hang is detection), 4 survive and are all equivalent by code argument. No product or test change is needed and nothing is logged in `docs/PROPOSED-DECISIONS.md`.

| Class | Count |
|---|---|
| caught (whole-file run) | 186 |
| the 3 whole-file timeouts, rerun: caught | 3 |
| unviable | 11 |
| equivalent (argued below) | 4 |
| real gap / logged | 0 |

The rerun also tested the second `||` of `arm_pending_deadline` (`composer.rs:158:73`), which the name filter picked up: caught.

## Caught on rerun (the three timeouts)

| Mutant | Existing killer |
|---|---|
| `composer.rs:431` `PresentationComposer::set` -> `()` | `set_drops_a_pending_upgrade_and_replaces_current` |
| `composer.rs:438` `begin_crossfade` -> `Ok(true)` | `begin_crossfade_freezes_the_frame_that_was_actually_drawn` (first call must be `Ok(false)`), `crossfade_requested_before_the_new_image_was_drawn_freezes_nothing_new` |
| `composer.rs:453` `cancel_crossfade` -> `()` | `cancel_crossfade_releases_the_old_image_and_the_clock` |

## Equivalent (4), all argued

| Mutant | Verdict |
|---|---|
| `compare.rs:95:23` `\|\|` -> `&&` in `layout::net_min` | equivalent. The only caller is `layout::image`, which returns before `net_min` unless `content.width > 0.0 && content.height > 0.0`, so no NaN reaches the content side. The arguments are `content / f64::from(i32)` with a pixel size `> 0`, so they are never NaN (an infinite client gives `inf`, and `inf.min(inf)` is `inf`). The NaN arm of `net_min` is therefore never taken and `||` versus `&&` cannot differ. `net_max` (used by `panel`, `border`, `content`) keeps its NaN arm covered by the `NaN` client tests. |
| `compare.rs:127:53` and `:128:55` `*` -> `+` in `layout::content` | equivalent. The operands are `2.0 * BORDER_THICKNESS` with `BORDER_THICKNESS = 2.0` (a C# constant, `CompareLayout.BorderThickness`), and `2.0 * 2.0 == 2.0 + 2.0 == 4.0`. No input can tell them apart; changing the constant would also change the mutant's meaning, so the mutant is detectable only by editing the constant, which is not a code mutation. |
| `composer.rs:158:37` `\|\|` -> `&&` in `Inner::arm_pending_deadline` (first `\|\|` of the deadline guard) | equivalent. The guard is `disposed \|\| pending.is_none() \|\| token != pending_token`. With `&&`, the closure would continue where the original returns only for (a) `disposed` with `pending` still set, which cannot happen because `Inner::dispose` clears `pending` and `set_incoming` refuses with `Err(Disposed)` once disposed, or (b) not disposed, `pending` empty and the token current. In (b) the closure calls `try_promote_pending`, whose first line returns `false` for an empty `pending`, so `invalidate` is not called and nothing observable changes. The remaining checks (`token`, and the second `\|\|` at col 73, caught) carry the behaviour. |

## Notes

- A first run with a combined name filter stopped after 3 mutants (cargo-mutants exited with code 1 before the summary); the 7 survivors were rerun with one anchored filter per mutant, so this report uses the second run (8 identities: 4 caught, 4 missed).
- Builds of `pr-platform-win` take 7 to 10 minutes per mutant on this machine while other lanes run; the whole rerun took 68 minutes.
