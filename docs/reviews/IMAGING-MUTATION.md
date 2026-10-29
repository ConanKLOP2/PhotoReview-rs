# Imaging mutation debt, 2026-10-29

Worktree `claude-mut-imaging`, branch `feat/claude-mut-imaging`, starting HEAD
`83da367f921664c7a26af70f4568a4b44f22d210`. Test code, test-only dependency/lockfile
wiring and documentation; no production behavior change, commit, push, native
decode, corpus download or C# edit.
The owner selected focused pure logic, not a whole-file mutation verdict.

## Method and reproducibility

Read `AGENTS.md` (mutation rules), `docs/LESSONS.md` and
[`W10-N4`](../../contracts/W10-N4.md). Cargo-mutants 27.1.0 uses one worker and
one mutation process at a time. Normal tests use `$PWD/target`; mutation uses
`$PWD/target-mut`. Both use `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and
`CARGO_INCREMENTAL=0`. Each mutation invocation is bounded by `timeout 1200`.
Never run ordinary tests in the mutation target.

The new RAW killer binary has a dev-dependency on `pr-libraw` (one lockfile
dependency edge; no version change). LibRaw survivors are rerun with mutation
package `-p pr-libraw`, `--test-package pr-imaging-raw` and
`--cargo-arg=--test=raw_mutation_killers`. The fake API panics if native init or
error formatting is called. The handoff test uses a 2x3 pixel allocation and
distinct 4000x6000 original dimensions. Parser cases use a channel receive
timeout of five seconds as a hang guard; there are no sleeps or timing assertions.

Common invocation (Git Bash, repository root; prepend `$HOME/.cargo/bin` to PATH):

```bash
export CARGO_TARGET_DIR="$PWD/target-mut"
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0
timeout 1200 cargo mutants -p CRATE --baseline skip --timeout 90 -j 1 \
  --file '**/FILE.rs' -F 'FILTER' --cargo-arg=--test=BINARY \
  --output "$PWD/mut-out-GROUP"
```

The baseline test binary must pass before a run. Inspect caught logs as well as
counts: an unrelated failing test cannot establish a catch. Version 27.1.0 also
emits struct-field deletions outside a function regex; these are accounted for,
not silently dropped. Exact listings and outcomes remain in each local
`mut-out-*/mutants.out/{mutants.json,outcomes.json,caught.txt,missed.txt,unviable.txt}`.
Artifacts and build directories are not source deliverables.

For the common command above, these are the exact selections (`**/` prefixes
apply to every file; multiple files mean repeated `--file` arguments):

| Group | Crate | Files | `-F` regex | Test binary / additional arguments |
|---|---|---|---|---|
| memory | pr-imaging | memory_headroom.rs | omitted | imaging_mutation_killers |
| key-valid | pr-imaging | cache_key.rs | `matches_current_source` | imaging_mutation_killers |
| cache-service | pr-imaging | cache/thumbnails.rs, service/decode.rs | `build_stamp\|build_key\|cache_file_path\|source_bytes_read` | imaging_mutation_killers |
| cache-rerun | pr-imaging | cache/thumbnails.rs | `cache_file_path\|delete.field` | imaging_mutation_killers |
| adaptive | pr-imaging | adaptive_preview.rs | `calculate_target_decode_box\|quantize\|validate` | imaging_mutation_killers |
| rw2-valid | pr-imaging-raw | readers/rw2.rs | `image_extent\|ifd0_orientation\|exif.*length` | raw_mutation_killers |
| readers | pr-imaging-raw | readers/nef.rs, readers/dng.rs, readers/orf.rs, readers/raf.rs | `can_read\|choose_sensor_size` | raw_mutation_killers |
| libraw-guard | pr-libraw | memory_guard.rs | `classify\|is_bayer\|estimate_peak_bytes` | memory_guard |
| libraw | pr-libraw | decoder.rs | `find_end_of_image_by_structure\|find_trailing_eoi_with_structure_free_tail` | decoder_repair |
| libraw-killers-valid | pr-libraw | decoder.rs | same as libraw | raw_mutation_killers; `--test-package pr-imaging-raw`; fresh metadata flag above |
| libraw-final | pr-libraw | decoder.rs | exact escaped names of the four remaining non-equivalent misses from lines 752/757 | raw_mutation_killers; same test-package and metadata flag; incidental five handoff field deletions also generated |
| service | pr-imaging | service/decode.rs | `decode_and_cache\|source_bytes_read` | imaging_mutation_killers |
| service-final | pr-imaging | service/decode.rs | `decode.rs:420:31:.*with.true` | imaging_mutation_killers; OOM regression |
| key-lib | pr-imaging | cache_key.rs | `matches_current_source` | imaging_mutation_killers plus `--cargo-arg=--lib` |

`CARGO_BUILD_JOBS=2` is set from the valid RW2 rerun onward. Every cargo-mutants
run still uses `-j 1`. Only `cache_key.rs` among the selected files has in-file
`#[cfg(test)]` tests; `key-lib` covers that second selection in addition to the
killer-only proof. No normal test command runs in `target-mut`.

