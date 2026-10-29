[CmdletBinding()]
param([string]$Path)
$ErrorActionPreference = 'Stop'
if (-not $Path) { $Path = Join-Path $PSScriptRoot '..\.github\workflows\release.yml' }
$text = [IO.File]::ReadAllText($Path)
if ($text -match 'secrets\s*\.') { throw 'Release workflow must not reference secrets' }
foreach ($line in $text -split "`n") {
    if ($line -match '^\s*(?:-\s*)?uses:\s*(\S+)') {
        if ($Matches[1] -cnotmatch '\A[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}\z') { throw "Unpinned action: $line" }
    }
}
if ($text -match '(?m)^\s{2}(pull_request|push|schedule|pull_request_target):' -or $text -notmatch '(?m)^  workflow_dispatch:') { throw 'Release must be manual dispatch only' }
foreach ($required in @('checks: read','contents: write','--verify-tag','build-test','continue-on-error: false','PHOTOREVIEW_RS_ISOLATE_CONFIG','fetch-release-native.ps1','verify-release-native.ps1','PHOTOREVIEW_RS_RELEASE','SHA256SUMS')) {
    if (-not $text.Contains($required)) { throw "Missing release gate: $required" }
}
if (Get-Command python -ErrorAction SilentlyContinue) {
    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & python -I -c 'import yaml' 2>$null
    $parserAvailable = $LASTEXITCODE -eq 0
    $ErrorActionPreference = $savedPreference
    if ($parserAvailable) {
        # BaseLoader preserves the YAML key `on` (PyYAML's default YAML 1.1 boolean conversion does not).
        & python -I (Join-Path $PSScriptRoot 'dev/check-release-yaml.py') $Path
        if ($LASTEXITCODE -ne 0) { throw 'YAML parser rejected release workflow/triggers' }
    } else { Write-Output 'NOT RUN: YAML parser unavailable; textual gates checked' }
} else { Write-Output 'NOT RUN: Python unavailable; textual gates checked' }
if (Get-Command actionlint -ErrorAction SilentlyContinue) {
    & actionlint $Path
    if ($LASTEXITCODE -ne 0) { throw 'actionlint failed' }
} else { Write-Output 'NOT RUN: actionlint not installed' }
Write-Output 'PASS: release workflow static safety gates'
