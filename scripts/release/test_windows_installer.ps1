$ErrorActionPreference = 'Stop'
$installer = (Get-Item 'dist/*-setup.exe').FullName
$install = Join-Path $env:RUNNER_TEMP 'DCUO Installer Smoke'
$p = Start-Process $installer -ArgumentList @('/S', "/D=$install") -PassThru -Wait
if ($p.ExitCode -ne 0) { throw "Installer failed: $($p.ExitCode)" }
$binary = Join-Path $install 'dcuo-companion.exe'
if (!(Test-Path $binary)) { throw 'Installed executable missing' }
$expected = (Get-FileHash 'target/release/dcuo-companion.exe' -Algorithm SHA256).Hash
if ((Get-FileHash $binary -Algorithm SHA256).Hash -ne $expected) { throw 'Installed bytes differ' }
# Exercise the actual installed GUI; startup verifies normal native window styles.
$stdout = Join-Path $env:RUNNER_TEMP 'dcuo-window.stdout'
$stderr = Join-Path $env:RUNNER_TEMP 'dcuo-window.stderr'
$gui = Start-Process $binary -ArgumentList @('--smoke-seconds', '3') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
if (!$gui.WaitForExit(30000)) {
    Stop-Process -Id $gui.Id
    throw 'Installed GUI did not exit within the smoke deadline'
}
$gui.Refresh()
if ($gui.ExitCode -ne 0) {
    Get-Content $stderr
    throw "Installed GUI failed: $($gui.ExitCode)"
}
$help = Start-Process $binary -ArgumentList '--help' -PassThru -Wait
if ($help.ExitCode -ne 0) { throw 'Installed executable help failed' }
$uninstall = Start-Process (Join-Path $install 'Uninstall.exe') -ArgumentList '/S' -PassThru -Wait
if ($uninstall.ExitCode -ne 0) { throw 'Uninstaller failed' }
$deadline = (Get-Date).AddSeconds(30)
while ((Test-Path $binary) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
if (Test-Path $binary) { throw 'Uninstaller left executable behind' }
Write-Output 'Per-user install, installed binary and uninstall passed.'
