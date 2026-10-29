$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
. (Join-Path $repo 'tools/release-common.ps1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('release-workflow-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
try {
    $path = Join-Path $scratch 'release.yml'
    $workflow = [IO.File]::ReadAllText((Join-Path $repo '.github/workflows/release.yml'))
    foreach ($case in @(
        @{ Name='secret'; Text=$workflow.Replace('github.token','secrets.TOKEN'); Error='secrets' },
        @{ Name='action pin'; Text=$workflow.Replace('3d3c42e5aac5ba805825da76410c181273ba90b1','v7'); Error='Unpinned' },
        @{ Name='PR trigger'; Text=$workflow.Replace('  workflow_dispatch:',"  pull_request:`n  workflow_dispatch:"); Error='manual dispatch' },
        @{ Name='smoke gate'; Text=$workflow.Replace('continue-on-error: false','continue-on-error: true'); Error='Missing release gate' }
    )) {
        [IO.File]::WriteAllText($path,$case.Text,[Text.UTF8Encoding]::new($false))
        $failure = ''
        try { & (Join-Path $repo 'tools/check-release-workflow.ps1') -Path $path | Out-Null } catch { $failure=$_.Exception.Message }
        if ($failure -notmatch $case.Error) { throw "FAIL $($case.Name): $failure" }
        Write-Output "ok rejects $($case.Name)"
    }
    & (Join-Path $repo 'tools/check-release-workflow.ps1')
    Write-Output 'release workflow selftest PASSED (4 rejection cases + real workflow)'
} finally { Remove-ReleaseScratch $scratch ([IO.Path]::GetTempPath()) }
# Explicit success code: child processes started by the rejection cases leave a non-zero $LASTEXITCODE behind, which the CI step would otherwise report as a failure.
exit 0
