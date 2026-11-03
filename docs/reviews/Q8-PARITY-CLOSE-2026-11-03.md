# Q8 - PARITY close-out of P-09, X-http-transport, X-http-composition (2026-11-03 run, executed 2026-11-04)

Task Q8, branch `parity/http-update`, worktree `.claude/worktrees/pc-http-update`, cut from origin/master `84d2c151`.
Owner request (2026-11-03): finish PARITY. Rules followed: no edit of PARITY.md / BACKLOG / task_on_progress / SYNC-LEDGER; no production code
changed; every number below was produced in this run on this desktop (Windows 10 19045, 12 logical CPUs, about 6.6 GB free RAM while 18 other
cargo processes of other agents were running; `CARGO_BUILD_JOBS=2`, own target dir `C:\Users\PC\Projects\target-pc-http-update`).
`cargo fmt -p` was not used (os error 206 on long paths): `rustfmt --edition 2024 --check` on the five new files, all clean.

## What is new in this run

| Item | Result |
|---|---|
| Native `.NET 10` runtime oracle (`dotnet` 10.0.401 is installed here; probe source at the end of this file, kept outside both repos) | First machine-run comparison of the Rust transport with the real `new HttpClient()` of `UpdateChecker.cs`: environment proxy selection and NO_PROXY, request headers, redirects (51 requests, last answer 302), cookie over redirect, no decompression, untrusted-certificate error code. One real difference found (proxy CONNECT refusal, see F1). |
| `crates/pr-platform-win/tests/http_proxy_env.rs` (11 tests, new) | `HTTPS_PROXY` > `ALL_PROXY`, blank `HTTPS_PROXY` falls through, `NO_PROXY` exact and leading-dot bypass, unrelated `NO_PROXY` does not bypass, proxy with credentials fails closed without a connection, CONNECT line seen on a loopback proxy; every scenario re-runs the binary as a child process (`Command::env`, no unsafe `set_var`). |
| `crates/pr-platform-win/tests/http_wire_oracle.rs` (3 tests, new) | Request line and header set vs .NET (`Host`, `User-Agent`, `Accept`; WinHTTP adds `Connection: Keep-Alive`, pinned as the one benign difference), cookie from a redirect is returned, `Content-Encoding: gzip` is not decoded (.NET reads `abc` raw). |
| `crates/pr-platform-win/tests/http_tls_untrusted.rs` (1 native `#[ignore]` test, new) | A real TLS server (powershell.exe child with an in-memory self-signed `CN=localhost`; no store touched, nothing trusted; child killed by the test): `WinHttpTransport::get` fails with `SecureConnectionError` and never returns a response. .NET gives `SecureConnectionError` (inner `AuthenticationException`) too. Ran with `--include-ignored`: pass. |
| `crates/pr-platform-win/tests/http_url_parse_killers.rs` (5 tests, new) | Killers for `Url::parse` and the header-injection guard (credentials alone/both, query reaches the wire, fragment does not, path-less URL, other schemes, CR/LF in either header value). |
| `crates/pr-core/tests/q8_checker_killers.rs` (3 tests, new) | Boundary killers for `find_property` (lone-surrogate names exactly as long as the key, and with the backslash exactly at the key end). |

## Test runs (this run, all on this desktop)

| Command | Result |
|---|---|
| `cargo test -p pr-platform-win --lib http::` | 11 passed |
| `--test http_policy` | 4 passed |
| `--test http_transport_loopback -- --include-ignored --test-threads=1` | 18 passed, 2 failed = the two deliberate `panic!` placeholders (`get_https_to_http_redirect_returns_three_xx_response`: needs a locally TRUSTED HTTPS server; `native_proxy_pac_trust_and_cancel_matches_windows_defaults`: attended rig). Same as CX-REL. |
| `--test http_update_wire` | 7 passed |
| `--test http_proxy_env` / `http_wire_oracle` / `http_url_parse_killers` | 11 / 3 / 5 passed |
| `--test http_tls_untrusted -- --include-ignored` | 1 passed |
| `cargo test -p pr-shell --test update_http_composition -- --include-ignored --test-threads=1` | 9 passed, 1 failed = placeholder `native_settings_close_cancels_and_open_page_uses_shell` (`panic!`, attended) |
| `cargo test -p pr-shell --test composition_graph -- --include-ignored --test-threads=1` | 5 passed (includes the native dispatcher tests) |
| `cargo test -p pr-core --test q8_checker_killers` | 3 passed |
| `cargo clippy -p pr-platform-win -p pr-core --all-targets --locked -- -D warnings` | clean |

