# W10 release helpers, Windows PowerShell 5.1. No ambient working-directory lookup.
Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
function Read-ReleasePin([string]$Path) {
    $pin = [IO.File]::ReadAllText($Path).Trim()
    if ($pin -cnotmatch '\A[0-9a-fA-F]{64}\z') { throw "Missing/invalid reviewed SHA-256 pin: $Path" }
    return $pin
}
function Assert-ReleaseHash([string]$Path, [string]$Pin) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing release file: $Path" }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Pin) { throw "SHA-256 mismatch: $Path" }
}
function Assert-ReleaseTag([string]$Tag) {
    if ($Tag -cnotmatch '\Av(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\z') { throw 'Invalid release tag: expected canonical ASCII vX.Y.Z' }
    foreach ($part in $Tag.Substring(1).Split('.')) {
        $number = 0
        if (-not [int]::TryParse($part, [ref]$number) -or $number -gt 65535) { throw 'Release component exceeds Win32 version range' }
    }
}
function Expand-ReleaseArchive([string]$Package, [string]$Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $root = [IO.Path]::GetFullPath($Destination).TrimEnd('\') + '\'
    $zip = [IO.Compression.ZipFile]::OpenRead($Package)
    try {
        $seen = @{}
        # Validate ALL names before writing even one entry (including NTFS ADS / drive names).
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('\', '/')
            if ($name -match '(^/|:|(^|/)\.\.(/|$))' -or $name -match '[. ](/|$)') { throw "Unsafe archive entry: $name" }
            $full = [IO.Path]::GetFullPath((Join-Path $Destination $name))
            if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) -or $seen.ContainsKey($full)) { throw "Unsafe/duplicate archive entry: $name" }
            $seen[$full] = $true
        }
        foreach ($entry in $zip.Entries) {
            $full = [IO.Path]::GetFullPath((Join-Path $Destination $entry.FullName))
            if ($entry.FullName.EndsWith('/') -or $entry.FullName.EndsWith('\')) { [void][IO.Directory]::CreateDirectory($full); continue }
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($full))
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $full, $false)
        }
    } finally { $zip.Dispose() }
}
function Remove-ReleaseScratch([string]$Path, [string]$Parent) {
    $resolved = [IO.Path]::GetFullPath($Path)
    $prefix = [IO.Path]::GetFullPath($Parent).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe scratch cleanup path' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
function Invoke-ReleaseProcess([string]$FilePath, [string[]]$Arguments, [int]$TimeoutSeconds = 120) {
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ('pr-release-process-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($scratch)
    try {
        $stdout = Join-Path $scratch 'stdout'; $stderr = Join-Path $scratch 'stderr'
        $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $handle = $process.Handle
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            # Only this invocation's child is owned here.
            $process.Kill(); $process.WaitForExit()
            throw "Process exceeded $TimeoutSeconds seconds: $FilePath"
        }
        $process.WaitForExit()
        Get-Content -LiteralPath $stdout
        Get-Content -LiteralPath $stderr
        if ($process.ExitCode -ne 0) { throw "Process exited $($process.ExitCode): $FilePath" }
    } finally { Remove-ReleaseScratch $scratch ([IO.Path]::GetTempPath()) }
}
