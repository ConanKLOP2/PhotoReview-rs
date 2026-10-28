# Isolated fake C# checkout and git pins; never reads or changes the real C# repo.
$ErrorActionPreference = 'Stop'
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ('stage-native-' + [guid]::NewGuid().ToString('N'))
$fixture = Join-Path $scratch 'PhotoReview\native'
$worktree = Join-Path $scratch 'PhotoReview-rs\.claude\worktrees\fixture'
$scriptDir = Join-Path $worktree 'tools\dev'
$script = Join-Path $scriptDir 'stage-native.ps1'
$wrapper = Join-Path $scratch 'invoke.ps1'
function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    Write-Output "ok $Message"
}
function Run-Stage([string[]]$StageArguments) {
    $stdout = Join-Path $scratch 'stdout.txt'
    $stderr = Join-Path $scratch 'stderr.txt'
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $wrapper + '"')) + $StageArguments
    $process = Start-Process powershell -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $processHandle = $process.Handle
    if (-not $process.WaitForExit(15000)) {
        $process.Kill()
        $process.WaitForExit()
        throw 'stage-native selftest child exceeded 15 seconds.'
    }
    $process.WaitForExit()
    return [pscustomobject]@{ Code = $process.ExitCode; Output = ((Get-Content $stdout -Raw) + (Get-Content $stderr -Raw)) }
}
try {
    New-Item -ItemType Directory -Path (Join-Path $fixture 'x64'), $scriptDir -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\stage-native.ps1') -Destination $script
    foreach ($name in @('libraw', 'turbojpeg')) {
        $dll = Join-Path $fixture "x64\$name.dll"
        [System.IO.File]::WriteAllBytes($dll, [System.Text.Encoding]::ASCII.GetBytes("fixture-$name"))
        (Get-FileHash $dll -Algorithm SHA256).Hash | Set-Content (Join-Path $fixture "$name.sha256") -Encoding ascii
    }
    # Mock only git, not hashes or filesystem operations. Assert the production baseline and main-checkout resolution.
    @'
param([string]$Destination, [switch]$Check)
$ErrorActionPreference = 'Stop'
function git {
    $global:LASTEXITCODE = 0
    if ($args.Count -eq 5 -and $args[0] -eq '-C' -and $args[2] -eq 'rev-parse' -and $args[3] -eq '--path-format=absolute' -and $args[4] -eq '--git-common-dir') {
        return (Join-Path $PSScriptRoot 'PhotoReview-rs\.git')
    }
    if ($args.Count -eq 4 -and $args[0] -eq '-C' -and $args[1] -eq (Join-Path $PSScriptRoot 'PhotoReview') -and $args[2] -eq 'show' -and $args[3] -match '^7eaee2a2:native/(libraw|turbojpeg)\.sha256$') {
        return (Get-Content (Join-Path $PSScriptRoot "PhotoReview\native\$($Matches[1]).sha256"))
    }
    throw "Unexpected git invocation: $args"
}
$stageParameters = @{ Check = $Check }
if ($Destination) { $stageParameters.Destination = $Destination }
& (Join-Path $PSScriptRoot 'PhotoReview-rs\.claude\worktrees\fixture\tools\dev\stage-native.ps1') @stageParameters
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $wrapper -Encoding ascii
    $dest = Join-Path $scratch 'destination with spaces'
    $destArgs = @('-Destination', ('"' + $dest + '"'))
    $badDll = Join-Path $fixture 'x64\turbojpeg.dll'
    $original = [System.IO.File]::ReadAllBytes($badDll)
    [System.IO.File]::WriteAllText($badDll, 'corrupt')
    $result = Run-Stage $destArgs
    if ($result.Code -ne 1 -or $result.Output -notmatch 'SHA-256 mismatch') { throw "Unexpected mismatch result: $($result.Code) $($result.Output)" }
    Assert ($result.Code -eq 1 -and $result.Output -match 'SHA-256 mismatch' -and -not (Test-Path $dest)) 'second DLL hash mismatch exits 1 before any copy'
    [System.IO.File]::WriteAllBytes($badDll, $original)
    $result = Run-Stage ($destArgs + @('-Check'))
    Assert ($result.Code -eq 0 -and -not (Test-Path $dest)) '-Check verifies without creating destination'
    $result = Run-Stage $destArgs
    Assert ($result.Code -eq 0) 'staging succeeds'
    foreach ($name in @('libraw', 'turbojpeg')) {
        $target = Join-Path $dest "$name.dll"
        Assert ((Test-Path $target) -and (Get-FileHash $target -Algorithm SHA256).Hash -eq (Get-Content (Join-Path $fixture "$name.sha256"))) "$name copied with matching bytes"
    }
    [System.IO.File]::WriteAllText((Join-Path $dest 'libraw.dll'), 'keep existing destination')
    $result = Run-Stage ($destArgs + @('-Check'))
    Assert ($result.Code -eq 0 -and [System.IO.File]::ReadAllText((Join-Path $dest 'libraw.dll')) -eq 'keep existing destination') '-Check leaves existing destination untouched'
    $result = Run-Stage @()
    Assert ($result.Code -eq 0 -and (Test-Path (Join-Path $worktree 'target\debug\libraw.dll')) -and (Test-Path (Join-Path $worktree 'target\debug\turbojpeg.dll'))) 'default destination belongs to current worktree'
    Write-Output 'stage-native selftest PASSED (7 checks)'
} finally {
    # Only this invocation owns this unique temporary directory.
    $resolved = [System.IO.Path]::GetFullPath($scratch)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempRoot, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe selftest cleanup path' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
