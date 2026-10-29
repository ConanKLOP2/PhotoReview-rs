# CX-RELEASE offline verification, 2026-10-30

Branch `feat/cx-release`, worktree `cx-release`, starting HEAD
`47c44dc6843c343cdc85da0d784b2b0258142fc4`. Owner lane date is October 30;
command timestamps are October 29 UTC. No add/commit/push, hosted dispatch,
publication, native-package download, dependency or production-code change.
C# `7eaee2a2:src/PhotoReview.Core/Updates/UpdateChecker.cs` was read with `git show`.

Read first: AGENTS.md:126-136, LESSONS.md:1-73, PARITY lane rows,
the dated CX review inventory (no earlier CX-RELEASE existed), and the relevant
P-09 mutation section of W2-FS-EVIDENCE-2026-10-29.md. CX-CORE2 covers the separate
settings JSON reader; it does not close the update-checker scanner gap.

## Row map and local boundaries

| Row | Source / covering tests | Remaining boundary |
|---|---|---|
| X-release-workflow | `.github/workflows/release.yml`, `tools/check-release-workflow.ps1`, release staging/packing scripts; `release-selftest.ps1`, `release-workflow-selftest.ps1` | Hosted dispatch/publication and P-55 review gates remain unverified; actionlint unavailable |
| X-release-dependencies | `deny.toml`, `.github/workflows/ci.yml` | cargo-deny is not installed (`cargo deny --version` exits 101); no install or advisory fetch attempted |
| X-release-native | native pins/review policy, `fetch-release-native.ps1`, `verify-release-native.ps1`, `release-pe.ps1`; release/stage selftests and new `w10-release-killers.ps1` | Real release package/PE, source archive, notices/licence/languages approval and P-55 remain open |
| X-http-transport | `pr-platform-win/src/http/{policy,transport}.rs`; `http_policy`, `http_transport_loopback`, lib `http::transport::tests` | Oracle defaults, PAC/proxy/trust rig, HTTPS downgrade and private helper coverage remain open; raw WinHTTP callback/FFI glue excluded from mutation |
| X-http-composition | `pr-shell/src/composition/{update_http,graph}.rs`, startup secondary-dialog adapter; `update_http_composition`, `composition_graph` | Actual Settings close/late-poster/ShellExecute remains unverified; composition glue excluded from mutation |
| P-09 local | `pr-core/src/updates/checker.rs`; `updates_checker`, `w2_fs_mutation_killers`, new `w10_release_killers` | Historical partial mutation and HTTP/desktop boundaries are kept explicit |

## Offline checks

- `release-selftest.ps1`: **44 checks pass**, including fake gh against the real
  inline preflight, hash/archive/review/layout/packing rejection cases. Packaging
  fixture still substitutes PE validation; this is not actual release packaging.
- `release-workflow-selftest.ps1`: **4 rejection cases + real workflow pass**,
  PyYAML syntax/dispatch-only trigger pass. **actionlint NOT RUN**, absent.
- `stage-native-selftest.ps1`: **7 checks pass**.
- `stage-native.ps1`: both local DLLs match C# baseline hashes and were staged
  to this worktree's target/debug, target/debug/deps and native/x64. Git first
  rejected ownership; process-local safe.directory entries for the exact worktree
  and read-only C# repo allowed the read. No global Git configuration changed.
- `cargo metadata --locked --offline --format-version 1`: **38 packages**, 26
  registry and 12 workspace/path, no missing licence field. This inventory is
  **not** a cargo-deny advisory/licence/bans/sources verdict.
- `updates_checker`: **34 pass**; `w2_fs_mutation_killers`: **16 pass / 1 ignored**
  (the ignored test belongs to another scope).
- `http_policy`: **4 pass**; `http_transport_loopback`: **18 pass / 2 ignored**;
  lib HTTP helpers: **3 pass**. Loopback uses local sockets only.
- `composition_graph`: **3 pass / 1 ignored**; `update_http_composition`:
  **9 pass / 1 ignored**. The old graph assertion now requires Some(checker) in
  this checkout; the PARITY note saying it still expects None was stale.
- New `w10_release_killers`: **2 pass**, pure inputs, no clock or sleep.
- New `tools/dev/test/w10-release-killers.ps1`: **18 checks pass** using synthetic
  PE bytes (resource tree, MZ/PE/AMD64/PE32+ signatures, inclusive section start,
  exclusive section end, unmapped RVA, truncated reads). No executable is loaded.
  Six manual single-operator mutants in scratch copies are caught by this tool
  test alone: remove each of four signature guards and change RVA `>=` to `>`
  or `<` to `<=`. Logs `target/pe-mutant-*.log`; these six are a selected manual
  sample, not an exhaustive PowerShell mutation inventory. Truncation tests do
  not prove the explicit bounds guard is necessary (runtime reads also throw).

## Mutation method and evidence

`CARGO_NET_OFFLINE=true`, `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=line-tables-only`, build jobs 2. Normal commands use
`CARGO_TARGET_DIR=<worktree>/target`; cargo-mutants uses `<worktree>/target-mut`
and `--output <worktree>/mut-out-GROUP`. One owned mutation process at a time;
outer budget 1200 seconds per file, per-mutant timeout 90, jobs 1, baseline skip.
Normal tests never use target-mut. Tests do not touch user config or Recycle Bin.

