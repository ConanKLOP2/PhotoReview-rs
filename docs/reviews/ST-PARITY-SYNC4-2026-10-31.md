# ST-PARITY-SYNC4 (2026-10-31): PARITY, ADR and task-file sync for PRs #476..#487

Status: report of lane `st-parity-sync4`. Docs only; no production code, no build, no test run. `git log origin/master` shows #475..#487 merged (HEAD `ca9bac16`). Every note was checked against the current row first (`grep` for the PR numbers, `catch_job`, `pending_opens`, `with_log`, `0x1100`, `AUTOLOGON` found nothing in PARITY before this lane), so nothing was duplicated. Source reports: `FIX-SETTINGS-STORE`, `ST-HTTP-SECURITY`, `FIX-HTTP-AUTOLOGON`, `ST-TEST-HYGIENE`, `FIX-TEST-HYGIENE`, `FIX-RECYCLE-TRUNG`, `FIX-RECYCLE-C1C2`, `ST-FFI-LIBS`, `FIX-FFI-HARDENING`, `FIX-PANIC-REST`, `ST-SHELL-REVIEW`, `FIX-SHELL-LIFECYCLE`, `FIX-IMAGING-LOW`, all in `docs/reviews/`.

## PARITY rows changed (25, no status change; `parity.py stats` before and after: Done 183, Partial 102, Ported 38, Superseded 17, Different 14, Todo 4, Implementation 3)

| Row | Change |
|---|---|
| S-64, INV-11, X-config-import | #477 stat failure: probe with a read, never overwrite (Different from C# `File.Exists`), ADR DV-13 |
| X-http-transport | #478/#484: `AUTOLOGON_POLICY=HIGH` Rust-only hardening, `Location` control characters, non-loopback http test (DV-14); still NOT proved: no-NTLM rig, TLS minimum (L2), proxy credentials in URL refused; M2 repo URL undecided |
| X-http-composition | M2 reference |
| I-13 (container readers), I-13 (CR2), X-cr3-reader, I-14 / X-preview-selector, RawDecoder routing row | #479 F1: corpus cases of `raw_reader_arw`, `readers_rw2`, `raw_nef`, `readers_cr2`, `cr3_reader`, `raw_exif`, `w5_18_integration` are "data-gated, false green on CI"; not fixed by #485 |
| X-release-workflow | F2 (about 1483 ignores, about 171 in nightly), +4 `pr-imaging` binaries in nightly (#485, unconfirmed on `windows-latest`), F18 unsafe grandfathered |
| P-03 | #480: T5 fixed, T4 partly (the "not in the module tree" sentence superseded), T1 deferred, T2 as C#; `recycle_bin_shell`/`restore_verb` stay below Done (DV-16) |
| I-15b, I-15, I-02b | #486: 0x1100 load flags (DV-15), NUL, `has_decode_exports`; M2/M3/M4 deferred |
| X-zoom-detail, X-image-presenter, X-sibling-navigator, X-main-view-model | #482 `catch_job` (DV-17); ST-APP-RACES M3 closed, M4/M5 open |
| X-startup-options-run, X-startup-composition-root, X-main-window-lifecycle, F-23 | #487: `stop_listening` before the box (Different), bare version (Different), `pending_opens`, explicit `SessionWriter` dispose (DV-18..DV-21); M6 equal to C#; L1..L8 open |
| I-10 / INV-2 / INV-8, P-08 / INV-10 / D-08 | #476: L1 deferred, L3/L4/L5/L6 equal to C# |

`parity-closeout` and `TRIAL-PLAN` numbers are unchanged (no status moved), so neither file was edited.

## ADR

Nine items appended to `docs/adr/ADR-D-2026-10-31-intentional-differences-from-fix-lanes.md` (now DV-1..DV-21; header, "Why one file" and the DV-8 remaining-risk sentence updated), each with context, C# file:line (read at `7eaee2a2`), Rust, reason, remaining risk and tests: DV-13 #477, DV-14 #478/#484, DV-15 #486, DV-16 #480, DV-17 #482, DV-18..DV-21 #487 (stop_listening, bare version, pending_opens, SessionWriter dispose). Proposal row `P-59` added to `docs/PROPOSED-DECISIONS.md`; `docs/adr/README.md` and `docs/INDEX.md` updated.

DV-16 is a weaker "difference": `with_log` restores the C# observable behaviour with an extra constructor; it is recorded mainly so the `recycle_bin_shell`/`restore_verb` rows are not read as Done.

## Questions waiting for the owner

Each is open in the rows above and acted on with the conservative default until answered.

1. **Update repository (ST-HTTP-SECURITY M2).** `checker.rs:17`, `version.rs:99`, `version.rs:187` point at the C# repository. Options: (a) keep, the Rust app announces C# releases (wrong binary offered); (b) point at the Rust repository once it publishes releases (needs the release workflow to produce them); (c) make it a build-time constant with a test that the three agree. Recommend (c) after (b) is decided. Until then: unchanged, no test pins the value.
2. **Real host for the bench binary (ST-BENCH-BACKEND).** `pr-platform-win -> pr-app -> pr-bench` blocks naming WIC, memory probe and the Recycle Bin in the `pr-bench` binary. Options: (a) a host crate or `pr-app` feature that builds the CLI with a Windows host (keeps one binary, adds a crate); (b) move the binary out of `pr-bench` (clean graph, moves a published CLI path). Recommend (a). Until then `UnwiredEngineHost`, no performance claim.
3. **RAW share-delete (INV-8, DV-9 M4 note).** `libraw_open_wfile` opens without `FILE_SHARE_DELETE`, so renaming or deleting the RAW fails while it decodes (inherited from C#). Option: read the RAW through `open_read_shared` into memory then `open_buffer`; costs RAM budget and the read-once rule. Needs an ADR; decide go or no-go.
4. **`unsafe` policy for `pr-core` and `pr-imaging` (F18).** Their `unsafe` (15 production sites plus 12 in one test) is grandfathered in `tools/dev/unsafe-audit.baseline` as rule B. Options: add both crates to the allowed list with `// SAFETY:` checks (honest, a policy change in AGENTS.md); keep grandfathered (no change); move the sites into `pr-platform-win` (large refactor). Recommend the first.
5. **COM progress sink C2 (FIX-RECYCLE-C1C2).** Without `IFileOperationProgressSink::PostDeleteItem` the app cannot prove a file reached the bin. The design is in the report (7 steps, seam change `ShellRecycleOutcome`); it needs an attended native run with a self-made temp file, never in CI. Decide who runs it.

## Not done

No status raised. Nightly additions (#485) are not confirmed on `windows-latest`. F1 (RAW corpus false green) is only recorded, not fixed (options in `ST-TEST-HYGIENE` F1: `#[ignore]` the corpus cases or set STRICT in nightly).
