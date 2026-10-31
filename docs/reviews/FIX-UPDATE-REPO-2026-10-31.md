# FIX-UPDATE-REPO (2026-10-31)

Status: done in this PR. Source: `docs/reviews/ST-HTTP-SECURITY-2026-10-31.md` M2.

## Decision

User answer (chat, 2026-10-31): the update source is the user's GitHub; understood as the Rust repository. `git remote -v` gives `origin = https://github.com/ConanKLOP2/PhotoReview-rs.git`, so the repository used is **`ConanKLOP2/PhotoReview-rs`**. Recorded as DV-22 (ADR) and P-60.

## Where the C# repository was named

Runtime configuration (changed, now one constant):
- `crates/pr-core/src/updates/mod.rs`: new `update_repo!()` macro and `UPDATE_REPO`. Single source of truth.
- `crates/pr-core/src/updates/checker.rs`: `LATEST_RELEASE_URL` derived from it.
- `crates/pr-core/src/updates/version.rs`: `RELEASES_PAGE_URL` and the allow-list path in `validate_update_url` derived from it (the former three duplicated literals are gone). Validation rules (HTTPS, host, userinfo, port, dot segments, case-insensitive prefix) are unchanged.

Not changed on purpose: `AGENTS.md`, `README.md` (they describe the C# spec and oracle repo), `contracts/*` (historic spec text), `docs/PARITY.md` P-09 text, `docs/reviews/*`.

Tests that used the C# URL as plain data (literal bulk change to `PhotoReview-rs`): `pr-core` `updates_version.rs`, `updates_checker.rs`, `w2_fs_mutation_killers.rs`; `pr-app` `settings_presenter_apply.rs`, `w8_presenter_mutation_killers.rs`; `pr-shell` `update_http_composition.rs`; `pr-platform-win` `http_transport_loopback.rs`. 12 production/test-source spots changed in code, 8 files of tests.

## Goldens

`tests/golden/update-url-policy.json`, `tests/golden/update-checker.json` (and the matching `golden-inputs`) are C# oracle output and were NOT edited. Their replay in `updates_version.rs` and `updates_checker.rs` goes through `divergence_adjusted_repo` (rewrites the repository path in inputs, request URI and expected URLs, including the JSON-escaped `ConanKLOP2\/PhotoReview`); four dot-segment or percent-encoded policy cases are adjusted by hand in `golden_update_url_policy`. All 126 checker cases and all policy cases pass, so only the repository name differs from the oracle.

## Tests (no network)

New: `validate_rejects_the_csharp_repo_and_accepts_the_rust_repo`, `endpoints_follow_the_rust_repository_not_the_csharp_one`, `no_release_published_404_is_a_failure_not_an_update`, `release_url_of_the_csharp_repository_falls_back_to_the_rust_releases_page`.

Run and green: `cargo test -p pr-core --test updates_version --test updates_checker --test w2_fs_mutation_killers` (22 + 37 + killers), `-p pr-app --test settings_presenter_apply --test w8_presenter_mutation_killers` (31), `-p pr-shell --test update_http_composition` (15), `-p pr-platform-win --test http_transport_loopback` (18, 2 ignored). `cargo fmt` per crate, one `clippy --all-targets -D warnings` each for pr-core, pr-app, pr-shell, pr-platform-win: clean. `python tools/dev/doc-links.py`: 0 errors.

## Behaviour when the Rust repository has no release

GitHub answers `releases/latest` with 404 when there is no published (non-draft, non-prerelease) release. The checker maps every non-2xx status to `Failed(BadResponse)` (same as C#; the existing test `http_error_maps_to_failure` pins 404). So the user sees the "could not check" failure, NOT "you are up to date" and not an update. Pinned by `no_release_published_404_is_a_failure_not_an_update`. Not changed here (would be a further divergence from C# and the golden); if "no release yet" should read as up to date, that is a separate decision.

A release whose `html_url` points at the old C# repository is not opened; it falls back to the Rust releases page (test above).

## PARITY

`docs/PARITY.md` was not edited (lane rule). Suggested: P-09 should say the endpoint is `ConanKLOP2/PhotoReview-rs` (Different, DV-22) instead of the C# repository.
