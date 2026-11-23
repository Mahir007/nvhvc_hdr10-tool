# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Packs nvhvc-winui.exe and nvhvc.exe into an MSIX, signed with the certificate in Pfx if given.

.DESCRIPTION
BinDir must hold an nvhvc-winui.exe built with NVHVC_WINUI_RUNTIME=framework, which uses the
Windows App SDK runtime the package depends on instead of carrying its own. Needs makeappx.exe,
and signtool.exe to sign, from the Windows SDK. Prints the package's four-part version.

With Pfx, the package's publisher is the certificate's subject, the password comes from
MSIX_PFX_PASSWORD, and Certificate is where to write the certificate users import. Without it,
the package is unsigned, and the OID in its publisher lets Windows 11 install it with
Add-AppxPackage -AllowUnsigned.

.EXAMPLE
pwsh packaging/msix/build-msix.ps1 -Version 1.0.0-beta.1+abc1234 -BinDir target/debug -Out dist/nvhvc.msix
#>
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $BinDir,
    [Parameter(Mandatory)] [string] $Out,
    [string] $Pfx,
    [string] $Certificate
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

# The newest SDK's copy of a Windows SDK tool.
function Find-SdkTool([string] $name) {
    $tool = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\$name" |
        Sort-Object { [version]$_.Directory.Parent.Name } |
        Select-Object -Last 1
    if (-not $tool) {
        throw "$name not found; install the Windows SDK"
    }
    $tool.FullName
}

$msixVersion = ConvertTo-MsixVersion $Version
if ($Pfx) {
    # Only the public part is needed here; signtool reads the key itself.
    $cert = [Security.Cryptography.X509Certificates.X509Certificate2]::new(
        [IO.Path]::GetFullPath($Pfx), $env:MSIX_PFX_PASSWORD,
        [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
    $publisher = $cert.Subject
} else {
    $publisher = 'CN=Mahir007, OID.2.25.311729368913984317654407730594956997722=1'
}
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
# A fast build's debug info, so its backtraces have names. A release build has none.
foreach ($pdb in 'nvhvc_winui.pdb', 'nvhvc.pdb') {
    $path = Join-Path $BinDir $pdb
    if (Test-Path -LiteralPath $path) {
        Copy-Item -LiteralPath $path -Destination $layout
    }
}
Copy-Item -Path (Join-Path $here 'Assets\*.png') -Destination (Join-Path $layout 'Assets')
$manifest = Get-Content -Raw -LiteralPath (Join-Path $here 'AppxManifest.xml.in')
$manifest = $manifest.Replace('@VERSION@', $msixVersion).Replace('@PUBLISHER@', $publisher)
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest)

& (Find-SdkTool makeappx.exe) pack /d $layout /p $out /o | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "makeappx failed with exit code $LASTEXITCODE"
}
Remove-Item -LiteralPath $layout -Recurse -Force

if ($Pfx) {
    # The timestamp keeps the signature valid after the certificate expires.
    & (Find-SdkTool signtool.exe) sign /q /fd SHA256 /f $Pfx /p $env:MSIX_PFX_PASSWORD `
        /tr http://timestamp.digicert.com /td SHA256 $out | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "signtool failed with exit code $LASTEXITCODE"
    }
    if ($Certificate) {
        [IO.File]::WriteAllBytes([IO.Path]::GetFullPath($Certificate), $cert.Export('Cert'))
    }
}
$msixVersion
