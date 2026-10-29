[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$Tag,
      [string]$ReleaseDirectory,
      [string]$OutputDirectory)
if (-not $ReleaseDirectory) { $ReleaseDirectory = Join-Path $PSScriptRoot '..\release-stage' }
if (-not $OutputDirectory) { $OutputDirectory = Split-Path $PSScriptRoot }
. (Join-Path $PSScriptRoot 'release-common.ps1')
Assert-ReleaseTag $Tag
& (Join-Path $PSScriptRoot 'verify-release-native.ps1') -ReleaseDirectory $ReleaseDirectory -Version $Tag.Substring(1)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zipPath = Join-Path $OutputDirectory "PhotoReviewRs-$Tag-win-x64.zip"
# No overwrite. CreateFromDirectory includes exactly the directory contents without a parent folder.
[IO.Compression.ZipFile]::CreateFromDirectory([IO.Path]::GetFullPath($ReleaseDirectory), $zipPath)
$zip = [IO.Compression.ZipFile]::OpenRead($zipPath)
try {
    $prefix = [IO.Path]::GetFullPath($ReleaseDirectory).TrimEnd('\') + '\'
    $expected = @(Get-ChildItem -LiteralPath $ReleaseDirectory -Recurse -File | ForEach-Object { $_.FullName.Substring($prefix.Length).Replace('\','/') } | Sort-Object)
    $actual = @($zip.Entries | Where-Object { -not $_.FullName.EndsWith('/') -and -not $_.FullName.EndsWith('\') } | ForEach-Object { $_.FullName.Replace('\','/') } | Sort-Object)
    if (@(Compare-Object $expected $actual -CaseSensitive).Count -ne 0) { throw 'Zip entry list differs from verified stage' }
    $actual | Write-Output
} finally { $zip.Dispose() }
$lines = @()
foreach ($name in @('PhotoReviewRs.exe','libraw.dll','turbojpeg.dll','LibRaw-SOURCE.zip')) {
    $lines += "$((Get-FileHash -LiteralPath (Join-Path $ReleaseDirectory $name) -Algorithm SHA256).Hash)  $name"
}
$lines += "$((Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash)  $([IO.Path]::GetFileName($zipPath))"
[IO.File]::WriteAllLines((Join-Path $OutputDirectory 'SHA256SUMS'), $lines, [Text.UTF8Encoding]::new($false))
Write-Output 'PASS: exact zip entries and SHA256SUMS'