`policy.rs` has no unit tests. Its full `--list` inventory matches the run's
**19 Found**: **19/19 tested, 16 caught, 0 missed, 3 unviable, 0 timeout**,
`mut-out-http-policy/mutants.out`. The three compile-unviable return replacements
require nonexistent Default implementations for RedirectDecision, ProxySource
and HttpRequestError; logs show E0277, not infrastructure launch failures.

The checker `Scanner::number` inventory matches **13 Found** in each invocation:
initial `updates_checker` **13/13 tested, 7 caught, 6 missed, 0 unviable/timeouts**
(`mut-out-checker-number`). Rerunning the entire same group against only
`w10_release_killers` yields **13/13 caught**, zero missed/unviable/timeouts
(`mut-out-checker-number-killers`). The six initial survivors are all killed:

| File/line in checker.rs | Operator | Discriminating input in killer |
|---|---|---|
| 737:13 | delete match arm `b'0'` | unused property `0`, `-0`, `0.1` must parse |
| 737:30 | `+=` to `-=` | zero must advance exactly one byte |
| 737:30 | `+=` to `*=` | zero at a nonzero scanner offset must advance |
| 743:54 | `==` to `!=` | valid fractional digits vs `1.` / `1.e2` |
| 749:26 | `+=` to `-=` | exponent `1E+2`, `1e-2` |
| 749:26 | `+=` to `*=` | exponent sign followed by digits |

These expose missing test cases, not a production bug. No equivalent survivor
is needed for this group. `Scanner::hex4` **11/11 caught** by the killers alone,
zero missed/unviable/timeouts; `--list` and Found both 11
(`mut-out-checker-hex4-killers`, 81 seconds). Neither
checker.rs nor policy.rs has in-file unit tests requiring a second `--lib` run.
Private transport helpers do have unit tests (3 pass above); their mutation
was not run. An exploratory transport `--list -F bypass_environment_proxy`
unexpectedly also includes 6 URL_COMPONENTS field deletions (14 names rather
than the intended 8); no mutation verdict is claimed for that listing.
The earlier W2 checker partial verdict is retained as historical evidence,
not described as a fresh complete verdict.

Fresh checker coverage is **24/355 unique mutants**, all 24 caught by the new
killers; **331/355 not run in this lane**. The number group was run twice; it is
counted once. These 24 lie in the historical 101-unrun scanner group, leaving
**77 historical unrun identities**; the historical timeout and equivalent
weekday survivor are not re-verified here. Thus no complete checker verdict.
Policy19 plus checker24 gives **43 fresh unique Rust mutants: 40 caught,
3 compile-unviable, 0 unresolved missed, 0 timeout** after the killer proof.
The six initial missed results remain visible in their original output directory.
No file exhausted its 20-minute budget; other checker/helper groups were left
unrun to keep this lane bounded. PowerShell manual6 is separate from Rust43.

Raw logs and inventories remain local under target and mut-out-*.

Commands (environment and outer time limits as above; each selection was also
listed with `--list` and compared with the run's Found count):

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 90 -j 1 --file "**/http/policy.rs" --cargo-arg=--test=http_policy --output mut-out-http-policy
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file "**/updates/checker.rs" -F "Scanner.*::number" --cargo-arg=--test=updates_checker --output mut-out-checker-number
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file "**/updates/checker.rs" -F "Scanner.*::number" --cargo-arg=--test=w10_release_killers --output mut-out-checker-number-killers
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file "**/updates/checker.rs" -F "Scanner.*::hex4" --cargo-arg=--test=w10_release_killers --output mut-out-checker-hex4-killers
```

## Gates and not run

Native binaries were invoked directly from target/debug/deps after staging,
under `timeout 120`, with `--include-ignored --test-threads=1`, avoiding another
cargo build during mutation. Config isolation variables point under target.
`composition_graph`: **4/4 pass**, including real dispatcher/message-loop graph
construction. `http_transport_loopback`: **18 pass / 2 fail**; both failures are
the explicit HTTPS/PAC/trust rig `panic!()` placeholders. `update_http_composition`:
**9 pass / 1 fail**, the explicit attended Settings/ShellExecute placeholder.
These are open verification gaps, not a passing native verdict; placeholder
failures do not exercise TLS/PAC/ShellExecute. The input desktop probe reports NO.

Per-package `cargo fmt -p PACKAGE --check` and
`cargo clippy -p PACKAGE --all-targets --locked -- -D warnings` pass for pr-core,
pr-platform-win and pr-shell. No workspace,
real release build/package, hosted check, online oracle or full-project mutation
claim is made. Real RAW samples and attended hardware measurements W8-12j,
W10-B2 and ui_smoke_perf are outside this lane.

`python -I tools/dev/doc-links.py --quiet`: 0 errors (472 files; 171 proposed,
14 archives unchecked in the initial check). Proposed entries are not failures.
The final check after documentation edits also passes with these counts. Killers contain no timing
or wall-clock assertions, so no timing-related 20-repeat claim is needed.
