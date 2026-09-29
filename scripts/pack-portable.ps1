# Build a portable zip: app exe + bundled ffmpeg, no installer.
# Output: release/portable/HYP-Wallpaper-Tool-portable-vX.Y.Z.zip

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$pkg = Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json
$version = $pkg.version
$product = "HYP-Wallpaper-Tool"
$outName = "$product-portable-v$version"
$releaseDir = Join-Path $root "src-tauri\target\release"
$staging = Join-Path $root "release\portable\$outName"
$zipPath = Join-Path $root "release\portable\$outName.zip"
$binDir = Join-Path $root "src-tauri\binaries"

Write-Host "==> Ensure ffmpeg"
& npm run fetch-ffmpeg
if ($LASTEXITCODE -ne 0) { throw "fetch-ffmpeg failed" }

$ffmpegSrc = Join-Path $binDir "ffmpeg.exe"
if (-not (Test-Path $ffmpegSrc)) {
  $ffmpegSrc = Join-Path $binDir "ffmpeg-x86_64-pc-windows-msvc.exe"
}
if (-not (Test-Path $ffmpegSrc)) {
  throw "ffmpeg binary missing under src-tauri/binaries"
}

Write-Host "==> tauri build (no installer bundle)"
& npx tauri build --no-bundle
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

$exeCandidates = @(
  (Join-Path $releaseDir "hyp-wallpaper-tool.exe"),
  (Join-Path $releaseDir "HYP Wallpaper Tool.exe")
) | Where-Object { Test-Path $_ }

# Force array: a single match would otherwise become a string, and [0] is the first char.
$exeCandidates = @($exeCandidates)
if ($exeCandidates.Count -eq 0) {
  throw "Release exe not found in $releaseDir"
}
$exeSrc = $exeCandidates[0]

Write-Host "==> Stage portable folder"
if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
New-Item -ItemType Directory -Force -Path $staging | Out-Null

Copy-Item -Force $exeSrc (Join-Path $staging "HYP Wallpaper Tool.exe")
Copy-Item -Force $ffmpegSrc (Join-Path $staging "ffmpeg.exe")

$readmePath = Join-Path $staging "README.txt"
$readme = @"
HYP Wallpaper Tool portable v$version

Usage:
1. Unzip to any folder
2. Double-click "HYP Wallpaper Tool.exe"

Notes:
- No installer required
- ffmpeg is bundled for MP4 export
- Requires Windows 10/11 with WebView2 (usually preinstalled)

使用方法：
1. 解压到任意目录
2. 双击「HYP Wallpaper Tool.exe」

说明：
- 无需安装，可直接运行
- 已内置 ffmpeg，支持导出 MP4
- 需要 Windows 10/11，并已安装 WebView2（系统一般自带）
"@
# UTF-8 with BOM so Chinese Windows Notepad displays correctly
$utf8Bom = New-Object System.Text.UTF8Encoding $true
[System.IO.File]::WriteAllText($readmePath, $readme, $utf8Bom)

Write-Host "==> Zip"
New-Item -ItemType Directory -Force -Path (Split-Path $zipPath) | Out-Null
if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
Compress-Archive -Path $staging -DestinationPath $zipPath -Force

Write-Host ""
Write-Host "OK: $zipPath"
Write-Host "OK: $staging"
Get-ChildItem $staging | Format-Table Name, Length -AutoSize
