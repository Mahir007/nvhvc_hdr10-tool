# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
Opens the app and checks that it shows a window and stays up for five seconds.

.DESCRIPTION
Give -Exe for nvhvc.exe, which is started with `gui`, or -App with a packaged app's ID (family
name!App). Stops the app afterwards.
#>
param(
    [string] $Exe,
    [string] $App
)
$ErrorActionPreference = 'Stop'

# What the portable exe writes to stderr, such as the error that ended it.
$stderr = [IO.Path]::GetTempFileName()
if ($Exe) {
    $process = Start-Process -FilePath $Exe -ArgumentList gui -PassThru -RedirectStandardError $stderr
} else {
    Start-Process "shell:AppsFolder\$App"
    $process = $null
    foreach ($attempt in 1..20) {
        Start-Sleep -Milliseconds 250
        $process = Get-Process nvhvc -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($process) { break }
    }
    if (-not $process) {
        throw "$App didn't start"
    }
}
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
