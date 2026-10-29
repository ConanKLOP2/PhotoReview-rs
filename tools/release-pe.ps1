# Parse the PE resource tree as data; never load or execute the file being inspected.
function Get-ReleasePeResources([byte[]]$Bytes) {
    function U16([int]$At) { if ($At -lt 0 -or $At + 2 -gt $Bytes.Length) { throw 'Truncated PE word' }; [BitConverter]::ToUInt16($Bytes,$At) }
    function U32([int]$At) { if ($At -lt 0 -or [long]$At + 4 -gt $Bytes.Length) { throw 'Truncated PE dword' }; [BitConverter]::ToUInt32($Bytes,$At) }
    if ((U16 0) -ne 0x5a4d) { throw 'Not an MZ executable' }
    $pe = [int](U32 60)
    if ((U32 $pe) -ne 0x4550 -or (U16 ($pe+4)) -ne 0x8664) { throw 'Not an AMD64 PE' }
    $optional = $pe + 24
    if ((U16 $optional) -ne 0x20b) { throw 'Not PE32+' }
    $rva = U32 ($optional+128)
    $section = $optional + (U16 ($pe+20))
    $resourceBase = -1
    for ($i=0; $i -lt (U16 ($pe+6)); $i++) {
        $s = $section + 40*$i
        $va = U32 ($s+12); $size = U32 ($s+16)
        if ($rva -ge $va -and $rva -lt [long]$va+$size) { $resourceBase = [int]((U32 ($s+20)) + $rva - $va) }
    }
    if ($resourceBase -lt 0) { throw 'Missing PE resources' }
    $types = @()
    $count = (U16 ($resourceBase+12)) + (U16 ($resourceBase+14))
    for ($i=0; $i -lt $count; $i++) { $types += U32 ($resourceBase+16+8*$i) }
    return ,$types
}
function Assert-ReleasePe([string]$Path, [string]$Version) {
    $types = Get-ReleasePeResources ([IO.File]::ReadAllBytes($Path))
    foreach ($type in @(14,16,24)) { if ($types -notcontains $type) { throw "Missing PE resource type $type" } }
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo([IO.Path]::GetFullPath($Path))
    if ($info.ProductVersion -cne $Version -or $info.FileVersion -cne $Version -or $info.OriginalFilename -cne 'PhotoReviewRs.exe' -or $info.FileMajorPart -ne [int]$Version.Split('.')[0] -or $info.FileMinorPart -ne [int]$Version.Split('.')[1] -or $info.FileBuildPart -ne [int]$Version.Split('.')[2] -or $info.FilePrivatePart -ne 0) { throw 'PE fixed/string version or original filename differs from release tag' }
}