`native/x64` does not exist in this worktree (the pinned DLLs are not tracked); none of these tests load LibRaw/TurboJPEG, so P-50 staging was not needed.

## Mutation (time-boxed; about 65 minutes in total, a little over the 60 minute box because the survivor re-runs were added)

`tools/dev/mutants.sh`, `-j 1`, `--baseline skip` with the script's own preflight (unmutated command ran N > 0 tests), `--timeout 90`, own target dir.

### `pr_platform_win::http::transport` (101 mutants in the file)

| Batch | Selection | Found / run | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| A | `-F 'Exchange::open\|Deadline\|Url::parse'`, tests `http_policy`, `http_transport_loopback`, `http_update_wire`, `http_proxy_env`, lib (331 tests) | 34 / 34 | 24 | 7 | 3 | 0 |
| A2 | the 7 survivors re-run against `http_url_parse_killers` alone (filter also matched 3 neighbours) | 10 / 10 | 9 | 1 | 0 | 0 |

Survivors of batch A: `168:13`, `169:13`, `170:13` (`dwExtraInfoLength`/`dwUserNameLength`/`dwPasswordLength` deleted from the `URL_COMPONENTS` request), `175:40` (`||` to `&&` in the credentials guard), `286:48` (`||` to `&&` in the header-injection guard): all five KILLED by `http_url_parse_killers` (A2: caught). Two equivalent: `174:53` (`text.len() - 1` to `/ 1`: the slice then includes the NUL terminator and `WinHttpCrackUrl` stops at the first NUL; the query-string test passes unchanged with the mutant) and `377:48` (`previous.is_some_and(|f| f == usize::MAX)` to `!=`: a fresh handle has no previous callback, so the branch is unreachable without a failing registration). 0 timeouts.

NOT RUN: 67 of the 101 (`wide`, `os_error`, `win_error`, `other`, `Handle::*`, `redirect_target`, `bypass_environment_proxy`, `environment_proxy`, the rest of `Exchange::open` incl. the proxy source match arms, `Exchange::headers`, `Body::read`, `WinHttpTransport::get`, `REQUEST_OPTIONS`). Of these, the pure proxy/NO_PROXY helpers (12) and `wide`/`os_error`/`win_error` (4) were counted caught by CX-REL 2026-10-30, not re-run here; `headers`, `Body::read`, `get` and the callback are FFI-driven glue (skipped on purpose by the mutation rules). The new `http_proxy_env` tests were not exercised against the proxy-selection arms (those arms were not selected).

### `pr_core::updates::checker` (355 mutants in the file)

| Batch | Selection | Found / run | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| B | `UpdateChecker\|find_property\|is_rate_limited`, tests `updates_checker`, `w10_release_killers`, `w10_release_killers2`, `mut_misc_killers` (45 tests) | 38 / 38 | 32 | 3 | 3 | 0 |
| C | `Scanner\|scan_root_object\|word_at\|retry_after_is_well_formed`, same tests | 124 / 117 (budget) | 114 | 1 | 2 | 0 |
| D | the 3 survivors of B re-run against `q8_checker_killers` + `w2_fs_mutation_killers` (filter matched 7) | 7 / 7 | 7 | 0 | 0 | 0 |

Survivors of B: `130:9` (`Debug for UpdateChecker` -> `Ok(Default::default())`, killed by the existing `w2_fs_mutation_killers` Debug test that batch B did not include), `618:22` and `618:59` (`>` to `>=` in `find_property`, killed by the new `q8_checker_killers`). Survivor of C: `711:41` (`+` to `*` in `Scanner::string`, `let end = (self.pos + 4).min(len)`): equivalent by reading, the two expressions differ only for `pos` 0 or 1 and `string` is never entered there (the opening quote is at offset 1 of a body that starts with `{`, a name tail starts at the backslash); for every other `pos` the extra bytes are cut by `valid_up_to`. NOT RUN in C: 7 (`Scanner::object` 3, `scan_root_object` 4; run earlier by CL-MUT-MISC).

