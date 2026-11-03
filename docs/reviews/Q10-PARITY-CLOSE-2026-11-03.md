Status: Partial for all seven rows - Q10 native/RAW/release parity close, run 2026-11-04 (worktree `pc-native-release`, branch `parity/native-release`, base origin/master 84d2c151). No row is promoted.

# Q10: native RAW and release rows, parity close

Owner goal (2026-11-03): finish PARITY. Every row below needs a person, a corpus, a P-55 review or a hosted publication for its last step; this run closed every gap a machine can close. Tests added: 9 (one file, no production change). Machine: Windows 10.0.19045, 12 logical CPUs, `CARGO_BUILD_JOBS=2`, own target dirs.

## What ran

- DLLs: `libraw.dll` and `turbojpeg.dll` taken from another agent's target dir and verified by SHA-256 against the pins `native/libraw.sha256` (6A459C22...C8CD) and `native/turbojpeg.sha256` (E9BDEC69...583D); both match. They were copied to `native/x64` (gitignored) and `<target>/debug/deps` (P-50). `tools/dev/stage-native.ps1` could not be used: the C# checkout has no `native/x64`.
- `cargo test -p pr-libraw --locked -- --include-ignored --test-threads=1`, all 16 binaries: lib 1, availability 30, decoder_golden 20, decoder_logic 66, decoder_native 25, decoder_repair 9, full_decode_gate 32, memory_guard 26, native 39, private_usage_helper 1, raw_orientation_patcher 6, raw_share_delete 15, rgb_bgra_resampler 62, synthetic_raw 5, w517_repair 9, w517_verify 3: 0 failed, 0 ignored.
- `cargo test -p pr-turbojpeg --locked --no-fail-fast -- --include-ignored --test-threads=1`: lib 7, area_resampler 17, availability 14, availability_extra 6, decoder_pure 92, native 46, native_golden 8 passed; `decoder_native` 40 passed, 1 FAILED (see X-raw-native-turbojpeg).
- Strict corpus mode: `PHOTOREVIEW_RAW_CORPUS_STRICT=1 cargo test -p pr-libraw --test decoder_native -- --include-ignored`: 8 passed, 17 FAILED with "the RAW corpus is required (strict): PHOTOREVIEW_RAW_CORPUS_DIR is not a folder". Without strict mode the same 17 corpus tests return early (they are not evidence).
- Release: `release-selftest.ps1` 44 checks PASS; `release-workflow-selftest.ps1` (4 rejection cases + real workflow) PASS; `check-release-workflow.ps1` PASS (actionlint NOT RUN: not installed); `stage-native-selftest.ps1` 7 PASS; `w10-release-killers.ps1` 18 checks PASS. `cargo build --release --locked -p pr-shell` with `PHOTOREVIEW_RS_RELEASE=1 PHOTOREVIEW_RS_VERSION=0.1.0` (4m39s) then `Assert-ReleasePe` on the exe: PASS; size 8,361,472 bytes (inside `native/release-size.json` 6,032,640..12,065,280), SHA-256 8FBE62C7...72AC, exe not launched. `cargo test -p pr-shell --test build_version_info --test build_res_writer --test exe_resources`: 2 + 9 + 3 passed.
- `tools/fetch-release-native.ps1 -Verify`: fails closed as designed ("Missing/invalid reviewed SHA-256 pin: native/turbojpeg.package.sha256"; the file holds the blocking placeholder).
- cargo-deny: NOT installed locally (`cargo deny` -> no such command); not installed here (a download needs the owner's yes). Hosted evidence read in this run with `gh run view` on master run 41681822169 (head 84d2c151, my base): job `deny` success, step output "advisories ok, bans ok, licenses ok, sources ok", one warning `duplicate` (2 entries for crate `syn`); job `actionlint` success. `native-nightly.yml`: the last 4 scheduled runs (2026-10-30 .. 2026-11-03) are success. `release.yml`: never dispatched (no runs).
- Mutation (`tools/dev/mutants.sh`, `-j 1`, own target dirs, preflight `running N tests`, N > 0): about 38 minutes in total.
  - `pr-libraw/src/full_decode_gate.rs` with bin `full_decode_gate`: 54 of 54 tested in 26 min: 30 caught, 2 missed (149:9 and 384:9, the two `Debug` impls), 11 unviable, 11 TIMEOUT (209:16, 214:36, 217:21, 268:9, 295:36, 340:9 twice, 365:9, 365:21, 411:9, 419:9; a mutant that makes `enter` wait for a grant that never comes hangs the 30 s-bound tests; a timeout is not a catch).
  - New killers `crates/pr-libraw/tests/mut_q10_gate_killers.rs` (9 tests, hang-proof: blocking calls run on a thread with a 5 s bound, a stuck thread is released through its cancel token; one 300 ms `recv_timeout` absence check for 295:36 that cannot fail on the real gate). Rerun, same 54 mutants, bin `mut_q10_gate_killers` alone: 35 caught, 11 unviable, 8 missed (66:9, 271:35 twice, 309:9, 342:17, 341:30 twice, 405:9), 0 timeouts. The 8 are outside the killers' scope: the first run caught them with `full_decode_gate`. Union of the two runs (same mutant identities): 54 tested, 43 caught, 11 unviable, 0 missed, 0 timeouts. The union is two runs, not one combined run.
  - `pr-libraw/src/memory_guard.rs`, bin `memory_guard`: 46 of 46 tested in 3 min: 45 caught, 1 unviable, 0 missed, 0 timeouts.
- Gates for the changed crate: `cargo clippy -p pr-libraw --all-targets --locked -- -D warnings` clean; `rustfmt --edition 2024 --check` on the new file clean (`cargo fmt -p` fails with os error 206 on these long paths, so rustfmt on the changed file was used instead).

## Row: X-raw-native-libraw

- Gaps before: native run dated 2026-10-29 only; mutation of `full_decode_gate`/`memory_guard` not run; corpus part open (P-30).
- After: a fresh pinned-DLL run in this run: all pr-libraw binaries green (counts above; `native` is 39 now, the 2026-10-29 note said 38). 17 `decoder_native` cases are corpus-gated and fail in strict mode without the corpus. Gate mutation closed (union 43 caught / 11 unviable / 0 survivors), `memory_guard` 45 / 1 / 0.
- Gaps left: the corpus-backed decode and golden cases (X-raw-corpus); `resampler.rs` (283 mutants) and `decoder.rs` (277 listed) mutation NOT RUN.
- Proposed status: stays Partial. Text: "Partial (contract W10-N1/N2; 2026-11-04 Q10 pinned-DLL run, all 16 pr-libraw binaries green with --include-ignored: availability 30, decoder_native 25, native 39, full_decode_gate 32, memory_guard 26, 0 failed; 17 corpus-gated decoder_native cases fail in strict mode without the corpus and return early otherwise; full_decode_gate.rs 54 mutants 43 caught + 11 unviable after killers `mut_q10_gate_killers`, memory_guard.rs 45 caught + 1 unviable; Q10 evidence file)".

## Row: X-raw-native-turbojpeg

- Gaps before: row text says `decoder_native` 41 of 41.
- After: `decoder_native` is 40 passed, 1 FAILED on this machine, reproducibly (4 runs): `flavour_decodes_consistently` panics at `arithmetic-444: WicDirect decode failed: WIC call failed (0x88982F60)` (WINCODEC_ERR_BADIMAGE). The cause is the OS, not the Rust code: the JPEG made by libjpeg-turbo (dumped to a file) is refused by .NET WPF `BitmapDecoder.Create` on this machine ("The image decoder cannot decode the image"), while the non-arithmetic 4:4:4 file decodes. The C# test `Flavour_DecodesConsistently` has the same chain "WicDirect->Wpf" with no skip, so it would fail the same way on Windows 10.0.19045; the 2026-10-29 41/41 run therefore ran on a machine or OS build whose WIC decodes arithmetic JPEG (not established which). With a temporary local skip of that one flavour (reverted, not committed) the other 17 flavours pass (cmyk and ycck: TurboJpeg declines, as in C#). Everything else is green: lib 7, area_resampler 17, availability 14, availability_extra 6, decoder_pure 92, native 46, native_golden 8.
- Gaps left: the arithmetic flavour on a Windows build whose WIC decodes arithmetic JPEG (or an owner decision on a skip, see proposals); `area_resampler.rs` 130 and `decoder.rs` 333 mutants NOT RUN.
- Proposed status: stays Partial; replace "41 of 41" with "2026-11-04 Q10: 40 of 41, `flavour_decodes_consistently` fails on arithmetic-444 because WIC of Windows 10.0.19045 cannot decode arithmetic JPEG (the same chain fails in C#); the other 17 flavours pass".

## Row: X-raw-corpus

- Gaps before: corpus absent.
- After: still absent (checked the folder tests/fixtures/raw-corpus in this worktree and in the main checkout, and the C# `tests/Fixtures/raw-corpus`). Not fetched: P-30 needs the owner's yes. Strict mode proven to fail instead of skip (17 failures above). No change.
- Proposed status: stays Partial; add the Q10 strict result.
- Steps for a person (about 590 MB, CC0, from raw.pixls.us, hash-pinned in the C# manifest): (1) say yes to P-30. (2) The Rust repo has no copy of the fetcher; run the C# one read-only from this repo root: `powershell -NoProfile -ExecutionPolicy Bypass -File ..\PhotoReview\tools\fetch-raw-samples.ps1 -SamplesFile ..\PhotoReview\tools\raw-samples.txt -TargetDir tests\fixtures\raw-corpus` (25 rows; it checks each file URL, the CC0 label and the SHA-256, fail closed). Alternatively port the script into `tools/` first. (3) `set PHOTOREVIEW_RAW_CORPUS_STRICT=1`, stage the DLLs (P-50), then `cargo test -p pr-libraw --test decoder_native -- --include-ignored --test-threads=1`, and the corpus tests of `pr-imaging-raw` (`cr3_reader`, `raf_reader`, `raw_corpus_presence`, `raw_exif`, `raw_reader_arw`, `readers_cr2`, `w5_18_integration`; some files read `PHOTOREVIEW_RS_RAW_CORPUS_STRICT` instead). (4) Regenerate the corpus goldens with `tools/oracle.ps1` and compare. (5) Report counts with 0 skipped.

## Row: X-raw-mutation-w5

- Gaps before: historical identities incomplete; `full_decode_gate`, `memory_guard` not run; others listed in the row.
- After: `full_decode_gate.rs` 54 and `memory_guard.rs` 46 are now fully run with 0 survivors (numbers above).
- Gaps left: `resampler.rs` 283 and `decoder.rs` 277 listed (libraw), `area_resampler.rs` 130 and `decoder.rs` 333 (turbojpeg; both re-listed this run), ARW 74, cr2 15, `preview_selector.rs` shard 1/2 120 (counts of the last three from earlier evidence). The original 277-mutant survivor identities stay unreconstructed. About 20 of the 60 minutes were left unused because none of the remaining files fits in them (283 mutants at 10 s or more each, and the turbojpeg resampler test takes 15 s per mutant).
- Proposed status: stays Partial.

## Row: X-release-workflow

- Gaps before: hosted dispatch/publication, local actionlint, P-55.
- After: selftests re-run green (44 + 4 cases + 7 + 18); the actionlint job is green on master 41681822169 (hosted); local actionlint still not installed. `release.yml` has never run on the host.
- Gaps left: a dispatch of `release.yml` on a real tag (needs P-55 closed first, a person with write access), local actionlint.
- Proposed status: stays Partial.

## Row: X-release-dependencies

- Gaps before: "audit output was not reviewed by the owner", local run absent.
- After: the hosted `deny` job on master 41681822169 read in this run: advisories, bans, licenses, sources all ok, one warning (duplicate `syn`, 2 entries). Local cargo-deny still not installed, so no local run.
- Gaps left: a person reads the audit output and accepts the duplicate-`syn` warning; optional local run: `cargo install cargo-deny --version 0.19.9 --locked` then `cargo deny --locked check --config deny.toml advisories licenses bans sources --deny unmaintained --deny unsound`.
- Proposed status: stays Partial (hosted pass on the base commit; local run absent).

## Row: X-release-native

- Gaps before: only fixture tests; offline release build last checked 2026-10-30.
- After: the offline release build and `Assert-ReleasePe` re-run on a fresh build of base 84d2c151 (details above), release selftest 44, killers 18, stage selftest 7, `-Verify` fails closed as designed. No real package fetch.
- Gaps left (person): fetch and review the TurboJPEG NuGet package and put its SHA-256 in `native/turbojpeg.package.sha256`; reviewed TurboJPEG notices (IJG/zlib) and LibRaw source offer; root LICENSE; validate English and Vietnamese in the built release and flip the four booleans of `native/release-review.json`; remeasure the exe size at the real tag; real package/source archive; SmartScreen; P-55.
- Proposed status: stays Partial.

## Proposals (not applied)

1. `flavour_decodes_consistently` on a Windows build without arithmetic JPEG support in WIC: keep as is (faithful to C#, fails on this OS build) or let the Rust test skip the WicDirect chain for `arithmetic-444` when WIC answers 0x88982F60 (a deviation from C#, owner decision). No production change is needed either way.
2. Record in `docs/PARITY.md`, row X-raw-native-turbojpeg, that a green `decoder_native` depends on the OS build.

## NOT RUN

- cargo-deny locally; actionlint locally; hosted `release.yml` dispatch; real package fetch; RAW corpus (all corpus-gated cases); mutation of libraw `resampler.rs` (283) and `decoder.rs` (277), turbojpeg `area_resampler.rs` (130) and `decoder.rs` (333), ARW (74), cr2 (15), preview_selector shard 1/2 (120); `cargo fmt -p` (os error 206); full-workspace tests.
