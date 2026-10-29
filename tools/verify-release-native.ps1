[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$ReleaseDirectory,
      [Parameter(Mandatory=$true)][string]$Version,
      [string]$SizePolicy)
if (-not $SizePolicy) { $SizePolicy = Join-Path $PSScriptRoot '..\native\release-size.json' }
. (Join-Path $PSScriptRoot 'release-common.ps1')
. (Join-Path $PSScriptRoot 'release-pe.ps1')
Assert-ReleaseTag "v$Version"
$root = Split-Path $PSScriptRoot
$review = Get-Content -LiteralPath (Join-Path $root 'native/release-review.json') -Raw | ConvertFrom-Json
foreach ($gate in @('native_notices_reviewed','libraw_source_offer_reviewed','application_license_reviewed','languages_runtime_validated')) {
    if ($review.$gate -isnot [bool] -or -not $review.$gate) { throw "Release review not completed: $gate (P-54/P-55)" }
}
$required = @('PhotoReviewRs.exe','libraw.dll','turbojpeg.dll','LibRaw-LICENSE.LGPL','LibRaw-LICENSE.CDDL',
    'LibRaw-NOTICE.txt','LibRaw-SOURCE.zip','TurboJPEG-NOTICE.txt','LICENSE','THIRD-PARTY-NOTICES.md','Languages/en.json','Languages/vi.json')
foreach ($name in $required) {
    $file = Join-Path $ReleaseDirectory $name
    if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -eq 0) { throw "Missing/empty release file: $name" }
}
# An allowlist also rejects every C# / CLR / WPF payload, debug file and unexpected subdirectory file.
$prefix = [IO.Path]::GetFullPath($ReleaseDirectory).TrimEnd('\') + '\'
foreach ($directory in Get-ChildItem -LiteralPath $ReleaseDirectory -Recurse -Directory) {
    if ($directory.FullName.Substring($prefix.Length) -cne 'Languages') { throw "Forbidden/unexpected release directory: $($directory.Name)" }
}
foreach ($file in Get-ChildItem -LiteralPath $ReleaseDirectory -Recurse -File) {
    $name = $file.FullName.Substring($prefix.Length).Replace('\','/')
    if ($required -cnotcontains $name) { throw "Forbidden/unexpected release file: $name" }
}
Assert-ReleaseHash (Join-Path $ReleaseDirectory 'libraw.dll') (Read-ReleasePin (Join-Path $root 'native/libraw.sha256'))
Assert-ReleaseHash (Join-Path $ReleaseDirectory 'turbojpeg.dll') (Read-ReleasePin (Join-Path $root 'native/turbojpeg.sha256'))
Assert-ReleaseHash (Join-Path $ReleaseDirectory 'LibRaw-SOURCE.zip') (Read-ReleasePin (Join-Path $root 'native/libraw.package.sha256'))
$map = @{
    'LibRaw-LICENSE.LGPL'='native/libraw/LICENSE.LGPL'; 'LibRaw-LICENSE.CDDL'='native/libraw/LICENSE.CDDL';
    'LibRaw-NOTICE.txt'='native/libraw/NOTICE.txt'; 'TurboJPEG-NOTICE.txt'='native/turbojpeg/NOTICE.txt';
    'LICENSE'='LICENSE'; 'THIRD-PARTY-NOTICES.md'='THIRD-PARTY-NOTICES.md';
    'Languages/en.json'='assets/languages/en.json'; 'Languages/vi.json'='assets/languages/vi.json'
}
foreach ($name in $map.Keys) {
    Assert-ReleaseHash (Join-Path $ReleaseDirectory $name) ((Get-FileHash -LiteralPath (Join-Path $root $map[$name]) -Algorithm SHA256).Hash)
}
$policy = Get-Content -LiteralPath $SizePolicy -Raw | ConvertFrom-Json
if ($null -eq $policy.min_bytes -or $null -eq $policy.max_bytes -or [long]$policy.min_bytes -le 0 -or [long]$policy.max_bytes -lt [long]$policy.min_bytes -or [string]::IsNullOrWhiteSpace($policy.measured_commit)) { throw 'Release exe size bounds must come from a measured Rust release build' }
$exe = Join-Path $ReleaseDirectory 'PhotoReviewRs.exe'
$length = (Get-Item -LiteralPath $exe).Length
if ($length -lt [long]$policy.min_bytes -or $length -gt [long]$policy.max_bytes) { throw "Implausible exe length: $length" }
Assert-ReleasePe $exe $Version
Write-Output 'PASS: release hashes, notices, layout, measured size, icon, manifest and exact version'