NOT RUN in this run: 200 of 355 (`match_format` 83, `weekday_of` 41, `read_digits` 21, `is_leap` 11, `days_in_month` 2, `skip_ws` 6, the function-replacement mutants not selected, and the 7 above). The Retry-After calendar/digit survivors are covered by `w2_fs_mutation_killers` (W2-FS-EVIDENCE); not re-verified here.

## Findings

* **F1 (parity difference, no fix, needs an owner decision).** With `HTTPS_PROXY` set and a proxy that answers the CONNECT with an error, `.NET` throws `HttpRequestException(ProxyTunnelError)` which `UpdateChecker.Classify` maps to `Failed(Offline)`. WinHTTP hands the proxy's 502 over as an ordinary response (`status 502`, pinned by `proxy_connect_refusal_surfaces_as_the_proxy_status_unlike_dotnet_proxy_tunnel_error`), so the Rust checker answers `Failed(BadResponse)`; a proxy 429 or 403 with `Retry-After` would even read as `RateLimited`. WinHTTP cannot tell a refused tunnel from an origin 502 by status alone, so this is not a small fix; no production change made. See Proposal 1.
* **F2 (benign).** WinHTTP sends `Connection: Keep-Alive` that .NET does not send (implied in HTTP/1.1). Pinned.
* **F3 (already known gap, now measured).** `HTTPS_PROXY=http://user:pass@host`: the Rust transport refuses before any connection (`unsupported environment proxy URI`); .NET accepts it (the oracle proxy saw the CONNECT; credentials are only sent after a 407 challenge, not tested). Pinned by `proxy_with_credentials_fails_explicitly_without_any_connection`.
* Everything else compared equal: the CONNECT target for `HTTPS_PROXY`/`ALL_PROXY`/blank `HTTPS_PROXY`/unrelated `NO_PROXY`; exact and leading-dot `NO_PROXY` bypass; 51 requests and a final `302 Location: /hop/51` over 50 redirects; cookie carried over a redirect; no automatic decompression; untrusted self-signed certificate = `SecureConnectionError`.
* Note on `localhost`: WinHTTP does not send `localhost` through a named proxy at all (the proxy saw nothing), so the proxy tests use the reserved name `update.example.test` (never resolved when a proxy is used).

## Per row

### P-09 (update check)

* Gaps before (from the row): checker mutation incomplete (77 historical unrun scanner identities, timeout not re-verified, full-file verdict missing); Settings close/late-poster/ShellExecute never run; TLS/PAC/runtime defaults NOT proved.
* C# test port check done: all 4 C# core test files of `Updates/` (`UpdateCheckerTests` 13, `GapTests` 6, `BoundaryTests` 6, `UpdateUrlPolicyTests` 2) have a Rust twin in `updates_checker.rs` / `updates_version.rs` (37 tests plus extras). The 6 `SettingsWindowUpdateCheckTests` (integration) are covered by the fake-host General page tests of `update_http_composition`, not by a real window.
* Ran: lib/loopback/wire/checker tests above (all green except the two documented placeholders), `.NET` oracle, 38 + 117 + 7 checker mutants (155 distinct of 355).
* Gaps after: 200 of 355 checker mutants NOT run in this run; no real-window Settings run (attended); no live GitHub check (no internet, and the Rust repository publishes no release: P-55).
* Proposed status: stays **Partial**.

### X-http-transport

* Gaps before: no real proxy, TLS or .NET comparison; transport mutation limited to pure helpers.
* Ran: see tables; 20 new tests in this crate, 34 transport mutants (5 real survivors killed, 2 equivalent), .NET oracle of 10 scenarios.
* Gaps after: (1) PAC/WPAD/WinINet user proxy, automatic proxy abortability; (2) a locally TRUSTED TLS server: the HTTPS to HTTP downgrade transport test and the TLS happy path (needs a root in the Windows trust store, a security setting this run did not change); (3) F1 and F3 above; (4) minimum TLS version and revocation (OS defaults, not pinned); (5) 67 transport mutants not run (glue excluded); (6) the FFI callback/handle lifetime has no exact-handle-count test.
* Proposed status: stays **Partial**.

