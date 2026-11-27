# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Opens the app and checks that it shows a window and stays up for five seconds after that.

.DESCRIPTION
Starts nvhvc.exe with `gui`, waits up to two minutes for its window, and stops it afterwards.
Right after Setup installs the Windows App Runtime, the first start has taken almost 30 seconds on
the runners, with the app idle and waiting on Windows.
#>
param(
    [Parameter(Mandatory)] [string] $Exe
)
$ErrorActionPreference = 'Stop'
$limit = 120

# What the exe writes to stderr, such as the error that ended it.
$stderr = [IO.Path]::GetTempFileName()
$process = Start-Process -FilePath $Exe -ArgumentList gui -PassThru -RedirectStandardError $stderr
# Keeps the exit code readable once the process has ended.
$null = $process.Handle
$clock = [Diagnostics.Stopwatch]::StartNew()
try {
    do {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
    } until ($process.HasExited -or $process.MainWindowHandle -ne 0 -or $clock.Elapsed.TotalSeconds -ge $limit)
    $shown = [math]::Round($clock.Elapsed.TotalSeconds, 1)
    if (-not $process.HasExited) {
        if ($process.MainWindowHandle -eq 0) { throw "nvhvc is running but has no window after $limit seconds" }
        Start-Sleep -Seconds 5
        $process.Refresh()
    }
    if ($process.HasExited) { throw "nvhvc exited with code $($process.ExitCode)" }
    "nvhvc is up: $($process.MainWindowTitle), window after $shown s"
} catch {
    Get-Content -LiteralPath $stderr | Out-Host
    throw
} finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id }
}
