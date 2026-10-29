# W10 release policy (proposed, P-54 through P-56)

Date: 2026-10-29. Scope: D-W10release-1 through D-W10release-8.
These are implemented recommendations, not maintainer approval of legal or signing inputs.

- Independent canonical `vX.Y.Z` tags select the exact peeled commit. Components must fit
  Win32 unsigned 16-bit version fields. Publication is manual dispatch only; no PR or push
  trigger, tag creation, CI bypass, release overwrite or credentials in tracked files.
- First release is unsigned. Certificate/service setup is deferred; no signing dependency
  or secret is introduced. Notes disclose unsigned status and coexistence/recovery rules.
- Supersede P-47's no-version rule only for explicit release builds:
  `PHOTOREVIEW_RS_RELEASE=1` requires `PHOTOREVIEW_RS_VERSION=X.Y.Z` and emits RT_VERSION.
  Ordinary builds, including ordinary `cargo build --release`, keep the old no-version policy.
  A Cargo optimization profile is not authorization to create a product release. Fixed/string
  version fields and manifest/icon are tested against the linked PE. Company/copyright fields
  await reviewed project text (P-55); no year or owner is invented.
- Install the pinned cargo-deny 0.19.9 tool with `cargo install --locked`; it is not an
  application dependency. Nightly at 03:00 UTC, dispatch and master pushes audit the graph;
  PRs audit when Cargo files, deny policy or CI policy change. The deny job succeeds without
  installing on unrelated PRs and participates in `build-test`. Vulnerabilities, unsound,
  unmaintained and yanked dependencies fail; duplicate versions remain warnings.
- Keep the shell's update checker explicitly `None`: no HTTP request or C# download link
  is offered by a failed check. Do not activate the HTTP lane until a Rust feed is verified.
  Full R5 coexistence process matrices remain separate work.
- Bundle the exact LibRaw 0.22.2 source/binary archive as `LibRaw-SOURCE.zip`, independently
  hash it and name its pin in release notes. The inspected baseline archive contains C/C++
  source, headers and buildfiles; that observation is not legal approval of source sufficiency.
  It is downloaded into ignored build inputs, never introduced as a tracked DLL. Native
  loaders remain dynamic, absolute executable siblings; neither PATH nor cwd is a fallback.
- Package both shipped catalogs under `Languages`. Vietnamese selection in the actual
  release still needs runtime proof; the review flag blocks publication until it is supplied.
- Smoke defaults on and failure is fatal, with an isolated root and a 60-second bound.
  A maintainer may disable runner smoke only with explicit machine smoke evidence in
  `smoke_result`; that text is included in release notes.

Alternatives and tradeoffs are recorded in the corresponding P rows and the eight contract
decisions. Publication remains blocked by incomplete review flags, TurboJPEG package pin
and complete licence texts, application LICENSE and measured exe-size policy. These are
intentional gates, not evidence that R1/R4 is fully validated.
