# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Packs nvhvc-winui.exe and nvhvc.exe into an unsigned MSIX.

.DESCRIPTION
BinDir must hold an nvhvc-winui.exe built with NVHVC_WINUI_RUNTIME=framework, which uses the
Windows App SDK runtime the package depends on instead of carrying its own. Needs makeappx.exe
from the Windows SDK. Prints the package's four-part version.

.EXAMPLE
pwsh packaging/msix/build-msix.ps1 -Version 0.1.0-beta.1+abc1234 -BinDir target/msix/release -Out dist/nvhvc.msix
#>
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $BinDir,
    [Parameter(Mandatory)] [string] $Out
)
$ErrorActionPreference = 'Stop'

# MSIX versions are four numbers. The fourth orders the builds of one version, so an upgrade from
# a pre-release to the release works: alpha.N is N, beta.N is 100 + N, rc.N is 200 + N and the
# release is 300. A dev build is 0.0.0.0.
function ConvertTo-MsixVersion([string] $semver) {
    $core = $semver -replace '\+.*$', ''
    if ($core -eq '0.0.0-dev') {
        return '0.0.0.0'
    }
    if ($core -match '^(\d+)\.(\d+)\.(\d+)$') {
        return "$($Matches[1]).$($Matches[2]).$($Matches[3]).300"
    }
    if ($core -match '^(\d+)\.(\d+)\.(\d+)-(alpha|beta|rc)\.([1-9][0-9]?)$') {
        $stage = @{ alpha = 0; beta = 100; rc = 200 }[$Matches[4]]
        return "$($Matches[1]).$($Matches[2]).$($Matches[3]).$($stage + [int]$Matches[5])"
    }
    throw "$semver has no MSIX version: use X.Y.Z, or X.Y.Z-alpha.N, -beta.N or -rc.N with N below 100"
}

$msixVersion = ConvertTo-MsixVersion $Version
$here = $PSScriptRoot
$out = [IO.Path]::GetFullPath($Out)
$layout = "$out.layout"
if (Test-Path -LiteralPath $layout) {
    Remove-Item -LiteralPath $layout -Recurse -Force
}
New-Item -ItemType Directory -Path (Join-Path $layout 'Assets') | Out-Null

foreach ($exe in 'nvhvc-winui.exe', 'nvhvc.exe') {
    Copy-Item -LiteralPath (Join-Path $BinDir $exe) -Destination $layout
}
Copy-Item -Path (Join-Path $here 'Assets\*.png') -Destination (Join-Path $layout 'Assets')
$manifest = (Get-Content -Raw -LiteralPath (Join-Path $here 'AppxManifest.xml.in')).Replace('@VERSION@', $msixVersion)
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest)

# The newest SDK's copy.
$makeappx = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" |
    Sort-Object { [version]$_.Directory.Parent.Name } |
    Select-Object -Last 1
if (-not $makeappx) {
    throw 'makeappx.exe not found; install the Windows SDK'
}
& $makeappx.FullName pack /d $layout /p $out /o | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "makeappx failed with exit code $LASTEXITCODE"
}
Remove-Item -LiteralPath $layout -Recurse -Force
$msixVersion
