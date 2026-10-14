# W2-18: UpdateChecker (GitHub "latest release" lookup, HTTP behind a trait)
- Color: 🟢 by the plan, treated as 🟡 here | Porter: **sonnet** (plan says Haiku; see "Porter model") | Test porter: haiku | Verifier: sonnet
- C# sources (baseline d92d6aae), under `src/PhotoReview.Core/Updates/`:
  - `UpdateChecker.cs` (126 lines)
  - `IUpdateChecker.cs` (10 lines)
  (read with `git -C C:/MyProjects/PhotoReview show d92d6aae:src/PhotoReview.Core/Updates/<file>`)
- C# tests to port (all at d92d6aae; `tests/PhotoReview.Core.Tests/Updates/`):
  - `UpdateCheckerTests.cs` (171 lines): `Newer_ReturnsUpdateAvailable`, `SameOrOlder_ReturnsUpToDate` (`v2.0.93`, `v2.0.10`), `ForeignUrl_FallsBackToReleasesPage`,
    `DraftOrPrerelease_IsIgnored` (3 rows), `HttpError_MapsToFailure` (403/429/404/500), `BadJson_ReturnsBadResponse` (6 bodies), `NetworkException_ReturnsOffline`,
    `TimeoutStyleCancellation_ReturnsTimeout`, `SlowServer_TimesOut` (50 ms), `CallerCancel_Throws`, `InvalidCurrentVersion_FailsWithoutNetwork` (null, `garbage`),
    `Request_IsMinimalAndHttps`, `OversizedBody_ReturnsBadResponse`.
  - `UpdateCheckerGapTests.cs`: `CheckAsync_Http403WithANonRateLimitBody_ReturnsFailedWithoutThrowing`, `CheckAsync_HandlerThrowsInvalidOperationException_ReturnsFailedWithoutThrowing`,
    `Http403_RemainingZero_IsRateLimited`, `Http403_RetryAfter_IsRateLimited`, `Http403_RemainingPositive_IsBadResponse`, `HttpRequestError_IsClassified` (7 rows).
  - `UpdateCheckerBoundaryTests.cs`: `ExactlyMaxBytes_WithContentLength_IsAccepted`, `ExactlyMaxBytes_WithoutContentLength_IsAccepted`,
    `OneByteOverMax_WithoutContentLength_IsRejected`, `DeclaredLengthOverMax_IsRejected` (assert the body was never requested),
    `NonStringHtmlUrl_FallsBackToReleasesPage` (`123`, `true`, `{}`), `CustomTimeout_IsHonoured` (100 ms).
  - NOT here: `AppVersion*Tests`, `UpdateUrlPolicyTests` (W1-10, merged); the App-side "Check for updates" button and its view model (App wave).
  - Golden test (new, required): `tests/golden/update-checker.json` (126 cases: 114 hand-written + one per `HttpRequestError` value).
- PARITY rows: P-09 (update check). Suggested new row `X-http-transport` for the `HttpTransport` trait (the real implementation is NOT part of this unit).
- Owned files (the ONLY files you may create/modify):
    crates/pr-core/src/updates/checker.rs        (stub: contracts-w2/W2-18/stubs/crates/pr-core/src/updates/checker.rs, compiles clippy-clean)
    crates/pr-core/tests/updates_checker.rs
  Lead-owned: `updates/mod.rs` gets `pub mod checker;`. Read-only input: the golden above, `updates::version` (W1-10), `sync::{CancelToken, Canceled}`.
- Public API (stub, do NOT change signatures): consts `LATEST_RELEASE_URL`, `DEFAULT_TIMEOUT`, `MAX_BODY_BYTES`, `USER_AGENT`, `ACCEPT`; trait `UpdateCheck::check`;
  `HttpRequest`, `HttpRequestError`, `HttpError`, `HttpBody::read`, `HttpResponse::{status, headers, content_length, body, header_values}`, trait `HttpTransport::get`;
  `UpdateChecker::{new, classify, is_rate_limited, interpret}`.