## Current run ledger

| Output group (`mut-out-`) | tested/selected | caught | missed | unviable | timeout | not run |
|---|---:|---:|---:|---:|---:|---:|---:|
| `memory` | 23/23 | 21 | 0 | 2 | 0 | 0 |
| `key-valid` | 10/10 | 10 | 0 | 0 | 0 | 0 |
| `cache-service` | 16/16 | 10 | 6 | 0 | 0 | 0 |
| `cache-rerun` | 6/6 | 4 | 2 | 0 | 0 | 0 |
| `adaptive` | 32/32 | 31 | 1 | 0 | 0 | 0 |
| `rw2-valid` | 31/31 | 30 | 1 | 0 | 0 | 0 |
| `readers` | 36/36 | 36 | 0 | 0 | 0 | 0 |
| `libraw-guard` | 43/43 | 42 | 0 | 1 | 0 | 0 |
| `libraw` | 77/77 | 36 | 33 | 0 | 8 | 0 |
| `libraw-killers-valid` | 77/77 | 66 | 11 | 0 | 0 | 0 |
| `libraw-final` | 9/9 | 8 | 1 | 0 | 0 | 0 |
| `service` | 15/15 | 9 | 2 | 4 | 0 | 0 |
| `service-final` | 1/1 | 1 | 0 | 0 | 0 | 0 |
| `key-lib` | 10/10 | 10 | 0 | 0 | 0 | 0 |

Rows overlap. Latest verdict per full identity: **280/280**; **261 caught,
12 missed (11 equivalent and 1 unobservable through the pure seams), 7 unviable,
0 timeouts, 0 selected identities pending**. The unobservable service I/O guard
is still a verification gap, not a clean whole-file verdict. Discarded runs are
excluded. The new tests catch 31 behavior-distinct initial misses (4 thumbnail,
26 LibRaw, 1 service) and all 8 initially timed-out LibRaw mutants.

Full-file listings are an inventory, not full-file runs:

| File | tested unique | full listing | not selected |
|---|---:|---:|---:|
| `crates/pr-imaging-raw/src/readers/dng.rs` | 8 | 94 | 86 |
| `crates/pr-imaging-raw/src/readers/nef.rs` | 7 | 126 | 119 |
| `crates/pr-imaging-raw/src/readers/orf.rs` | 7 | 109 | 102 |
| `crates/pr-imaging-raw/src/readers/raf.rs` | 14 | 82 | 68 |
| `crates/pr-imaging-raw/src/readers/rw2.rs` | 31 | 50 | 19 |
| `crates/pr-imaging/src/adaptive_preview.rs` | 32 | 33 | 1 |
| `crates/pr-imaging/src/cache/thumbnails.rs` | 13 | 83 | 70 |
| `crates/pr-imaging/src/cache_key.rs` | 10 | 44 | 34 |
| `crates/pr-imaging/src/memory_headroom.rs` | 23 | 23 | 0 |
| `crates/pr-imaging/src/service/decode.rs` | 15 | 68 | 53 |
| `crates/pr-libraw/src/decoder.rs` | 77 | 277 | 200 |
| `crates/pr-libraw/src/memory_guard.rs` | 43 | 46 | 3 |

