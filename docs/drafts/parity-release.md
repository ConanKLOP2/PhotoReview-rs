# Proposed PARITY rows for W10 release

These are proposals only; add to `docs/PARITY.md` after the gates pass. Header/columns match that file. Test names marked `new` do not exist at HEAD.

| ID | Status (Todo / Doing / Done / Different) | Rust location | Proving test | Note |
|---|---|---|---|---|
| X-release-workflow | Todo | `.github/workflows/release.yml` (new) | `check_release_workflow` (new) + tagged CI smoke | Manual existing tag, green `build-test`, one native exe, zip and SHA256SUMS; `contracts/W10-R1.md` |
| X-exe-versioninfo | Different (D-W8B6-53; new ADR pending) | `pr_shell::build_support::version_info` (new) | `exe_version_resource_matches_tag` (new) | W8-20a omits RT_VERSION; R2 proposes one for releases only, pending P-47 |
| X-release-dependencies | Todo | `deny.toml` (new) | `cargo deny check advisories licenses bans sources` (new CI gate) | External Rust crates only; native LibRaw separately verified, `contracts/W10-R3.md` |
| X-release-native | Todo | `tools/verify-release-native.ps1` (new) | `release_native_hashes_and_notices_match` (new) | Two DLLs, pins, source offer, third-party notices, exe size/icon and chosen catalog layout (D-W10release-7); `contracts/W10-R4.md` |
| X-release-coexistence | Different (R0002; release test planned) | `pr_core::identity`, `pr_core::app_paths` | `rust_instance_does_not_join_csharp_named_mutex` (new); `new_never_points_into_csharp_app_root` (existing) | Separate exe, data and instance scope; update feed correction pending, `contracts/W10-R5.md` |
