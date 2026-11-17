<#
  Silent install → verify files/shortcut → launch check → silent uninstall → verify
  user data (databases/backups) is preserved.
#>
param([Parameter(Mandatory)][string]$Installer)
$ErrorActionPreference = "Stop"
$installer = (Resolve-Path $Installer).Path
$dir = Join-Path $env:LOCALAPPDATA "TOP Server Manager"
$dataDir = Join-Path $env:LOCALAPPDATA "TOP Server Manager Data"
$marker = Join-Path $dataDir "databases\smoke-test-marker.txt"

function Check($ok, $what) { if ($ok) { Write-Host "PASS  $what" } else { Write-Host "FAIL  $what"; $script:failed = $true } }
$failed = $false

New-Item -ItemType Directory -Force (Split-Path $marker) | Out-Null
Set-Content $marker "user data must survive uninstall"

$p = Start-Process $installer -ArgumentList "/S" -Wait -PassThru
Check ($p.ExitCode -eq 0) "silent install exit code 0"
$exe = Get-ChildItem $dir -Filter "*.exe" -ErrorAction SilentlyContinue | Where-Object { $_.Name -notlike "uninstall*" } | Select-Object -First 1
Check ($null -ne $exe) "application installed in $dir"
$uninst = Join-Path $dir "uninstall.exe"
Check (Test-Path $uninst) "uninstaller present"
$lnk = Get-ChildItem "$env:APPDATA\Microsoft\Windows\Start Menu\Programs" -Recurse -Filter "TOP Server Manager*.lnk" -ErrorAction SilentlyContinue
Check ($lnk.Count -gt 0) "Start Menu shortcut created"
$reg = Get-ChildItem "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall" | Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -like "TOP Server Manager*" }
Check ($null -ne $reg) "registered in Apps & features (per-user)"

if ($exe) {
  $app = Start-Process $exe.FullName -PassThru
  Start-Sleep -Seconds 6
  Check (-not $app.HasExited) "application starts and keeps running"
  if (-not $app.HasExited) { Stop-Process -Id $app.Id -Force }
}

# Upgrade / reinstall over the existing install
$p = Start-Process $installer -ArgumentList "/S" -Wait -PassThru
Check ($p.ExitCode -eq 0) "reinstall/upgrade over existing install"

$p = Start-Process $uninst -ArgumentList "/S" -Wait -PassThru
# the NSIS uninstaller re-launches itself from %TEMP%, so -Wait can return before files are gone
$gone = $false
foreach ($i in 1..30) { if (-not (Test-Path (Join-Path $dir $exe.Name))) { $gone = $true; break }; Start-Sleep -Seconds 1 }
if (-not $gone) {
  $busy = Get-Process | Where-Object { $_.Path -like "$dir*" } | ForEach-Object { "$($_.ProcessName)($($_.Id))" }
  Write-Host "      still in use by: $($busy -join ', ')"
}
Check $gone "application removed by uninstaller"
Check (Test-Path $marker) "user databases/backups preserved after uninstall"
Remove-Item $marker -Force

if ($failed) { exit 1 } else { Write-Host "INSTALLER SMOKE TEST: ALL CHECKS PASSED" }
