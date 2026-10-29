# Windows PowerShell 5.1: compile inert console fixtures; no real benchmark executable or photo library.
$ErrorActionPreference = 'Stop'
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ('bench compare selftest ' + [guid]::NewGuid().ToString('N'))
$driver = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\bench-compare.ps1'))
$checks = 0
function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $script:checks++
    Write-Output "ok $Message"
}
function Run-Driver([string]$Name, [string[]]$Extra = @()) {
    $out = Join-Path $scratch $Name
    $stdout = Join-Path $scratch ($Name + '.stdout')
    $stderr = Join-Path $scratch ($Name + '.stderr')
    $arguments = @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $driver + '"'),'-BaselineExe',('"' + $baseline + '"'),'-CandidateExe',('"' + $candidate + '"'),'-Folder',('"' + $fixture + '"'),'-Out',('"' + $out + '"')) + $Extra
    $process = Start-Process powershell -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $handle = $process.Handle
    if (-not $process.WaitForExit(30000)) { $process.Kill(); $process.WaitForExit(); throw 'driver exceeded 30 seconds' }
    $process.WaitForExit()
    return [pscustomobject]@{ Code=$process.ExitCode; Text=([System.IO.File]::ReadAllText($stdout) + [System.IO.File]::ReadAllText($stderr)); Out=$out }
}
$saved = @{}
foreach ($name in @('PR_BENCH_SELFTEST_LOG','PR_BENCH_SELFTEST_CODE','PR_BENCH_SELFTEST_FAILURE')) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
try {
    New-Item -ItemType Directory -Path $scratch -Force | Out-Null
    $baseline = Join-Path $scratch 'baseline.exe'
    $candidate = Join-Path $scratch 'candidate.exe'
    $fixture = Join-Path $scratch 'empty fixture'
    New-Item -ItemType Directory -Path $fixture | Out-Null
    Add-Type -OutputAssembly $baseline -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.IO;
public static class BenchCompareSelftest {
    public static int Main(string[] args) {
        string side = Path.GetFileNameWithoutExtension(System.Reflection.Assembly.GetExecutingAssembly().Location);
        File.AppendAllText(Environment.GetEnvironmentVariable("PR_BENCH_SELFTEST_LOG"), side + "|" + String.Join("|", args) + "\n");
        if (args[0] == "--bench-compare") {
            if (Directory.GetFiles(args[1], "*.json").Length != 3 || Directory.GetFiles(args[2], "*.json").Length != 3) return 99;
            Console.WriteLine("# synthetic comparison report");
            return Int32.Parse(Environment.GetEnvironmentVariable("PR_BENCH_SELFTEST_CODE"));
        }
        if (args.Length != 3 || !Directory.Exists(args[1])) return 98;
        string failure = Environment.GetEnvironmentVariable("PR_BENCH_SELFTEST_FAILURE");
        if (failure == "execution") return 7;
        if (failure == "timeout") { new System.Threading.ManualResetEvent(false).WaitOne(); return 99; }
        string dir = args[0] == "--benchmark" ? Path.Combine(Path.GetDirectoryName(Environment.GetEnvironmentVariable("PR_BENCH_SELFTEST_LOG")), Guid.NewGuid().ToString("N")) : args[2];
        Directory.CreateDirectory(dir);
        string path = Path.Combine(dir, "summary.json");
        File.WriteAllText(path, "{\"synthetic\":true}");
        if (failure != "report") Console.WriteLine("REPORT: " + path);
        return 0;
    }
}
'@
    Copy-Item -LiteralPath $baseline -Destination $candidate
    $env:PR_BENCH_SELFTEST_LOG = Join-Path $scratch 'calls.txt'
    $env:PR_BENCH_SELFTEST_CODE = '0'
    $env:PR_BENCH_SELFTEST_FAILURE = ''
    $result = Run-Driver 'all output'
    Assert ($result.Code -eq 0 -and $result.Text -match '# synthetic comparison report') 'prints comparison report, returns 0, paths with spaces survive'
    $calls = [System.IO.File]::ReadAllLines($env:PR_BENCH_SELFTEST_LOG)
    $sides = @($calls | ForEach-Object { ($_ -split '\|')[0] }) -join ','
    Assert ($sides -eq 'baseline,candidate,candidate,baseline,baseline,candidate,candidate') 'sequential alternating pairs followed by candidate CLI comparison'
    Assert (@(Get-ChildItem (Join-Path $result.Out 'baseline') -Filter '*.json').Count -eq 3) 'baseline paired evidence copied'
    Assert (@(Get-ChildItem (Join-Path $result.Out 'candidate') -Filter '*.json').Count -eq 3) 'candidate paired evidence copied'
    Assert (($calls[0] -split '\|')[1] -eq '--benchmark-all') 'all mode passes output override'
    Assert (Test-Path -LiteralPath (Join-Path $result.Out 'invocation.json')) 'records invocation and executable hashes'
    [System.IO.File]::WriteAllText($env:PR_BENCH_SELFTEST_LOG, '')
    $env:PR_BENCH_SELFTEST_CODE = '1'
    $result = Run-Driver 'warm output' @('-Scenario','warm','-Profiles','preview-light')
    $calls = [System.IO.File]::ReadAllLines($env:PR_BENCH_SELFTEST_LOG)
    Assert ($result.Code -eq 1 -and $calls.Length -eq 9) 'one unmeasured warmup each, regression exit propagated'
    Assert (($calls[0] -split '\|')[1] -eq '--benchmark' -and ($calls[0] -split '\|')[3] -eq 'preview-light') 'profile list occupies third benchmark argument'
    $env:PR_BENCH_SELFTEST_CODE = '3'
    $result = Run-Driver 'actions output' @('-Profiles','actions')
    Assert ($result.Code -eq 3 -and $result.Text -match 'synthetic comparison report') 'incomparable comparison exit propagated'
    foreach ($failure in @('execution','report','timeout')) {
        $env:PR_BENCH_SELFTEST_FAILURE = $failure
        $result = Run-Driver ($failure + ' output') @('-TimeoutSeconds','1')
        Assert ($result.Code -eq 3 -and $result.Text -match 'bench-compare:') "$failure fails closed with exit 3"
    }
    $env:PR_BENCH_SELFTEST_FAILURE = ''
    $result = Run-Driver 'all output'
    Assert ($result.Code -eq 3 -and $result.Text -match 'new directory') 'existing evidence preserved'
    Write-Output "bench-compare selftest PASSED ($checks checks)"
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    $resolved = [System.IO.Path]::GetFullPath($scratch)
    $temp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($temp, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'unsafe selftest cleanup path' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
