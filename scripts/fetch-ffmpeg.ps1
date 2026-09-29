# Download Windows ffmpeg essentials and place it for Tauri externalBin packaging.
# Output: src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $root "src-tauri\binaries"
$targetName = "ffmpeg-x86_64-pc-windows-msvc.exe"
$targetPath = Join-Path $binDir $targetName

New-Item -ItemType Directory -Force -Path $binDir | Out-Null

if (Test-Path $targetPath) {
  Write-Host "Already exists: $targetPath"
  exit 0
}

$tmp = Join-Path $env:TEMP ("hyp-ffmpeg-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
  $zipPath = Join-Path $tmp "ffmpeg-essentials.zip"
  $url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
  Write-Host "Downloading $url ..."
  Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing

  Write-Host "Extracting..."
  Expand-Archive -Path $zipPath -DestinationPath $tmp -Force

  $ffmpeg = Get-ChildItem -Path $tmp -Recurse -Filter "ffmpeg.exe" |
    Where-Object { $_.FullName -match '\\bin\\ffmpeg\.exe$' } |
    Select-Object -First 1

  if (-not $ffmpeg) {
    throw "ffmpeg.exe not found inside the downloaded archive"
  }

  Copy-Item -Force $ffmpeg.FullName $targetPath
  # Convenient local/dev name
  Copy-Item -Force $ffmpeg.FullName (Join-Path $binDir "ffmpeg.exe")

  Write-Host "OK: $targetPath"
  Write-Host "OK: $(Join-Path $binDir 'ffmpeg.exe')"
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