Across these file listings: 1035 candidates, 280 tested, 755 not run. Other files were not inventoried or mutated.

## Survivor dispositions

- `cache/thumbnails.rs:686` returning an empty `PathBuf`, and field deletions at
  lines 732/735/736 (`downscaled`, `original_width`, `original_height`), survived
  the first killer-only run. The new
  `thumbnail_disk_decode_preserves_metadata_and_cache_path` catches all four in
  the six-identity rerun using `imaging_mutation_killers` alone.
- `cache/thumbnails.rs:733` deleting `orientation: 1` and line 734 deleting
  `actual_backend: WicDirect` remain MISSED but are equivalent: both values equal
  `DecodedImageOptions::default()` (`decode_types.rs:358-369`). No corpus changes
  that identity. Counts retain these as missed; they are not counted as catches.
- `memory_headroom.rs:40` replacing `MemoryInfo` with `Default::default()` is
  unviable: the type has no `Default` implementation. Line 51 `/` to `*` is
  unviable because `i64::MAX * 2` overflows at compilation.
- The first cache-key run (`mut-out-key`) is **discarded**, including all its
  reported catches: its source snapshot contained a failing service test. The
  shared fake does not forward its script's `source_bytes_read` field. The test
  now checks fallback to source length; a fresh green-baseline run uses
  `mut-out-key-valid`. No result from the discarded run enters totals.
- One initial PowerShell/Git Bash regex invocation failed to preserve `|` quoting
  and terminated before usable outcomes; it supplies no mutation evidence.
- `mut-out-libraw-killers` is **discarded** in full (including its 77 reported
  catches): the dependency `pr-imaging-raw` reused an earlier mutated RAF library
  from the mutation target. The unrelated sensor-choice test failed even on
  equivalent LibRaw mutants. `mut-out-libraw-killers-valid` keeps the required
  `target-mut` directory but sets
  `RUSTFLAGS='-Cmetadata=imaging-mutation-libraw-replay-v2'` to force a fresh
  dependency build. No ordinary Cargo test or target cleanup was used in
  `target-mut`. Caught logs must name the relevant LibRaw killer, not the RAF
  test. The initial `decoder_repair` run never calls the RAW reader and is not
  adjudicated by that unrelated sensor test.

- `adaptive_preview.rs:59` `<` to `<=` at the minimum long side is equivalent:
  at exactly 1200 the added scale is exactly 1 for both dimensions.
- `readers/rw2.rs:83:22` `>` to `>=` for the size tag is equivalent: zero
  converts to zero, equal to the original fallback.
- LibRaw `decoder.rs`, latest identity verdict **70 caught / 7 equivalent** of
  77 selected. Initial `decoder_repair` results were 36 caught, 33 missed and
  8 timeouts. The fresh dependency replay using the RAW killer alone caught 66
  and missed 11, with no timeout; four more misses were killed by direct tail
  cases (EOI beyond the midpoint, forbidden marker immediately after EOI,
  forbidden marker before EOI, and a terminal lone FF). The final nine-identity
  run caught those four plus four handoff fields, leaving orientation equivalent.
  Thus all 26 behavior-distinct initial misses and all eight initial timeouts
  have a caught rerun with the new binary alone. No production parser change.
