<#
  Builds TOP Server Manager and assembles dist/:
    TOP-Server-Manager-Setup.exe, TOP-Server-Manager-Source.zip, README_TH.md, README_EN.md, TEST_REPORT.md
  Usage:  ./scripts/build-dist.ps1            (build + package)
          ./scripts/build-dist.ps1 -SkipBuild (package an existing build)
#>
param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

if (-not $SkipBuild) {
  npm ci
  npx tauri build
  if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
}

$nsis = Get-ChildItem "$root\target\release\bundle\nsis\*-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $nsis) { throw "NSIS installer not found under target\release\bundle\nsis" }

New-Item -ItemType Directory -Force "$root\dist" | Out-Null
$target = "$root\dist\TOP-Server-Manager-Setup.exe"
if (-not (Test-Path $target) -or (Get-FileHash $target).Hash -ne (Get-FileHash $nsis.FullName).Hash) {
  Copy-Item $nsis.FullName $target -Force
}
foreach ($f in "README_TH.md", "README_EN.md", "TEST_REPORT.md", "SECURITY_REVIEW.md") {
  if (Test-Path "$root\docs\$f") { Copy-Item "$root\docs\$f" "$root\dist\$f" -Force }
}
foreach ($f in "LICENSE", "THIRD_PARTY_NOTICES.md", "CHANGELOG.md") {
  if (Test-Path "$root\$f") { Copy-Item "$root\$f" "$root\dist\$f" -Force }
}

# Source archive (tracked sources only – no build output, node_modules or local test data)
$zip = "$root\dist\TOP-Server-Manager-Source.zip"
if (Test-Path $zip) { Remove-Item $zip }
$include = @("README.md", "LICENSE", "THIRD_PARTY_NOTICES.md", "CHANGELOG.md", "SECURITY.md",
             "Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "tsconfig.json", "vite.config.ts", "index.html", ".gitignore",
             "crates", "src", "src-tauri", "scripts", "docs", "fixtures", ".github")
$staging = Join-Path $env:TEMP "tsm-src-$(Get-Random)"
New-Item -ItemType Directory $staging | Out-Null
foreach ($i in $include) {
  if (Test-Path "$root\$i") { Copy-Item "$root\$i" "$staging\$i" -Recurse -Force }
}
Remove-Item -Recurse -Force "$staging\src-tauri\target", "$staging\src-tauri\gen" -ErrorAction SilentlyContinue
Compress-Archive -Path "$staging\*" -DestinationPath $zip
Remove-Item -Recurse -Force $staging

$hash = (Get-FileHash "$root\dist\TOP-Server-Manager-Setup.exe" -Algorithm SHA256).Hash
Set-Content -Encoding ascii "$root\dist\SHA256SUMS.txt" "$hash  TOP-Server-Manager-Setup.exe"
Get-ChildItem "$root\dist" | Format-Table Name, Length
