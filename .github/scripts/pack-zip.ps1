# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Zips a folder so the same files give the same archive.

.DESCRIPTION
Entries are added in ordinal order of their paths, with forward slashes, and every entry's time
is Epoch (the commit's time, in seconds since 1970).
#>
param(
    [Parameter(Mandatory)] [string] $Source,
    [Parameter(Mandatory)] [string] $Out,
    [Parameter(Mandatory)] [long] $Epoch
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression

$source = (Resolve-Path -LiteralPath $Source).Path.TrimEnd('\', '/')
$time = [DateTimeOffset]::FromUnixTimeSeconds($Epoch)
[string[]] $names = Get-ChildItem -LiteralPath $source -Recurse -File |
    ForEach-Object { $_.FullName.Substring($source.Length + 1).Replace('\', '/') }
[Array]::Sort($names, [StringComparer]::Ordinal)

$stream = [IO.File]::Create([IO.Path]::GetFullPath($Out))
$zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($name in $names) {
        $entry = $zip.CreateEntry($name, [IO.Compression.CompressionLevel]::Optimal)
        $entry.LastWriteTime = $time
        $in = [IO.File]::OpenRead((Join-Path $source $name))
        $entryStream = $entry.Open()
        try {
            $in.CopyTo($entryStream)
        } finally {
            $entryStream.Dispose()
            $in.Dispose()
        }
    }
} finally {
    $zip.Dispose()
    $stream.Dispose()
}
