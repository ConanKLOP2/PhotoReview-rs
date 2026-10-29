# W10-R1/R3/R4: owned fixture packages, fake gh API, real hashes/zip extraction.
# No network, C# repository, user's DLLs or application data are touched.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
. (Join-Path $repo 'tools/release-common.ps1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('release-selftest-' + [guid]::NewGuid().ToString('N'))
$checks = 0
function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }; $script:checks++; Write-Output "ok $Message"
}
function Reject([scriptblock]$Action, [string]$Pattern, [string]$Message) {
    $errorText = ''
    try { & $Action | Out-Null } catch { $errorText = $_.Exception.Message }
    Assert ($errorText -match $Pattern) "$Message [$errorText]"
}
function Write-Utf8([string]$Path, [string]$Text) {
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path))
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
function Make-Zip([string]$Path, [hashtable]$Entries) {
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::Open($Path, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($name in $Entries.Keys) {
            $stream = $zip.CreateEntry($name).Open()
            try { $bytes = [Text.Encoding]::UTF8.GetBytes($Entries[$name]); $stream.Write($bytes,0,$bytes.Length) } finally { $stream.Dispose() }
        }
    } finally { $zip.Dispose() }
}
try {
    [void][IO.Directory]::CreateDirectory($scratch)
    foreach ($tag in @('v0.1.0','v65535.65535.65535')) { Assert-ReleaseTag $tag; Assert $true "valid tag $tag" }
    foreach ($tag in @("v1.2.3`n",'v01.2.3','v65536.0.0','v1.2.3-rc','v1.2',('v'+[char]0xff11+'.2.3'),'v1.2.3;whoami')) {
        Reject { Assert-ReleaseTag $tag } 'Invalid|exceeds' "reject tag $tag"
    }
    foreach ($name in @('../escape','/absolute','C:/drive','file:stream','ok/../../escape','trailing./file')) {
        $zipPath = Join-Path $scratch ([guid]::NewGuid().ToString('N') + '.zip')
        Make-Zip $zipPath @{ 'safe/file'='safe'; $name='bad' }
        $extract = Join-Path $scratch 'rejected-extract'
        Reject { Expand-ReleaseArchive $zipPath $extract } 'Unsafe' "reject archive $name"
        Assert (-not (Test-Path -LiteralPath $extract)) 'archive validated before any writes'
    }
    $fixture = Join-Path $scratch 'fixture'
    $tools = Join-Path $fixture 'tools'
    [void][IO.Directory]::CreateDirectory($tools)
    foreach ($name in @('release-common.ps1','fetch-release-native.ps1','verify-release-native.ps1','stage-release.ps1','pack-release.ps1')) {
        Copy-Item -LiteralPath (Join-Path $repo "tools/$name") -Destination $tools
    }
    # Only the PE call is substituted in fixture packaging tests. Linked PE tests use the real writer/parser.
    Write-Utf8 (Join-Path $tools 'release-pe.ps1') 'function Assert-ReleasePe($Path,$Version) { if ($Version -cne ''1.2.3'') { throw ''wrong fixture PE version'' } }'
    $cache = Join-Path $fixture 'target/release-inputs'
    [void][IO.Directory]::CreateDirectory($cache)
    $rawPackage = Join-Path $cache 'LibRaw-0.22.2-Win64.zip'
    $jpegPackage = Join-Path $cache 'libjpeg-turbo-native-windows.3.0.0.nupkg'
    Make-Zip $rawPackage @{ 'LibRaw-0.22.2/bin/libraw.dll'='fixture raw'; 'LibRaw-0.22.2/src/raw.cpp'='fixture source' }
    Make-Zip $jpegPackage @{ 'lib/net6.0/Turbojpeg.dll'='fixture jpeg' }
    foreach ($pair in @(@('libraw',$rawPackage,'fixture raw'),@('turbojpeg',$jpegPackage,'fixture jpeg'))) {
        Write-Utf8 (Join-Path $fixture "native/$($pair[0]).package.sha256") (Get-FileHash -LiteralPath $pair[1] -Algorithm SHA256).Hash
        $tmp = Join-Path $scratch 'bytes'
        Write-Utf8 $tmp $pair[2]
        Write-Utf8 (Join-Path $fixture "native/$($pair[0]).sha256") (Get-FileHash -LiteralPath $tmp -Algorithm SHA256).Hash
    }
    $fetch = Join-Path $tools 'fetch-release-native.ps1'
    Reject { & $fetch -Verify } 'Missing' 'verify cannot fetch missing DLLs'
    & $fetch
    & $fetch -Verify
    Assert ((Get-Content -LiteralPath (Join-Path $fixture 'native/x64/libraw.dll') -Raw) -ceq 'fixture raw') 'offline packages stage exact verified DLL bytes'
    $badPin = Join-Path $fixture 'native/turbojpeg.package.sha256'
    $originalPin = [IO.File]::ReadAllText($badPin)
    Write-Utf8 $badPin ('0' * 64)
    Reject { & $fetch } 'SHA-256 mismatch' 'package hash mismatch is fatal'
    Write-Utf8 $badPin '# not reviewed'
    Reject { & $fetch } 'reviewed SHA-256' 'missing package pin is fatal before network'
    Write-Utf8 $badPin $originalPin
    foreach ($file in @('native/libraw/LICENSE.LGPL','native/libraw/LICENSE.CDDL','native/libraw/NOTICE.txt','native/turbojpeg/NOTICE.txt','LICENSE','THIRD-PARTY-NOTICES.md','assets/languages/en.json','assets/languages/vi.json','target/release/PhotoReviewRs.exe')) {
        Write-Utf8 (Join-Path $fixture $file) "fixture $file"
    }
    $size = (Get-Item -LiteralPath (Join-Path $fixture 'target/release/PhotoReviewRs.exe')).Length
    Write-Utf8 (Join-Path $fixture 'native/release-size.json') (@{ min_bytes=$size; max_bytes=$size; measured_commit='fixture' } | ConvertTo-Json)
    $reviewPath = Join-Path $fixture 'native/release-review.json'
    $review = @{ native_notices_reviewed=$true; libraw_source_offer_reviewed=$true; application_license_reviewed=$true; languages_runtime_validated=$true }
    Write-Utf8 $reviewPath ($review | ConvertTo-Json)
    $stage = Join-Path $fixture 'release-stage'
    & (Join-Path $tools 'stage-release.ps1') -Version '1.2.3'
    Assert (Test-Path -LiteralPath (Join-Path $stage 'Languages/vi.json')) 'stage preserves language directory'
    $verify = Join-Path $tools 'verify-release-native.ps1'
    $review.native_notices_reviewed=$false
    Write-Utf8 $reviewPath ($review | ConvertTo-Json)
    Reject { & $verify -ReleaseDirectory $stage -Version '1.2.3' } 'review not completed' 'unreviewed notices block release'
    $review.native_notices_reviewed=$true
    Write-Utf8 $reviewPath ($review | ConvertTo-Json)
    foreach ($forbidden in @('PhotoReview.exe','coreclr.dll','nested/PresentationFramework.dll','extra.txt')) {
        $path = Join-Path $stage $forbidden
        Write-Utf8 $path 'forbidden'
        Reject { & $verify -ReleaseDirectory $stage -Version '1.2.3' } 'Forbidden' "reject $forbidden"
        Remove-Item -LiteralPath $path
        if ($forbidden.Contains('/')) { Remove-Item -LiteralPath (Split-Path $path) }
    }
    $raw = Join-Path $stage 'libraw.dll'
    Write-Utf8 $raw 'corrupt'
    Reject { & $verify -ReleaseDirectory $stage -Version '1.2.3' } 'SHA-256 mismatch' 'staged DLL mismatch'
    Copy-Item -LiteralPath (Join-Path $fixture 'native/x64/libraw.dll') -Destination $raw -Force
    $lang = Join-Path $stage 'Languages/vi.json'
    Remove-Item -LiteralPath $lang
    Reject { & $verify -ReleaseDirectory $stage -Version '1.2.3' } 'Missing/empty' 'missing Vietnamese catalog is fatal'
    Copy-Item -LiteralPath (Join-Path $fixture 'assets/languages/vi.json') -Destination $lang
    & (Join-Path $tools 'pack-release.ps1') -Tag 'v1.2.3' | Out-Null
    $sums = Get-Content -LiteralPath (Join-Path $fixture 'SHA256SUMS')
    Assert ($sums.Count -eq 5 -and $sums[-1] -match 'PhotoReviewRs-v1.2.3-win-x64.zip$') 'zip contains exact layout and sums cover exe, DLLs, source and zip'
    Write-Utf8 (Join-Path $fixture 'native/release-size.json') '{"min_bytes":null,"max_bytes":null,"measured_commit":null}'
    Reject { & $verify -ReleaseDirectory $stage -Version '1.2.3' } 'measured Rust release' 'unset size bounds fail closed'
    # Exercise the exact inline workflow preflight with fake gh; no GitHub request occurs.
    $workflow = [IO.File]::ReadAllText((Join-Path $repo '.github/workflows/release.yml'))
    $start = $workflow.IndexOf('          $ErrorActionPreference')
    $end = $workflow.IndexOf('      - name: Checkout tagged commit', $start)
    $preflight = [scriptblock]::Create(($workflow.Substring($start,$end-$start) -replace '(?m)^          ',''))
    function gh {
        $global:LASTEXITCODE = 0
        $call = $args -join ' '
        if ($script:apiFails) { $global:LASTEXITCODE = 1; return }
        if ($call -match '/git/ref/tags/') { if (-not $script:missingTag) { return 'refs/tags/v1.2.3' }; return }
        if ($call -match '/check-runs\?') { return $script:runs }
        if ($call -match '/commits/') { return ('a' * 40) }
        if ($call -match '/releases\?') { return $script:releaseTags }
        throw "Unexpected gh call: $call"
    }
    $env:TAG='v1.2.3'; $env:RUN_SMOKE='true'; $env:SMOKE_RESULT=''
    $env:GITHUB_REPOSITORY='fixture/repository'; $env:GITHUB_OUTPUT=Join-Path $scratch 'output'
    $script:apiFails=$false; $script:missingTag=$false; $script:releaseTags=@(); $script:runs=@("10`tbuild-test`tcompleted`tsuccess")
    & $preflight
    Assert ((Get-Content $env:GITHUB_OUTPUT -Raw) -match ('sha=' + ('a'*40))) 'preflight accepts existing tag with green peeled commit'
    $script:runs += "11`tbuild-test`tcompleted`tfailure"
    Reject { & $preflight } 'not green' 'latest check failure overrides old green'
    $script:runs=@()
    Reject { & $preflight } 'not green' 'missing check is fatal'
    $script:runs=@("10`tbuild-test`tin_progress`tsuccess")
    Reject { & $preflight } 'not green' 'incomplete check is fatal'
    $script:runs=@("10`tbuild-test`tcompleted`tsuccess"); $script:releaseTags=@('v1.2.3')
    Reject { & $preflight } 'already exists' 'existing release cannot be overwritten'
    $script:releaseTags=@(); $script:apiFails=$true
    Reject { & $preflight } 'does not exist' 'API failure fails closed'
    $script:apiFails=$false; $script:missingTag=$true
    Reject { & $preflight } 'does not exist' 'missing tag fails closed'
    $script:missingTag=$false; $env:RUN_SMOKE='false'
    Reject { & $preflight } 'maintainer smoke result' 'disabled runner smoke requires evidence'
    $env:SMOKE_RESULT='fixture maintainer smoke exited 0'
    & $preflight
    Assert $true 'manual smoke evidence path accepted'
    Write-Output "release selftest PASSED ($checks checks; fixture packaging substitutes only PE metadata)"
} finally { Remove-ReleaseScratch $scratch ([IO.Path]::GetTempPath()) }
