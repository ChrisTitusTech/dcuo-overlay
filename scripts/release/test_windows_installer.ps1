param([switch]$SoftwareOpenGl)
$ErrorActionPreference = 'Stop'
$installer = (Get-Item 'dist/*-setup.exe').FullName
$install = Join-Path $env:RUNNER_TEMP 'DCUO Installer Smoke'
$p = Start-Process $installer -ArgumentList @('/S', "/D=$install") -PassThru -Wait
if ($p.ExitCode -ne 0) { throw "Installer failed: $($p.ExitCode)" }
$binary = Join-Path $install 'dcuo-companion.exe'
if (!(Test-Path $binary)) { throw 'Installed executable missing' }
$expected = (Get-FileHash 'target/release/dcuo-companion.exe' -Algorithm SHA256).Hash
if ((Get-FileHash $binary -Algorithm SHA256).Hash -ne $expected) { throw 'Installed bytes differ' }
# Hosted Windows runners have no modern hardware OpenGL driver. Supply a pinned
# application-local Mesa driver only for this test, never to the release package.
$testDrivers = @()
if ($SoftwareOpenGl) {
    $archive = Join-Path $env:RUNNER_TEMP 'mesa3d-26.2.4-release-msvc.7z'
    $mesa = Join-Path $env:RUNNER_TEMP 'dcuo-test-mesa'
    Invoke-WebRequest 'https://github.com/pal1000/mesa-dist-win/releases/download/26.2.4/mesa3d-26.2.4-release-msvc.7z' -OutFile $archive
    if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne '351fc8c8b695878ffb3eaa044b3ead08672a48b1a045e3c3e3975811df0f6695') {
        throw 'Test graphics driver checksum mismatch'
    }
    & 7z x $archive "-o$mesa" -y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Test graphics driver extraction failed' }
    $testDrivers = @(Get-ChildItem (Join-Path $mesa 'x64/*.dll'))
    if (!($testDrivers.Name -contains 'opengl32.dll')) { throw 'Test OpenGL driver missing' }
    $testDrivers | Copy-Item -Destination $install
    $env:GALLIUM_DRIVER = 'llvmpipe'
}
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
foreach ($driver in $testDrivers) { Remove-Item (Join-Path $install $driver.Name) }
$uninstall = Start-Process (Join-Path $install 'Uninstall.exe') -ArgumentList '/S' -PassThru -Wait
if ($uninstall.ExitCode -ne 0) { throw 'Uninstaller failed' }
$deadline = (Get-Date).AddSeconds(30)
while ((Test-Path $binary) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
if (Test-Path $binary) { throw 'Uninstaller left executable behind' }
Write-Output 'Per-user install, installed binary and uninstall passed.'