### X-http-composition

* Gaps before: attended Settings close / late poster / ShellExecute.
* Ran: `update_http_composition` 9 pass + the attended placeholder fails by design; `composition_graph` 5/5 including native dispatcher tests.
* Gaps after: unchanged. A real Settings window needs a person at the machine; `ShellExecute` would open the user's browser.
* Proposed status: stays **Partial**.

## Proposed row text (for the lead to merge; nothing written to PARITY.md)

* P-09 note, append: `Q8 2026-11-04 (evidence file reviews/Q8-PARITY-CLOSE-2026-11-03.md, link it when merging): checker.rs 155 of 355 mutants run in this run (batch B 38: 32 caught, 3 missed, 3 unviable; batch C 117 of 124: 114 caught, 1 missed, 2 unviable; 0 timeouts); the 3 real survivors killed (Debug by w2_fs_mutation_killers, find_property 618:22/618:59 by q8_checker_killers), 711:41 equivalent; 200 NOT run. All C# Updates tests have a Rust twin. Stays Partial: attended Settings window, live check.`
* X-http-transport note, append: `Q8 2026-11-04: .NET 10 runtime oracle run on this desktop: HTTPS_PROXY/ALL_PROXY/NO_PROXY selection, request headers, 50 redirects, cookie over redirect, no decompression and untrusted certificate (SecureConnectionError, real TLS server, native test) equal; new http_proxy_env 11, http_wire_oracle 3, http_tls_untrusted 1 (native), http_url_parse_killers 5; transport mutation 34 run (24 caught, 7 missed, 3 unviable; 5 killed, 2 equivalent), 67 NOT run. Difference F1: a proxy that refuses CONNECT is Offline in C# (ProxyTunnelError) but a 502 response in Rust (BadResponse). Still open: PAC/WPAD, trusted TLS/downgrade, proxy credentials. Stays Partial.`
* X-http-composition note, append: `Q8 2026-11-04: update_http_composition 9 pass + 1 attended placeholder, composition_graph 5/5 (incl. native); unchanged gap: attended Settings close/ShellExecute. Stays Partial.`

## For a person (exact steps)

1. Attended Settings window (X-http-composition, P-09): `cargo build -p pr-shell`, start `PhotoReviewRs.exe`, open Settings, General. (a) Disable the network adapter and press "Check for updates": the status must read the offline text. Close Settings while a check is in progress (point `HTTPS_PROXY` at a listener that accepts and stays silent, so the check hangs until the 10 s timeout): no crash, no late write. (b) With a published release newer than the running version (needs P-55, or a throwaway release in `ConanKLOP2/PhotoReview-rs`), press the button: the open-release-page button must open the default browser on the Rust releases page, and a foreign `html_url` must show no button. Then replace the body of `native_settings_close_cancels_and_open_page_uses_shell` by a note and run `cargo test -p pr-shell --test update_http_composition -- --include-ignored`.
2. Trusted TLS rig (X-http-transport): on a DISPOSABLE VM only, create a test CA, trust it for the current user, run an HTTPS server for `localhost` that redirects to `http://127.0.0.1:<port>/`, and write the real `get_https_to_http_redirect_returns_three_xx_response` against it; then remove the CA. This run did not change the trust store.
3. PAC/WPAD rig (X-http-transport): on a disposable VM set an automatic configuration script that routes `update.example.test` to a loopback proxy and compare with `dotnet out\probe.dll proxy https://update.example.test:8443/update` using the same setting (no environment proxy variables).

## Proposals (no production code was changed)

