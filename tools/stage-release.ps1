[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$Version,
      [string]$Destination)
if (-not $Destination) { $Destination = Join-Path $PSScriptRoot '..\release-stage' }
. (Join-Path $PSScriptRoot 'release-common.ps1')
Assert-ReleaseTag "v$Version"
$root = Split-Path $PSScriptRoot
if (Test-Path -LiteralPath $Destination) { throw 'Release destination must not exist (refusing stale files)' }
$map = @{
    'PhotoReviewRs.exe'='target/release/PhotoReviewRs.exe'; 'libraw.dll'='native/x64/libraw.dll'; 'turbojpeg.dll'='native/x64/turbojpeg.dll';
    'LibRaw-LICENSE.LGPL'='native/libraw/LICENSE.LGPL'; 'LibRaw-LICENSE.CDDL'='native/libraw/LICENSE.CDDL'; 'LibRaw-NOTICE.txt'='native/libraw/NOTICE.txt';
    'LibRaw-SOURCE.zip'='target/release-inputs/LibRaw-0.22.2-Win64.zip'; 'TurboJPEG-NOTICE.txt'='native/turbojpeg/NOTICE.txt';
    'LICENSE'='LICENSE'; 'THIRD-PARTY-NOTICES.md'='THIRD-PARTY-NOTICES.md'; 'Languages/en.json'='assets/languages/en.json'; 'Languages/vi.json'='assets/languages/vi.json'
}
foreach ($source in $map.Values) {
    if (-not (Test-Path -LiteralPath (Join-Path $root $source) -PathType Leaf)) { throw "Missing release input: $source" }
}
[void][IO.Directory]::CreateDirectory((Join-Path $Destination 'Languages'))
foreach ($name in $map.Keys) { Copy-Item -LiteralPath (Join-Path $root $map[$name]) -Destination (Join-Path $Destination $name) }
& (Join-Path $PSScriptRoot 'verify-release-native.ps1') -ReleaseDirectory $Destination -Version $Version
