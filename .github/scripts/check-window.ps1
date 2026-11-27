# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Opens the app and checks that it shows a window and stays up for five seconds.

.DESCRIPTION
Starts nvhvc.exe with `gui`, and stops it afterwards.
#>
param(
    [Parameter(Mandatory)] [string] $Exe
)
$ErrorActionPreference = 'Stop'

# What the exe writes to stderr, such as the error that ended it.
$stderr = [IO.Path]::GetTempFileName()
$process = Start-Process -FilePath $Exe -ArgumentList gui -PassThru -RedirectStandardError $stderr
Start-Sleep -Seconds 5
$process.Refresh()
if ($process.HasExited) {
    Get-Content -LiteralPath $stderr | Out-Host
    throw "nvhvc exited with code $($process.ExitCode)"
}
if ($process.MainWindowHandle -eq 0) {
    throw 'nvhvc is running but has no window'
}
"nvhvc is up: $($process.MainWindowTitle)"
Stop-Process -Id $process.Id