1. **Proxy CONNECT refusal (F1).** Options: (A) accept and record a `Different` row (rare: only when a configured proxy refuses the tunnel; both outcomes are a failed check, `Offline` vs `BadResponse`); (B) map, for an https target reached through a proxy, a 407 / 5xx answer that carries no origin headers (`Server`, `Date`) to `ProxyTunnelError`, which needs a proxy lab to verify and risks misreading a real origin 502; (C) query the connect status through a WinHTTP option if a documented one exists (none found in this run). Recommendation: A now, B only if the owner wants it after a proxy lab. After the choice: an ADR + `Different` row, or a test-first change in `transport.rs`.
2. **Proxy credentials (F3).** Either keep the fail-closed refusal as a `Different` row, or parse `user:pass@` and answer a 407 with `WinHttpSetCredentials` (needs the proxy lab).

## Probe source (.NET 10, not committed to either repo; `dotnet build -c Release -o out`)

Results of the 2026-11-04 run: `HTTPS_PROXY`, `ALL_PROXY`, blank `HTTPS_PROXY` + `ALL_PROXY`, `NO_PROXY` not matching: proxy saw `CONNECT update.example.test:8443 HTTP/1.1` + `Host:` and the client got `ProxyTunnelError`; `NO_PROXY="other.example, update.example.test"` and `NO_PROXY=.example.test`: proxy saw nothing (`NameResolutionError`); `user:secret@` proxy: CONNECT seen, `ProxyTunnelError`; `tls`: `SecureConnectionError inner=AuthenticationException`; `raw`: 1 request, headers `Host`, `User-Agent`, `Accept`; `redir`: 51 requests, `302 location=/hop/51`; `cookie`: second request carries `Cookie: a=1`; `gzip`: body `abc`.

