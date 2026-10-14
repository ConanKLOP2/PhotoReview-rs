<#
.SYNOPSIS
  Lead tooling for multi-agent waves (docs/MULTI-AGENT-PLAN.md): per-WU worktrees and the integration gate.

.EXAMPLE
  ./tools/wave.ps1 -Wave 1 -Start W1-06            # worktree .claude/worktrees/W1-06 on wu/W1-06 from integration/wave-1
  ./tools/wave.ps1 -Wave 1 -Gate                   # full gate on the current checkout
  ./tools/wave.ps1 -Wave 1 -Gate -Crate pr-core    # quick gate for one crate (porter self-check)
  ./tools/wave.ps1 -Wave 1 -Cleanup W1-06          # remove the WU worktree and its local branch after merge
#>
param(
    [Parameter(Mandatory)][int]$Wave,
    [string]$Start,
    [switch]$Gate,
    [string]$Crate,
    [string]$Cleanup
)
$ErrorActionPreference = 'Stop'
$root = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
$common = (& git -C $PSScriptRoot rev-parse --path-format=absolute --git-common-dir).Trim()
$main = Split-Path $common
$integration = "integration/wave-$Wave"
# One shared target dir per wave so 8 parallel porters do not each build from scratch.
$env:CARGO_TARGET_DIR = Join-Path $main "target-wave-$Wave"

function Invoke-Step([string]$name, [scriptblock]$body) {
    Write-Host "== $name"
    # cargo/git report progress on stderr; Windows PowerShell 5.1 would turn that into a terminating error under
    # 'Stop'. The exit code is the only success signal.
    $ErrorActionPreference = 'Continue'
    & $body 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { throw "$name failed (exit $LASTEXITCODE)" }
}

if ($Start) {
    & git -C $main fetch -q origin
    $base = if (& git -C $main ls-remote --heads origin $integration) { "origin/$integration" } else { 'origin/master' }
    $path = Join-Path $main ".claude\worktrees\$Start"
    Invoke-Step "worktree $Start from $base" { git -C $main worktree add -q $path -b "wu/$Start" $base }
    Write-Host "Worktree: $path"
    Write-Host "CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR"
    return
}

if ($Cleanup) {
    $path = Join-Path $main ".claude\worktrees\$Cleanup"
    Invoke-Step "remove worktree $Cleanup" { git -C $main worktree remove $path }
    Invoke-Step "delete branch wu/$Cleanup" { git -C $main branch -D "wu/$Cleanup" }
    return
}

if ($Gate) {
    Push-Location $root
    try {
        Invoke-Step 'fmt' { cargo fmt --all --check }
        if ($Crate) {
            Invoke-Step "clippy $Crate" { cargo clippy -p $Crate --all-targets --locked -- -D warnings }
            Invoke-Step "test $Crate" { cargo test -p $Crate --locked }
            return
        }
        Invoke-Step 'clippy' { cargo clippy --workspace --all-targets --locked -- -D warnings }
        Invoke-Step 'test' { cargo test --workspace --locked }
        Invoke-Step 'synced assets' { powershell -NoProfile -File (Join-Path $root 'tools\sync-from-csharp.ps1') -Check }
        $goldenChanges = & git diff --name-only origin/master -- tests/golden tests/golden-inputs
        if ($goldenChanges) {
            Write-Host "Golden files changed vs origin/master (only the Golden Generator may do this):"
            $goldenChanges | ForEach-Object { Write-Host "  $_" }
        }
        Write-Host 'GATE PASSED'
    } finally {
        Pop-Location
    }
    return
}

throw 'Pass -Start <WU>, -Gate or -Cleanup <WU>.'
