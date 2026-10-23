# W10-http decisions (lead draft)

Lead handoff: after reviewing these draft decisions, copy the three roster lines below into the Wave 10 section of `contracts/README.md` in H1/H2/H3 order. The contract writer leaves that shared file untouched.

## D-W10http-1: OS stack = WinHTTP through `windows` / windows-rs

The C# baseline creates `new HttpClient()` without a handler, gives it `Timeout.InfiniteTimeSpan`, and uses a linked token with `CancelAfter(10 s)` (`UpdateChecker.cs` 21-45). `PhotoReview.Shell.Win32.csproj` targets `net10.0-windows` (7-10). Its application code does not set the handler's proxy, redirect, TLS, decompression, or HTTP version options. .NET 10's `HttpClientHandler` uses SocketsHttpHandler; `HttpClient.DefaultProxy` on Windows checks `HTTPS_PROXY`/`ALL_PROXY` (and `NO_PROXY`) before the current user's proxy settings, including PAC/WPAD. This is a runtime default, not code in the repository. [DefaultProxy](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclient.defaultproxy), [HttpClientHandler](https://learn.microsoft.com/en-us/dotnet/fundamentals/runtime-libraries/system-net-http-httpclienthandler).

| Stack | Proxy and trust parity | Unsafe / cancellation | Binary size and fake tests |
|---|---|---|---|
| **WinHTTP (chosen)** | Windows certificate validation through the OS; explicitly implement .NET environment precedence and current-user WinINet/IE proxy settings with `WinHttpGetIEProxyConfigForCurrentUser` and PAC/WPAD via `WinHttpGetProxyForUrl`. WinHTTP's `PRECONFIG` alone is insufficient. Redirect policy and decompression need explicit settings. | Small FFI surface confined to `pr-platform-win`, every `unsafe` block justified. Use asynchronous request handles and close the **request** handle on cancel/deadline; completion races must be guarded. `WinHttpSetTimeouts` is phase protection, not the whole-exchange deadline. | Windows DLL, no new Rust HTTP dependency; pure policy and in-process loopback tests. |
| WinINet | Current-user proxy is close, but env precedence still manual; Windows trust. Intended for interactive clients, with more implicit state. | WinINet FFI and handle-cancel races; verify prompt cancellation before adopting. | OS DLL; fake protocol tests possible but global configuration harder to isolate. |
| Windows.Web.Http / WinRT | Windows trust and brokered settings, but .NET proxy and redirect defaults require measurement. | COM/WinRT lifetime and async cancellation mapping are larger than needed. | OS runtime; testing adapter without network is more complex. |
| Pure Rust HTTP client | New dependency plus explicit Windows trust and proxy/PAC integration; cannot assume parity from TLS backend alone. | Smaller local unsafe surface; cancellation depends on client. | Adds code and dependency tree; fake servers straightforward. |
| `curl.exe` / PowerShell child | External executable's proxy, TLS, redirects, headers and decompression differ by installation/configuration. | Kill child on cancellation; process cleanup and output limits. | No linked crate, but startup and distribution costs; difficult deterministic tests. |

