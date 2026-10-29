# Offline killers for the release PE reader; synthetic bytes only, no executable loading.
param([string]$ReaderScript = (Join-Path $PSScriptRoot '../../release-pe.ps1'))
$ErrorActionPreference = 'Stop'
. $ReaderScript
function New-PeFixture {
    $bytes = [byte[]]::new(640)
    function Put16($at, $value) { [BitConverter]::GetBytes([uint16]$value).CopyTo($bytes,$at) }
    function Put32($at, $value) { [BitConverter]::GetBytes([uint32]$value).CopyTo($bytes,$at) }
    Put16 0 0x5a4d
    Put32 60 64
    Put32 64 0x4550
    Put16 68 0x8664
    Put16 70 1
    Put16 84 240
    Put16 88 0x20b
    Put32 216 4096
    Put32 340 4096
    Put32 344 128
    Put32 348 384
    Put16 396 0
    Put16 398 3
    Put32 400 14
    Put32 408 16
    Put32 416 24
    return ,$bytes
}
function Assert-Rejected([byte[]]$Bytes, [string]$Name) {
    $rejected = $false
    try { Get-ReleasePeResources $Bytes | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw "Accepted invalid PE: $Name" }
}
# release-pe.ps1:7-11, deleting MZ/PE/AMD64/PE32+ validation must be observed.
$valid = New-PeFixture
$types = Get-ReleasePeResources $valid
if (($types -join ',') -cne '14,16,24') { throw "Wrong resource types: $types" }
foreach ($at in @(0,64,68,88)) {
    $bytes = New-PeFixture
    $bytes[$at] = 0
    Assert-Rejected $bytes "signature at $at"
}
# release-pe.ps1:19, >= -> > excludes a resource RVA at the section start.
# release-pe.ps1:19, < -> <= incorrectly accepts the first byte after a section.
$bytes = New-PeFixture
[BitConverter]::GetBytes([uint32]4224).CopyTo($bytes,216)
# Keep a readable tree beyond the section so rejection proves the RVA comparison,
# rather than an incidental truncated-buffer exception.
[Array]::Copy($bytes,384,$bytes,512,48)
Assert-Rejected $bytes 'resource RVA at exclusive section end'
# release-pe.ps1:21, deleting the missing-resource guard accepts an unmapped RVA.
$bytes = New-PeFixture
[BitConverter]::GetBytes([uint32]4095).CopyTo($bytes,216)
Assert-Rejected $bytes 'resource RVA before section'
# release-pe.ps1:3-4, deleting truncated-word/dword guards must still reject data.
foreach ($length in @(0,1,60,63,65,85,89,217,349,399,419)) {
    $bytes = [byte[]]::new($length)
    [Array]::Copy($valid,$bytes,$length)
    Assert-Rejected $bytes "truncated at $length"
}
Write-Output 'PASS: synthetic PE resources, 4 signatures, 2 RVA bounds, 11 truncations (18 checks)'
