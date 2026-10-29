# W10-B1/B3. Explicit fixture and executables only; never discovers user photo folders.
# Example: powershell -NoProfile -ExecutionPolicy Bypass -File tools/bench-compare.ps1 `
#   -BaselineExe <absolute C# exe> -CandidateExe <absolute pr-bench.exe> `
#   -Folder <fixture> -Profiles preview-light -Out <new evidence directory>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$BaselineExe,
    [Parameter(Mandatory=$true)][string]$CandidateExe,
    [Parameter(Mandatory=$true)][string]$Folder,
    [Parameter(Mandatory=$true)][string]$Out,
    [string]$Profiles = 'all',
    [ValidateRange(3,1000)][int]$Repeats = 3,
    [ValidateSet('fresh-process','warm')][string]$Scenario = 'fresh-process',
    [string]$Thresholds,
    [ValidateRange(1,86400)][int]$TimeoutSeconds = 600
)
$ErrorActionPreference = 'Stop'

# Start-Process joins ArgumentList in Windows PowerShell 5.1. Apply Windows argv quoting,
# including trailing backslashes, rather than relying on PowerShell array quoting.
function Quote-Argument([string]$Value) {
    return '"' + [regex]::Replace([regex]::Replace($Value, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
}
function Invoke-Bounded([string]$Exe, [string[]]$Arguments, [string]$Label) {
    $stdout = Join-Path $Out ($Label + '.stdout.txt')
    $stderr = Join-Path $Out ($Label + '.stderr.txt')
    $quoted = @($Arguments | ForEach-Object { Quote-Argument $_ })
    $process = Start-Process -FilePath $Exe -ArgumentList $quoted -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $handle = $process.Handle
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill()
        $process.WaitForExit()
        throw "$Label exceeded $TimeoutSeconds seconds"
    }
    $process.WaitForExit()
    return [pscustomobject]@{ Code = $process.ExitCode; Text = [System.IO.File]::ReadAllText($stdout); ErrorText = [System.IO.File]::ReadAllText($stderr) }
}
function Invoke-Batch([string]$Exe, [string]$Side, [string]$Label, [bool]$Measured) {
    $outputDirectory = Join-Path $Out ($Label + '-batch')
    if ($Profiles -eq 'all') { $batchArgs = @('--benchmark-all', $Folder, $outputDirectory) }
    elseif ($Profiles -eq 'actions') { $batchArgs = @('--benchmark-actions', $Folder, $outputDirectory) }
    else { $batchArgs = @('--benchmark', $Folder, $Profiles) }
    $result = Invoke-Bounded $Exe $batchArgs $Label
    if ($result.Code -ne 0) { throw "$Label exited $($result.Code): $($result.ErrorText)" }
    $reports = @([regex]::Matches($result.Text, '(?m)^REPORT:\s*(.+?)\r?$'))
    if ($reports.Count -eq 0) { throw "$Label did not print a REPORT: path" }
    $summary = $reports[$reports.Count - 1].Groups[1].Value.Trim()
    if (-not [System.IO.Path]::IsPathRooted($summary)) { throw "$Label REPORT path must be absolute" }
    if (Test-Path -LiteralPath $summary -PathType Container) { $summary = Join-Path $summary 'summary.json' }
    if (-not (Test-Path -LiteralPath $summary -PathType Leaf)) { throw "$Label summary missing: $summary" }
    if ($Measured) {
        Copy-Item -LiteralPath $summary -Destination (Join-Path (Join-Path $Out $Side) ($Label.Substring(0,4) + '.json'))
    }
}
try {
    foreach ($exe in @($BaselineExe, $CandidateExe)) {
        if (-not [System.IO.Path]::IsPathRooted($exe) -or -not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Executable must be an existing absolute path: $exe" }
    }
    $Folder = (Resolve-Path -LiteralPath $Folder).ProviderPath
    if (-not (Test-Path -LiteralPath $Folder -PathType Container)) { throw 'Folder must be a fixture directory' }
    $Out = [System.IO.Path]::GetFullPath($Out)
    if (Test-Path -LiteralPath $Out) { throw 'Out must be a new directory, preserving previous evidence' }
    if ($Thresholds) { $Thresholds = (Resolve-Path -LiteralPath $Thresholds).ProviderPath }
    New-Item -ItemType Directory -Path (Join-Path $Out 'baseline'), (Join-Path $Out 'candidate') -Force | Out-Null
    # Trace only actual invocation facts. This is not an attended B2 machine record or a disk-cold claim.
    $record = [ordered]@{ baselineExe=$BaselineExe; candidateExe=$CandidateExe; baselineSha256=(Get-FileHash -LiteralPath $BaselineExe -Algorithm SHA256).Hash; candidateSha256=(Get-FileHash -LiteralPath $CandidateExe -Algorithm SHA256).Hash; folder=$Folder; profiles=$Profiles; repeats=$Repeats; scenario=$Scenario; method='Sequential, alternating order; no OS cache clearing'; rustEnvironment=@(Get-ChildItem Env:PHOTOREVIEW_RS_* | Select-Object Name,Value) }
    $record | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Out 'invocation.json') -Encoding UTF8
    if ($Scenario -eq 'warm') {
        Invoke-Batch $BaselineExe 'baseline' 'warm-baseline' $false
        Invoke-Batch $CandidateExe 'candidate' 'warm-candidate' $false
    }
    for ($i=0; $i -lt $Repeats; $i++) {
        $pair = '{0:D4}' -f $i
        if ($i % 2 -eq 0) {
            Invoke-Batch $BaselineExe 'baseline' ($pair + '-baseline') $true
            Invoke-Batch $CandidateExe 'candidate' ($pair + '-candidate') $true
        } else {
            Invoke-Batch $CandidateExe 'candidate' ($pair + '-candidate') $true
            Invoke-Batch $BaselineExe 'baseline' ($pair + '-baseline') $true
        }
    }
    $compareArgs = @('--bench-compare', (Join-Path $Out 'baseline'), (Join-Path $Out 'candidate'), '--out', (Join-Path $Out 'report'))
    if ($Thresholds) { $compareArgs += @('--thresholds', $Thresholds) }
    $result = Invoke-Bounded $CandidateExe $compareArgs 'compare'
    Write-Output $result.Text
    if ($result.ErrorText) { [Console]::Error.WriteLine($result.ErrorText) }
    if ($result.Code -notin @(0,1,2,3)) { throw "comparison execution failed: $($result.Code)" }
    exit $result.Code
} catch {
    [Console]::Error.WriteLine("bench-compare: $($_.Exception.Message)")
    exit 3
}