- The seven final LibRaw equivalents are:
  `507:13` orientation field deletion (default is 1);
  `708:18` `>` to `>=` (exactly two remaining bytes can contain only a length,
  never the required EOI, and both routes return None);
  `711:55` OR to XOR (high byte shifted eight bits and low byte have disjoint bits);
  `716:14` `>` to `==` and `>` to `>=` (an offset at or beyond EOF returns None
  at the next outer/entropy guard regardless);
  `748:12` `<` to `==` and `<` to `<=` (the public caller rejects lengths below
  four, so this private fallback never sees length two or less).
- LibRaw `memory_guard.rs:71` replacing `classify` with `Default::default()` is
  unviable because `RawBufferFamily` has no Default implementation. The other
  42 selected classifier/estimate mutants were caught by `memory_guard`.
- Service `decode.rs:420:31`, replacing the cache-entry-failure guard with true,
  is caught 1/1 by the new fake OOM payload decoder. The error propagates, no
  source decode runs, and the intact disk entry remains byte-identical.
- The four service unviable identities are `404:9` (PreviewResult has no Default)
  and `459:13`/`460:13`/`461:13` (`&&` to `||` is illegal inside the let chain).
- Service `decode.rs:417:33`, replacing `disk_entry_exists` with true, remains
  **unobservable through the current pure seams**, not globally equivalent.
  On a stable missing cache entry, the extra open returns NotFound, the failure
  is treated as a miss, `try_delete` first checks file existence and does
  nothing, and the same source decode/result/metrics follow. Existing entries
  take the same path. It adds unnecessary disk I/O; creation between the
  existence check and open can also change a race result. The check and open
  directly use the OS filesystem, with no injectable pre-open observation
  seam. No native I/O trace or nondeterministic race test is invented here.
  This observation gap stays open and W4-14b remains Partial.
- `mut-out-rw2` is discarded as environment failure: the first build reports
  `STATUS_DLL_INIT_FAILED`, and subsequent Cargo launches return -1073741502
  (the same Windows status). All 31 tool-reported unviable outcomes are invalid
  mutation verdicts. A green normal baseline and `CARGO_BUILD_JOBS=2` precede
  the successful `mut-out-rw2-valid` run.

## Historical W10-N4 reconciliation and remaining limits

- The original 277-mutant W5-16c run and its ten survivor identities are not
  reconstructed by adding counts from overlapping runs. The contract and
  historical ledger remain the provenance of that gap.
- Current `raw_decoder_core` passes 3 tests and reports **63 executed steps**:
  16 read-info, 7 early-error, 40 scripted image decode; **0 gated cases and 0
  gated steps**. This resolves the contract's uncertainty about the seven
  identity/rewrite golden gates for the current fake-backed harness. It does
  not establish actual RAW pixel decoding.
- NEF's historical 126/126 catch, DNG's 89 caught/2 equivalent/3 unviable, ORF's
  reviewed equivalents, and TurboJPEG's overlapping 162-candidate reconciliation
  are historical evidence in [`W10-N4`](../../contracts/W10-N4.md) and the
  [Wave 5 archive](../archive/task_on_progress-wave5.md). They are not added to
  fresh totals. TurboJPEG is outside the three crates selected by the owner.
- Native/FFI glue, full RAW decoding, native buffer pinning/cleanup, actual
  corpus orientation rewrites, demosaic/headroom integration, repeated native
  decode memory growth, and ORF thumbnail extraction cannot be judged through
  corpus-gated tests that return early. No new verdict is claimed for mutants
  in those paths; none is labeled caught from a vacuous pass. The prior
  [native report](RAW-NATIVE-DLL-2026-10-29.md) records 17 such `decoder_native`
  and 7 `native` LibRaw cases. No `--include-ignored` or strict corpus run here.
- W4-10b retains unwired native codec twins and its documented behavior
  differences. W4-14b's diagnostic pre-read `>` to `>=` boundary requires about
  2 GB to distinguish through the current seam and is not selected. Its
  `WithSourceBytes` busy/cancel retry gap is unchanged. Focused pure checks cannot
  turn these rows Done.

## Validation and terminal lines

