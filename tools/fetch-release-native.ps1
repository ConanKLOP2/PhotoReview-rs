<# W10-R4: pinned packages only, no C# checkout on runners, no committed DLLs.
   -Verify is read-only/offline. PackageDirectory allows an offline cache, never bypasses pins. #>
[CmdletBinding()]
param([switch]$Verify, [string]$PackageDirectory)
. (Join-Path $PSScriptRoot 'release-common.ps1')
$root = Split-Path $PSScriptRoot
if (-not $PackageDirectory) { $PackageDirectory = Join-Path $root 'target\release-inputs' }
$specs = @(
    @{ Name='libraw'; Package='LibRaw-0.22.2-Win64.zip'; Url='https://www.libraw.org/data/LibRaw-0.22.2-Win64.zip'; Dll='LibRaw-0.22.2/bin/libraw.dll' },
    @{ Name='turbojpeg'; Package='libjpeg-turbo-native-windows.3.0.0.nupkg'; Url='https://www.nuget.org/api/v2/package/libjpeg-turbo-native-windows/3.0.0'; Dll='lib/net6.0/Turbojpeg.dll' }
)
# Validate every pin BEFORE network, extraction or staging. A missing TurboJPEG package pin blocks release.
foreach ($spec in $specs) {
    $spec.PackagePin = Read-ReleasePin (Join-Path $root "native\$($spec.Name).package.sha256")
    $spec.DllPin = Read-ReleasePin (Join-Path $root "native\$($spec.Name).sha256")
}
if ($Verify) {
    foreach ($spec in $specs) {
        Assert-ReleaseHash (Join-Path $PackageDirectory $spec.Package) $spec.PackagePin
        Assert-ReleaseHash (Join-Path $root "native\x64\$($spec.Name).dll") $spec.DllPin
    }
    Write-Output 'PASS: pinned native packages and DLLs (offline, read-only)'
    return
}
[void][IO.Directory]::CreateDirectory($PackageDirectory)
$scratch = Join-Path $PackageDirectory ('extract-' + [guid]::NewGuid().ToString('N'))
try {
    foreach ($spec in $specs) {
        $package = Join-Path $PackageDirectory $spec.Package
        if (-not (Test-Path -LiteralPath $package -PathType Leaf)) {
            # Bounded transport; HTTPS for the original request AND every redirect.
            & curl.exe --fail --location --proto '=https' --proto-redir '=https' --connect-timeout 20 --max-time 180 $spec.Url -o "$package.download"
            if ($LASTEXITCODE -ne 0) { throw "Native download failed: $($spec.Name)" }
            Assert-ReleaseHash "$package.download" $spec.PackagePin
            Move-Item -LiteralPath "$package.download" -Destination $package
        }
        Assert-ReleaseHash $package $spec.PackagePin
        $extract = Join-Path $scratch $spec.Name
        Expand-ReleaseArchive $package $extract
        Assert-ReleaseHash (Join-Path $extract $spec.Dll) $spec.DllPin
    }
    # Both packages and DLLs verified before staging either DLL.
    $dest = Join-Path $root 'native\x64'
    [void][IO.Directory]::CreateDirectory($dest)
    foreach ($spec in $specs) {
        $target = Join-Path $dest "$($spec.Name).dll"
        Copy-Item -LiteralPath (Join-Path (Join-Path $scratch $spec.Name) $spec.Dll) -Destination $target -Force
        Assert-ReleaseHash $target $spec.DllPin
    }
    Write-Output 'PASS: fetched and staged both pinned native DLLs'
} finally { Remove-ReleaseScratch $scratch $PackageDirectory }