- Semantics notes (all verified against the baseline source and the 126 golden outputs):
  1. **`check` order.** (a) `AppVersion::try_parse(current_version)`; `None` => `Failed(InvalidVersion)` with ZERO transport calls and no cancel check.
     (b) ONE `transport.get(&HttpRequest { url: LATEST_RELEASE_URL, user_agent: USER_AGENT, accept: ACCEPT }, timeout, cancel)`: GET, no body, no query, nothing about the
     user (golden `request`). (c) `is_rate_limited` => `Failed(RateLimited)`. (d) status not in 200..=299 => `Failed(BadResponse)` (302 is a failure: the checker does no
     redirect handling, the transport does). (e) `content_length > MAX_BODY_BYTES` => `Failed(BadResponse)` WITHOUT touching the body (golden `body_was_requested == false`).
     (f) read the body in chunks (16 KiB in C#; any size is fine): when the running total would exceed `MAX_BODY_BYTES` => `Failed(BadResponse)`; exactly 1 048 576 bytes
     is accepted (golden cap, cap+1, buffered and streamed, and a lying `Content-Length: 10`). (g) decode UTF-8 LOSSILY (`String::from_utf8_lossy`: `0xFF` becomes U+FFFD;
     a BOM is NOT stripped, see note 5), then `interpret(&text, current)`.
  2. **Error mapping.** Transport/body-read `HttpError`: BOTH `Canceled` and `Timeout` are decided by the CALLER token, like C#'s
     `catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)`: `cancel.is_canceled()` => `Err(Canceled)`, else `Ok(Failed(Timeout))`
     (so a transport returning `Canceled` while the caller token is not canceled is a Timeout, like golden `timeout-style-cancellation-is-timeout`).
     `Request(e)` => `Failed(classify(e))` (golden `http-request-error:*`, `network-exception-without-error-code-is-offline` = `Request(Unknown)` => Offline).
     `Other(_)` => `Failed(BadResponse)`. `Err(Canceled)` is returned ONLY for caller cancellation; everything else is `Ok(Failed(..))`.
  3. **Timeout** is enforced by the TRANSPORT over the whole exchange (connect, headers, every body read), with the `timeout` argument (`DEFAULT_TIMEOUT` 10 s, or the
     constructor value). The checker itself starts no timer; a transport that ignores it is a transport bug. Fake transports in the tests honour it
     (`hang`: sleep up to `timeout` then `Err(Timeout)`; `cancel_by_caller`: poll `cancel` every 1 ms then `Err(Canceled)`).
  4. **`is_rate_limited`.** 429 => true. Not 403 => false (so `200` + `X-RateLimit-Remaining: 0` is not rate limited). 403 => true when the response has a
     `Retry-After` that .NET's typed `RetryAfter` accessor can parse, or any `X-RateLimit-Remaining` value (header repeated => any value) that is exactly `"0"` after
     Unicode-whitespace trim (`str::trim`, covers tab; `"00"`, `"-0"`, `""` are NOT zero). `Retry-After` is "well formed" when it is (after trimming) either a plain run of
     ASCII digits that fits `i32` (`"0"`, `"30"`, `" 30 "`, `2147483647` yes; `99999999999`, `+30`, `-5`, `1.5`, `""`, `abc` no) or an HTTP-date as .NET's `HttpDateParser`
     accepts it: RFC 1123 `Wed, 21 Oct 2015 07:28:00 GMT` (also single-digit hour, missing zone, numeric zone `+0000`, any past date), RFC 850 `Wednesday, 21-Oct-15 07:28:00 GMT`,
     asctime `Wed Oct 21 07:28:00 2015`. Rejected by .NET (golden => BadResponse): all-lowercase date, wrong weekday for the date, missing weekday, day 32.
     The golden is the spec: implement a small grammar that reproduces exactly those 12 date cases; do NOT call it "RFC 7231 compliant" and do not add formats the golden
     does not cover without asking (see Open questions 3). A malformed `Retry-After` plus `X-RateLimit-Remaining: 0` is still rate limited (either condition suffices).
  5. **`interpret` (JSON).** Strictly the `System.Text.Json` `JsonDocument.Parse(string)` rules, with default options:
     - Invalid JSON (empty text, trailing garbage, trailing comma, `//` comment, `NaN`, leading zero number such as `01`, even inside a field we ignore), a BOM char at the start
       of the text (`utf8-bom-bytes-before-json-are-invalid`) => `Failed(BadResponse)`.
     - Max depth 64, root object counted as depth 1: root + 63 nested arrays is fine, root + 64 is invalid (golden `nesting-63/64`). `serde_json::from_str` allows 128 and
       is therefore WRONG here; enforce 64 yourself.
     - Strings may contain lone-surrogate `\ud800` escapes; the document still parses. Only reading such a string as a value (`tag_name`, `html_url`) or comparing a property
       NAME that holds one throws => `Failed(BadResponse)`. `serde_json` rejects every lone surrogate, so it cannot be used as is (golden
       `lone-surrogate-escape-in-ignored-field-is-fine` must give UpdateAvailable). Recommended: a ~120-line hand-written strict RFC 8259 scanner over the text (objects,
       arrays, strings with escape validation, numbers with the JSON grammar and no size limit, `true/false/null`), recording for the ROOT object a list of
       `(unescaped name or invalid marker, value kind, string value)`; no allocation of the other fields needed.
     - Root not an object (`[]`, `"x"`, `null`) => `BadResponse`.
     - Property lookup (`TryGetProperty`): name compared ordinally and case-SENSITIVELY, after unescaping (`"tag\u005fname"` matches `tag_name`); with DUPLICATE names the LAST
       one wins (`tag_name` twice, last being a number means "missing" => BadResponse; `draft: true` then `draft: false` => not a draft).
     - `tag_name` must be a JSON string, else (missing, `null`, number) `BadResponse`; `AppVersion::try_parse(tag)` must succeed (tolerates spaces, leading `v`/`V`, `+metadata`;
       a 2-part tag, `v1.0.0-`, an overflowing component `99999999999` fail) else `BadResponse`. This check comes BEFORE the draft/prerelease checks (golden
       `draft-with-invalid-tag-is-bad-response-first`).
     - `draft` / `prerelease` count only when the value is the JSON literal `true`: `"true"` (string) and `1` do NOT (golden `draft-as-string-is-not-a-draft`).
     - Drafts, `prerelease: true` and tags with a pre-release suffix (`latest.is_prerelease`) => `UpToDate(current.to_string())` (the RUNNING version, build metadata dropped
       by `Display`). Note the asymmetry: a running `2.1.0-beta.1` against tag `v2.1.0` is UpdateAvailable (compare ignores the suffix, golden).
     - `latest.cmp(&current) != Greater` => `UpToDate(latest.to_string())` (an OLDER tag reports the older number, golden `older-tag-up-to-date-reports-latest`).
     - Otherwise `UpdateAvailable(latest.to_string(), validate_update_url(html_url string or None).unwrap_or(RELEASES_PAGE_URL))`; a non-string `html_url` counts as `None`.
       The validated URL is already normalized by W1-10 (host lowercased, golden `html-url-padded-and-uppercase-host-is-normalized`).
  6. **`classify`** is a total match; keep an explicit arm for every variant (no `_`) so a new variant is a compile error. `Unknown` is Offline ("historical behavior").
  7. **No `Send`/threading in the checker.** `check` is blocking (R0003); the App calls it from a worker. `UpdateChecker` is `Send + Sync` through `Arc<dyn HttpTransport>`.
- Traps checklist (docs/MULTI-AGENT-PLAN §7): 2 (culture: the C# header/JSON parsing is invariant; `str::trim` vs `trim_ascii` matters for the `0` test: use `trim`),
  3 (`Content-Length` is `u64`, the cap is `usize`: compare as `u64`; never `as i32` on `Retry-After`), 8 (`None` vs missing vs wrong-typed JSON values all go to the same
  fallback, but only for `html_url`; for `tag_name` all of them are `BadResponse`), 9 (do not add `serde`/`serde_json` derive types for the release: they would accept what
  STJ rejects and the reverse), 10 (nothing order-dependent here except last-wins duplicates).
- Depends on:
  - W1-10 `updates::version::{AppVersion, UpdateCheckResult, UpdateFailure, validate_update_url, RELEASES_PAGE_URL}` (merged; `AppVersion` is `Copy + Ord`).
  - W0 `sync::{CancelToken, Canceled}` (merged). pr-test-support `golden` loader.
  - No other Wave 2 unit. The real network client is NOT owned here (Open question 1); the unit is fully testable with fakes.
- Done when: every C# test above has a Rust twin with `// C#: <Class>.<Method>` (theory rows become table loops); the golden `update-checker.json` passes all 126 cases
  (result status/latest_version/download_url/failure, `request_count`, the `request` fields, `body_was_requested` for `length_mode: declared`, and `threw` =>
  `Err(Canceled)`); `UpdateChecker::new(.., None)` uses `DEFAULT_TIMEOUT` (unit test); clippy clean; the stub's `#![allow(...)]` line removed; no new dependency in `Cargo.toml`.

## Golden package
- `oracle/W2-18.cs`: command `update-checker`; drives `UpdateChecker.CheckAsync` through a fake `HttpMessageHandler`, one result object per case, plus one appended case per
  `System.Net.Http.HttpRequestError` value (run on .NET 10: 12 values). Dispatch line is in the file header
  (`"update-checker" => Commands.UpdateCheckerCases(inputs),`). Reads `MaxBodyBytes` by reflection and reports it as `max_body_bytes` (1048576).
- `golden-inputs/update-checker.json`: `cases[]` with `id`, `current` (string or null), `timeout_ms` (optional), `transport`:
  - `kind`: `response` (default status 200) | `throw_http_request_error` (`error` = enum name) | `throw_http_request_exception_no_error` | `throw_task_canceled` |
    `throw_invalid_operation` | `hang` | `cancel_by_caller`.
  - `body`: `{text}` | `{raw_hex}` | `{nested_arrays: n}`; modifiers `padded_to` (pad with spaces to N bytes), `bom`.
  - `length_mode`: `buffered` (known length = body size) | `stream_unknown_length` (`content_length: None`) | `declared` + `declared_length` (header value, body may differ,
    read counted) | `text_string_content`.
  - `headers` (validated add) / `raw_headers` (added without validation; value string or array = repeated header) / `retry_after_seconds` (typed property).
  Rust mapping: `headers`/`raw_headers`/`retry_after_seconds` all become plain `(name, value)` pairs (`retry_after_seconds: 30` => `("Retry-After", "30")`); the validated/raw
  distinction is a .NET artifact the string-level `is_rate_limited` already absorbs.
- `golden-reference/update-checker.json`: outputs; `result` is `{status, latest_version, download_url, failure}` (enum names as strings) or `{threw: "OperationCanceledException"}`.
- Case design: every `UpdateFailure`; all status classes; 43 `http-*` cases (mostly the 403 rate-limit header rule); JSON grammar edge cases (BOM, depth 63/64, surrogates, invalid UTF-8,
  duplicates, escaped keys); size cap at cap, cap+1, buffered/streamed/declared/lying declared; non-string `html_url` kinds; invalid running versions.
- Verified in this review: oracle, inputs and reference are consistent (126 = 114 + 12; ids unique per input; reference ids equal input ids plus the 12 appended). No defect found.

## Porter model
**sonnet.** The plan lists Haiku because the unit is 126 lines, but the difficulty is not the line count: (1) a strict JSON reader that must match `System.Text.Json`
(depth 64, lone surrogates tolerated, last-wins duplicates, escaped key matching) where the obvious `serde_json` solution fails 3-4 golden cases; (2) reproducing .NET's
`Retry-After` date grammar from examples; (3) error mapping around cancel vs timeout. All three are exactly the "plausible but wrong" class where a weaker model passes the
hand-written tests and fails the golden. A Haiku test porter is fine for the golden-driven fake transport once the API exists.

## Open questions
1. **Real transport and endpoint.** `HttpTransport` has no production implementation; the repo has no HTTP crate (R0003 and `Cargo.toml` list none). Lead decides which crate
   and which layer (`pr-platform-win` with WinHTTP keeps the system proxy and certificate store, like `HttpClient`; or a pure-Rust client). The stub documents the
   requirements: follow up to 50 redirects, system proxy and trust store, whole-exchange timeout, prompt cancel. Until then the App wires a "not available" transport
   that returns `HttpError::Other` (=> BadResponse) or the button stays hidden: lead's call.
2. **Endpoint ownership.** `LATEST_RELEASE_URL`, `RELEASES_PAGE_URL` and the `validate_update_url` allow-list still name `ConanKLOP2/PhotoReview` (the C# repo). If the
   Rust app is published from another repository these three must change TOGETHER with the golden (it embeds the URL in 100+ cases). Parity says keep; confirm.
3. **`Retry-After` date grammar.** .NET's `HttpDateParser` accepts a fixed list of formats that is only sampled by the golden. Real GitHub sends integer seconds, so the
   risk is low; the contract asks the porter to match the 12 date cases and no more. Acceptable, or should the oracle grow a case per .NET format string?
4. **Property scan on malformed names.** A root-object property NAME with a lone-surrogate escape fails the check in the golden (it was before `tag_name`). .NET scans
   backwards and the `draft`/`prerelease` lookups scan the whole object when those keys are absent, so a bad name AFTER `tag_name` fails too; a bad name when `draft` and
   `prerelease` are both present near the end might not. The contract treats "any bad name in the root object" as BadResponse. Not covered by a golden case; worth one if
   cheap.
