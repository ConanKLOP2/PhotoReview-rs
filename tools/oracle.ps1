<#
.SYNOPSIS
  Regenerates golden files (tests/golden/<command>.json) by running the C# PhotoReview code at a given commit.

.DESCRIPTION
  1. `git archive` the C# repo at -Commit into %TEMP%\pr-oracle\<sha> (outside every git repo, so the C# version
     target falls back cleanly and nothing in the C# repo is touched; `git archive` only reads).
  2. dotnet build PhotoReview.Core (Release) there, once per commit (cached).
  3. Build tools/oracle-cs against that DLL and run each command on tests/golden-inputs/<command>.json.
  Only the Golden Generator role runs this (docs/MULTI-AGENT-PLAN.md); porters never edit tests/golden/.

.EXAMPLE
  ./tools/oracle.ps1                       # every command, baseline from docs/SYNC-LEDGER.md
  ./tools/oracle.ps1 -Command natural-cmp -Commit origin/master
#>
param(
    [string]$CSharpRepo,
    [string]$Commit,
    [string[]]$Command
)
$ErrorActionPreference = 'Stop'
if (-not $CSharpRepo) {
    # Sibling of the MAIN checkout (also right when this runs from a worktree under .claude/worktrees/).
    $common = (& git -C $PSScriptRoot rev-parse --path-format=absolute --git-common-dir).Trim()
    $CSharpRepo = Join-Path (Split-Path (Split-Path $common)) 'PhotoReview'
}
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

if (-not $Commit) {
    $ledger = Get-Content (Join-Path $root 'docs\SYNC-LEDGER.md') -Raw
    if ($ledger -notmatch 'Current spec baseline:\*\* C# `master` at `([0-9a-f]+)`') { throw 'Cannot read the baseline from docs/SYNC-LEDGER.md' }
    $Commit = $Matches[1]
}
$sha = (& git -C $CSharpRepo rev-parse --verify "$Commit^{commit}").Trim()
if ($LASTEXITCODE -ne 0) { throw "Cannot resolve $Commit in $CSharpRepo" }

$work = Join-Path $env:TEMP "pr-oracle\$sha"
$coreDir = Join-Path $work 'core'
# PhotoReview.PerfAnalysis.dll (separate project, no references) and PhotoReview.Imaging.dll (W4: references Core only) are built
# into the same folder; rebuild if any of the DLLs is missing.
if (-not (Test-Path (Join-Path $coreDir 'PhotoReview.Core.dll')) -or -not (Test-Path (Join-Path $coreDir 'PhotoReview.PerfAnalysis.dll')) -or -not (Test-Path (Join-Path $coreDir 'PhotoReview.Imaging.dll'))) {
    $src = Join-Path $work 'src'
    if (Test-Path $src) { Remove-Item -Recurse -Force -Confirm:$false $src }
    New-Item -ItemType Directory -Force $src | Out-Null
    $tar = Join-Path $work 'src.tar'
    & git -C $CSharpRepo archive --format=tar -o $tar $sha
    if ($LASTEXITCODE -ne 0) { throw 'git archive failed' }
    & tar -xf $tar -C $src
    if ($LASTEXITCODE -ne 0) { throw 'tar extract failed' }
    & dotnet build (Join-Path $src 'src\PhotoReview.Core\PhotoReview.Core.csproj') -c Release -o $coreDir -nologo -v quiet
    if ($LASTEXITCODE -ne 0) { throw 'C# Core build failed' }
    & dotnet build (Join-Path $src 'src\PhotoReview.PerfAnalysis\PhotoReview.PerfAnalysis.csproj') -c Release -o $coreDir -nologo -v quiet
    if ($LASTEXITCODE -ne 0) { throw 'C# PerfAnalysis build failed' }
    & dotnet build (Join-Path $src 'src\PhotoReview.Imaging\PhotoReview.Imaging.csproj') -c Release -o $coreDir -nologo -v quiet
    if ($LASTEXITCODE -ne 0) { throw 'C# Imaging build failed' }
}

$oracleOut = Join-Path $work 'oracle'
& dotnet build (Join-Path $root 'tools\oracle-cs\OracleCs.csproj') -c Release -o $oracleOut -nologo -v quiet "-p:CSharpCoreDir=$coreDir"
if ($LASTEXITCODE -ne 0) { throw 'oracle-cs build failed' }

$inputs = Join-Path $root 'tests\golden-inputs'
if (-not $Command) { $Command = Get-ChildItem $inputs -Filter *.json | ForEach-Object BaseName }
foreach ($c in $Command) {
    & (Join-Path $oracleOut 'oracle-cs.exe') $c (Join-Path $inputs "$c.json") (Join-Path $root "tests\golden\$c.json") $sha
    if ($LASTEXITCODE -ne 0) { throw "oracle-cs $c failed" }
}