Final checks run with normal `CARGO_TARGET_DIR=$PWD/target`, separately from
mutation. Each command is bounded by `timeout 240` (the small baseline commands
also used 180 seconds). Fmt uses one package per command.

| Command | Last nonempty line / result |
|---|---|
| `cargo fmt -p pr-imaging --check` | no output, exit 0 |
| `cargo fmt -p pr-imaging-raw --check` | no output, exit 0 |
| `cargo fmt -p pr-libraw --check` | no output, exit 0 |
| `cargo clippy -p pr-imaging --all-targets --locked -- -D warnings` | Finished dev profile in 0.39s, exit 0 |
| `cargo clippy -p pr-imaging-raw --all-targets --locked -- -D warnings` | Finished dev profile in 0.30s, exit 0 |
| `cargo clippy -p pr-libraw --all-targets --locked -- -D warnings` | Finished dev profile in 0.15s, exit 0 |
| `cargo test -p pr-imaging --locked --test imaging_mutation_killers` | test result: ok. 10 passed; 0 failed; 0 ignored (0.02s) |
| `cargo test -p pr-imaging-raw --locked --test raw_mutation_killers` | test result: ok. 7 passed; 0 failed; 0 ignored (0.01s) |
| `cargo test -p pr-imaging-raw --locked --test raw_decoder_core -- --nocapture` | test result: ok. 3 passed; 0 failed; 0 ignored (0.20s); log: 63 executed steps, 0 gated |
| `cargo test -p pr-imaging --locked --lib` | test result: ok. 150 passed; 0 failed; 0 ignored |
| `cargo test -p pr-libraw --locked --test memory_guard --test decoder_repair --lib` | each binary green: memory_guard 26, decoder_repair 9, lib 1; 0 failed/ignored |
| `python -I tools/dev/doc-links.py --quiet` | doc-links: files=450 errors=0 proposed=171 archive_unchecked=14 csharp_refs=503 runtime_paths=13 placeholders=104 |
| `git diff --check` | no output, exit 0 |

The final nonempty cargo-mutants summary lines (groups refer to the command
table above; elapsed times are from console output) are:

```text
memory: 23 mutants tested in 4m: 21 caught, 2 unviable
key-valid: 10 mutants tested in 74s: 10 caught
cache-service: 16 mutants tested in 86s: 6 missed, 10 caught
cache-rerun: 6 mutants tested in 44s: 2 missed, 4 caught
adaptive: 32 mutants tested in 3m: 1 missed, 31 caught
rw2-valid: 31 mutants tested in 2m: 1 missed, 30 caught
readers: 36 mutants tested in 2m: 36 caught
libraw-guard: 43 mutants tested in 2m: 42 caught, 1 unviable
libraw: 77 mutants tested in 15m: 33 missed, 36 caught, 8 timeouts
libraw-killers-valid: 77 mutants tested in 4m: 11 missed, 66 caught
libraw-final: 9 mutants tested in 36s: 1 missed, 8 caught
service: 15 mutants tested in 48s: 2 missed, 9 caught, 4 unviable
service-final: 1 mutant tested in 7s: 1 caught
key-lib: 10 mutants tested in 57s: 10 caught
```

The adaptive shell wrapper reported a parse error after the completed mutation
summary because its script was edited while Bash was running. All 32 terminal
outcomes, the end timestamp, and their relevant failing-test logs are present;
the wrapper error is not treated as a mutation verdict or hidden as exit 0.

NOT RUN: 755 unselected candidates in the file inventory, other source files,
workspace fmt/Clippy/tests, native/FFI mutation, `--include-ignored`, strict RAW
corpus, native pixel decode, performance, real Recycle Bin or GUI verification.
The corpus directory is absent (`Test-Path tests/fixtures/raw-corpus` is False).
No C# oracle regeneration or C# tests; no commit or push. Seven named project
files changed: two killer sources, RAW Cargo.toml and Cargo.lock test wiring,
this review, PARITY and task status. Local mutation artifacts are retained and
must not be added to Git.