WinHTTP is chosen because the Windows OS supplies certificate validation and PAC primitives without a new third-party crate. The porter must reproduce the documented .NET defaults explicitly, never assume WinHTTP and SocketsHttpHandler are interchangeable. [WinHTTP current-user proxy](https://learn.microsoft.com/en-us/windows/win32/winhttp/setting-wininet-proxy-configurations-in-winhttp), [WinHTTP cancellation](https://learn.microsoft.com/en-us/windows/win32/winhttp/concurrency-in-winhttp).

## D-W10http-2: preserve the existing checker and UI logic

Inject `Arc<UpdateChecker>` built from `Arc<WinHttpTransport>` into the existing `DialogServices.update_checker`. The C# presenter falls back to `new UpdateChecker()` when DI supplies none (`SettingsPresenter.cs` 575); the Rust presenter has no such fallback. Keep the merged UI worker/poster, localized status mapping, allow-listed URL validation and close cancellation. Only fix demonstrated gaps, with a fake or native test. Do not introduce another network request or an automatic check.

## D-W10http-3: HTTPS proxy source precedence (recommended, believed, not verified)

For an HTTPS target, choose a usable nonempty `HTTPS_PROXY`, else `ALL_PROXY`, else the current user's WinINet/IE configuration, else direct. Apply `NO_PROXY` or the user's bypass list after selecting the source. For current-user automatic configuration, call `WinHttpGetIEProxyConfigForCurrentUser` and resolve PAC/WPAD for the target with `WinHttpGetProxyForUrl`; WinHTTP machine `PRECONFIG` alone does not reproduce `HttpClient.DefaultProxy`. This is the recommended implementation rule, **believed but not verified** for empty/invalid environment values, bypass grammar and PAC edge cases. It follows the documented Windows `HttpClient.DefaultProxy` source order but is not yet a measured baseline result. [DefaultProxy](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclient.defaultproxy), [WinHTTP AutoProxy](https://learn.microsoft.com/en-us/windows/win32/winhttp/winhttp-autoproxy-api).

Lead verification probe: proposed `tools/oracle-cs/Commands/W10-H1.cs` should include a tiny .NET 10 command that prints `HttpClient.DefaultProxy.GetProxy` and `IsBypassed` for an HTTPS test URL under controlled `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, and current-user proxy scenarios, plus the default values of `HttpClientHandler.AllowAutoRedirect`, `MaxAutomaticRedirections`, `AutomaticDecompression`, `MaxResponseHeadersLength`, and `HttpRequestMessage.Version`. Run it only in a controlled environment; the file and probe are **proposed only, not created or run** by this writer. PAC/WPAD requires a separately controlled local PAC endpoint.

## Roster lines for `contracts/README.md`

```text
W10-H1 | HTTP redirect/proxy/deadline/error policy | contracts/W10-H1.md | F | H1
W10-H2 | WinHTTP transport and streamed body | contracts/W10-H2.md | F/N | H2 after H1
W10-H3 | Shell injection and manual check completion | contracts/W10-H3.md | F/N | H3 after H2
```

## Runtime defaults and uncertainties

Verified from Microsoft documentation: automatic redirects are enabled with a maximum of 50; .NET 5+ rejects automatic HTTPS-to-HTTP downgrade; `Authorization` is cleared on automatic redirect. [Redirects](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclienthandler.allowautoredirect), [limit](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclienthandler.maxautomaticredirections). C# `UpdateChecker.Classify` maps `NameResolutionError`, `ConnectionError`, `ProxyTunnelError`, and unknown to Offline, all other listed `HttpRequestError` values to BadResponse (`UpdateChecker.cs` 74-83).

The request is explicitly a `new HttpRequestMessage(...)`, whose default version is HTTP/1.1; `HttpClient.DefaultRequestVersion` does not override `SendAsync`'s request object. The default response-header limit is 64 KiB. The normal `HttpClient.Timeout` is 100 seconds but the C# checker explicitly replaces it with `Timeout.InfiniteTimeSpan` and a linked token canceled after ten seconds. [Request version](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httprequestmessage.version), [SendAsync version rule](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclient.defaultrequestversion), [header limit](https://learn.microsoft.com/en-us/dotnet/fundamentals/code-analysis/quality-rules/ca2262), [timeout](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclient.timeout).

**Not verified:** the exact .NET 10 default `AutomaticDecompression` value (the public page states `None` only for .NET Framework 4.x), request version policy, Windows trust store scope, exact redirect status/method edge cases, exact WinHTTP error-code mapping, whether proxy auto-discovery time is included in the WinHTTP deadline, and repository-wide absence of an external `HttpClient.DefaultProxy` setter. Probe these locally or inspect pinned runtime sources before implementation; do not encode guessed values as parity. The local tests must not call the internet.
