# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Fails if an exe needs the Visual C++ runtime DLLs, which Windows doesn't ship.

.DESCRIPTION
Reads each exe's imports, delay-loaded ones included, with dumpbin from the Visual Studio
install. Check the exes before UPX packs them, since packing hides their imports.
#>
param(
    [Parameter(Mandatory)] [string[]] $Exe
)
$ErrorActionPreference = 'Stop'

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$dumpbin = Get-ChildItem "$vs\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" | Select-Object -Last 1
if (-not $dumpbin) {
    throw "dumpbin.exe not found in '$vs'"
}
foreach ($file in $Exe) {
    $out = & $dumpbin.FullName /nologo /dependents $file
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin failed on $file"
    }
    $crt = $out | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^(vcruntime|msvcp|concrt|vccorlib)\S*\.dll$' }
    if ($crt) {
        throw "$file needs $($crt -join ', '), which Windows doesn't ship"
    }
    "$file doesn't need the Visual C++ runtime"
}
