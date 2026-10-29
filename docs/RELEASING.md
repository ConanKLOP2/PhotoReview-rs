# Releasing PhotoReviewRs

Implementation: W10-R4 (native inputs), W10-R2 (required version resources), W10-R3
(dependency audit), then W10-R1 (manual publication). See
[release policy](adr/ADR-W10-release.md) and [proposed decisions](PROPOSED-DECISIONS.md).
No Cargo.toml dependency, certificate or secret was changed by this lane.

## Current publication blockers

The workflow is deliberately fail closed until a maintainer completes these inputs:

1. Obtain libjpeg-turbo-native-windows 3.0.0 from the HTTPS URL in the fetch script;
   review the package and its `lib/net6.0/Turbojpeg.dll` against the baseline DLL pin.
   Replace the explicit blocking text in `native/turbojpeg.package.sha256` with the
   measured package SHA-256. This environment could not reach NuGet. Never use a DLL
   hash as a package hash. No unverified package was unpacked.
2. Replace the incomplete native TurboJPEG notice with reviewed complete 3.0.0 licence
   texts, including IJG/zlib, and review the LibRaw source offer. The synced third-party
   notices are adapted from read-only C# baseline `7eaee2a2` for Rust paths/exe name;
   they do not purport to cover all Cargo crates. Supply the missing root LICENSE with
   maintainer-approved copyright text. Cargo metadata declares MIT, but does not identify
   a copyright owner. CompanyName/LegalCopyright resource fields remain omitted.
3. Validate English and Vietnamese selection in the built release. Set each boolean in
   `native/release-review.json` to true only after its named review actually passes.
4. Measure the first Rust release exe and record reviewed plausible min/max bounds and
   the immutable source commit in `native/release-size.json`. Null bounds are fatal;
   do not copy C# AOT size limits or derive bounds from the exe being verified at runtime.
   The lane's initial unsigned x64 `0.1.0` build measured 8,043,520 bytes; the recorded
   6,032,640..12,065,280 range is 75%..150% of that measured file, with SHA-256 and base
   commit recorded. It was built with uncommitted lane changes, so remeasure the actual
   tag and record that immutable source identity before publication.
5. Run the installed cargo-deny gate and resolve findings without silently ignoring
   advisories or changing dependencies. Cargo-deny was unavailable locally; the exact
   [0.19.9 template](https://github.com/EmbarkStudios/cargo-deny/blob/0.19.9/deny.template.toml)
   and schema were inspected, but local audit execution remains unverified.
   The pinned 0.19.9 CLI takes `--config` after `check` (the draft command had it before):
   `cargo deny --locked check --config deny.toml advisories licenses bans sources --deny unmaintained --deny unsound`.

LibRaw pins, LGPL/CDDL texts and notice were imported using `git show` at the baseline;
the source package is fetched by hash and kept under ignored `target/release-inputs`.
The package includes source and buildfiles as well as native binaries. Legal sufficiency
still needs review. Never add DLLs to Git. Local native testing can continue to use the
existing [stage-native script](../tools/dev/stage-native.ps1); hosted release runners use
the pinned fetcher and do not need the C# checkout.

## Build and verify locally

Use Windows PowerShell 5.1 with `powershell -NoProfile -ExecutionPolicy Bypass -File`
for the scripts. Set `CARGO_TARGET_DIR` to this worktree's target directory,
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, then set
`PHOTOREVIEW_RS_RELEASE=1` and `PHOTOREVIEW_RS_VERSION=X.Y.Z` for the explicit release.

```powershell
cargo build --release --locked -p pr-shell
cargo test -p pr-shell --locked --test build_version_info --test build_res_writer --test exe_resources
./tools/fetch-release-native.ps1
./tools/fetch-release-native.ps1 -Verify
./tools/stage-release.ps1 -Version X.Y.Z
./tools/verify-release-native.ps1 -ReleaseDirectory release-stage -Version X.Y.Z
./tools/pack-release.ps1 -Tag vX.Y.Z
```

`-Verify` is offline and read-only. Both package pins must be valid before download;
package hashes precede extraction; every archive name is checked before any entry is
written; both DLL hashes precede staging. No fallback to a system DLL exists. The stage
verifier enforces a complete file allowlist, baseline DLL/source hashes, exact notice and
catalog bytes, measured exe-size policy, icon/manifest types and actual PE version.
Packing compares zip file entries to the verified stage and writes SHA256SUMS for the
exe, DLLs, source archive and zip. The source archive inside the zip is a durable source
asset; release notes identify its exact upstream name/hash.

Run `tools/dev/test/release-selftest.ps1` and `tools/dev/test/release-workflow-selftest.ps1`
through the same PowerShell command. Packaging selftests use owned temporary packages
and substitute PE metadata only; linked PE checks live in `exe_resources`. This does not
prove native loading or a GUI smoke on a release runner. Selftests contain no real photo
or Recycle Bin operations. PowerShell smoke and local verification commands must be run
with bounds; the workflow has job and smoke timeouts.

## Dispatch

Create an independently governed Rust release tag only after its commit's latest
`build-test` passes. Dispatch **Release PhotoReviewRs** with that existing tag and
`run_smoke=true`. No tag push triggers publication. API errors, absent checks, a newer
failed/pending check and an existing release are all fatal. The tagged checkout must
equal the checked peeled SHA. All action references are pinned.

If the hosted runner cannot create a Direct2D window, run the same staged exe's `--smoke`
on a maintainer desktop with an isolated data root and paste the command, version/hash,
machine and successful exit into `smoke_result` while selecting `run_smoke=false`.
The script checks that evidence text is present; a maintainer remains responsible for
its truth. The zip changes no registry/associations. Finish pending undo/recovery in
the application that started it; Rust and C# retain separate data roots.