```csharp
using System.Net;
using System.Net.Http;
using System.Net.Security;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text;

// Q8 runtime oracle probe: what `new HttpClient()` (as UpdateChecker uses it) does against LOCAL servers only.
string mode = args[0];
if (mode == "proxy")
{
    var l = new TcpListener(IPAddress.Loopback, 0); l.Start();
    int port = ((IPEndPoint)l.LocalEndpoint).Port;
    var saw = new List<string>();
    var t = Task.Run(() => { while (true) { try { using var c = l.AcceptTcpClient(); var s = c.GetStream(); var buf = new byte[8192]; int n = s.Read(buf, 0, buf.Length); lock (saw) saw.Add(Encoding.ASCII.GetString(buf, 0, n).Replace("\r\n", " | ")); s.Write(Encoding.ASCII.GetBytes("HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")); } catch { return; } } });
    // PROXY_PORT placeholder in env values is substituted here
    foreach (var name in new[] { "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY" })
    {
        var v = Environment.GetEnvironmentVariable(name);
        if (v != null) Environment.SetEnvironmentVariable(name, v.Replace("PROXY_PORT", port.ToString()));
    }
    string result;
    try { using var c = new HttpClient(); c.Timeout = TimeSpan.FromSeconds(8); using var r = await c.GetAsync(args[1], HttpCompletionOption.ResponseHeadersRead); result = "status " + (int)r.StatusCode; }
    catch (HttpRequestException e) { result = "HttpRequestException " + e.HttpRequestError; }
    catch (Exception e) { result = e.GetType().Name; }
    await Task.Delay(300);
    lock (saw) Console.WriteLine("PROXY_SAW: " + (saw.Count == 0 ? "none" : string.Join(" || ", saw)));
    Console.WriteLine("RESULT: " + result);
    l.Stop();
}
else if (mode == "tls")
{
    using var rsa = RSA.Create(2048);
    var req = new CertificateRequest("CN=localhost", rsa, HashAlgorithmName.SHA256, RSASignaturePadding.Pkcs1);
    using var tmp = req.CreateSelfSigned(DateTimeOffset.UtcNow.AddDays(-1), DateTimeOffset.UtcNow.AddDays(1));
    using var cert = X509CertificateLoader.LoadPkcs12(tmp.Export(X509ContentType.Pfx, "x"), "x");
    var l = new TcpListener(IPAddress.Loopback, 0); l.Start();
    int port = ((IPEndPoint)l.LocalEndpoint).Port;
    _ = Task.Run(async () => { while (true) { try { var c = await l.AcceptTcpClientAsync(); var ssl = new SslStream(c.GetStream()); await ssl.AuthenticateAsServerAsync(cert); } catch { } } });
    string result;
    try { using var c = new HttpClient(); c.Timeout = TimeSpan.FromSeconds(8); using var r = await c.GetAsync($"https://localhost:{port}/x", HttpCompletionOption.ResponseHeadersRead); result = "status " + (int)r.StatusCode; }
    catch (HttpRequestException e) { result = "HttpRequestException " + e.HttpRequestError + " inner=" + e.InnerException?.GetType().Name; }
    Console.WriteLine("RESULT: " + result);
}
if (mode == "raw" || mode == "redir" || mode == "cookie")
{
    var l = new TcpListener(IPAddress.Loopback, 0); l.Start();
    int port = ((IPEndPoint)l.LocalEndpoint).Port;
    var seen = new List<string>();
    string CRLF = "\r\n";
    _ = Task.Run(async () => { while (true) { try {
        using var c = await l.AcceptTcpClientAsync(); var s = c.GetStream(); var buf = new byte[8192]; int n = await s.ReadAsync(buf);
        var text = Encoding.ASCII.GetString(buf, 0, n); lock (seen) seen.Add(text.Replace(CRLF, " | "));
        var path = text.Split(' ')[1]; string resp;
        string tail = CRLF + "Connection: close" + CRLF + "Content-Length: 0" + CRLF + CRLF;
        if (mode == "raw") resp = "HTTP/1.1 200 OK" + tail;
        else if (mode == "cookie") resp = path == "/start" ? "HTTP/1.1 302 Found" + CRLF + "Set-Cookie: a=1; Path=/" + CRLF + "Location: /next" + tail.Substring(0) : "HTTP/1.1 200 OK" + tail;
        else { int k = path.StartsWith("/hop/") ? int.Parse(path.Substring(5)) : 0; resp = "HTTP/1.1 302 Found" + CRLF + "Location: /hop/" + (k + 1) + tail; }
        await s.WriteAsync(Encoding.ASCII.GetBytes(resp));
    } catch { } } });
    string result;
    try { using var c = new HttpClient(); c.Timeout = TimeSpan.FromSeconds(8);
        using var rq = new HttpRequestMessage(HttpMethod.Get, "http://127.0.0.1:" + port + (mode == "cookie" ? "/start" : "/x"));
        rq.Headers.UserAgent.Add(new System.Net.Http.Headers.ProductInfoHeaderValue("PhotoReview", "update-check"));
        rq.Headers.Accept.Add(new System.Net.Http.Headers.MediaTypeWithQualityHeaderValue("application/vnd.github+json"));
        using var r = await c.SendAsync(rq, HttpCompletionOption.ResponseHeadersRead); result = "status " + (int)r.StatusCode + " location=" + r.Headers.Location; }
    catch (Exception e) { result = e.GetType().Name + " " + e.Message; }
    await Task.Delay(200);
    lock (seen) { Console.WriteLine("REQUESTS: " + seen.Count); if (mode != "redir") foreach (var x in seen) Console.WriteLine("  " + x); else Console.WriteLine("  last: " + seen[seen.Count - 1]); }
    Console.WriteLine("RESULT: " + result);
}

if (mode == "gzip")
{
    var l = new TcpListener(IPAddress.Loopback, 0); l.Start();
    int port = ((IPEndPoint)l.LocalEndpoint).Port;
    string CRLF = "\r\n";
    _ = Task.Run(async () => { while (true) { try { using var c = await l.AcceptTcpClientAsync(); var s = c.GetStream(); var buf = new byte[8192]; await s.ReadAsync(buf);
        await s.WriteAsync(Encoding.ASCII.GetBytes("HTTP/1.1 200 OK" + CRLF + "Content-Encoding: gzip" + CRLF + "Connection: close" + CRLF + "Content-Length: 3" + CRLF + CRLF + "abc")); } catch { } } });
    try { using var c = new HttpClient(); using var r = await c.GetAsync("http://127.0.0.1:" + port + "/x"); Console.WriteLine("RESULT: body=" + await r.Content.ReadAsStringAsync()); }
    catch (Exception e) { Console.WriteLine("RESULT: " + e.GetType().Name + " " + e.Message); }
}

```
