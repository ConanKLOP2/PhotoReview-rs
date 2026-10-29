# Pinned release inputs

W10-R4. Text pins and LibRaw licence/notice files come from read-only C# baseline
`7eaee2a2`; no DLL is tracked. The DLLs are loaded dynamically from absolute executable
siblings. [Release instructions and outstanding reviews](../docs/RELEASING.md).

| Input | Version | Verification |
|---|---|---|
| LibRaw DLL | 0.22.2, x64 | `libraw.sha256` |
| LibRaw source/binary package | LibRaw-0.22.2-Win64.zip | `libraw.package.sha256` |
| TurboJPEG DLL | 3.0.0, x64 | `turbojpeg.sha256` |
| TurboJPEG NuGet package | libjpeg-turbo-native-windows 3.0.0 | `turbojpeg.package.sha256`, deliberately blocked until verified |

`tools/fetch-release-native.ps1` accepts an optional offline package directory, checks
package hashes before safe extraction, then checks both DLL hashes before staging.
Default packages live in ignored `target/release-inputs`, DLLs in ignored `native/x64`.
`-Verify` performs no network or writes. Review/size policy files intentionally block
publication while incomplete. Stage-local native verification is separate from Cargo's
dependency audit and does not establish legal compliance by itself.
