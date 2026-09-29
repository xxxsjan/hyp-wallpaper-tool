#!/usr/bin/env node
/**
 * Build a portable zip: app exe + bundled ffmpeg (no installer).
 * Output: release/portable/HYP-Wallpaper-Tool-portable-vX.Y.Z.zip
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
process.chdir(root);

const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
const version = pkg.version;
const product = "HYP-Wallpaper-Tool";
const outName = `${product}-portable-v${version}`;
const releaseDir = path.join(root, "src-tauri", "target", "release");
const staging = path.join(root, "release", "portable", outName);
const zipPath = path.join(root, "release", "portable", `${outName}.zip`);
const binDir = path.join(root, "src-tauri", "binaries");

function run(cmd, opts = {}) {
  console.log(`$ ${cmd}`);
  execSync(cmd, { stdio: "inherit", shell: true, ...opts });
}

function exists(p) {
  try {
    fs.accessSync(p);
    return true;
  } catch {
    return false;
  }
}

console.log("==> Ensure ffmpeg");
run("npm run fetch-ffmpeg");

let ffmpegSrc = path.join(binDir, "ffmpeg.exe");
if (!exists(ffmpegSrc)) {
  ffmpegSrc = path.join(binDir, "ffmpeg-x86_64-pc-windows-msvc.exe");
}
if (!exists(ffmpegSrc)) {
  throw new Error("ffmpeg binary missing under src-tauri/binaries");
}

console.log("==> tauri build (no installer bundle)");
run("npx tauri build --no-bundle");

const exeCandidates = [
  path.join(releaseDir, "hyp-wallpaper-tool.exe"),
  path.join(releaseDir, "HYP Wallpaper Tool.exe"),
].filter(exists);

if (exeCandidates.length === 0) {
  throw new Error(`Release exe not found in ${releaseDir}`);
}
const exeSrc = exeCandidates[0];

console.log("==> Stage portable folder");
fs.rmSync(staging, { recursive: true, force: true });
fs.mkdirSync(staging, { recursive: true });

fs.copyFileSync(exeSrc, path.join(staging, "HYP Wallpaper Tool.exe"));
fs.copyFileSync(ffmpegSrc, path.join(staging, "ffmpeg.exe"));

const readme = `HYP Wallpaper Tool portable v${version}

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
`;
// UTF-8 with BOM for Chinese Windows Notepad
fs.writeFileSync(
  path.join(staging, "README.txt"),
  "\uFEFF" + readme,
  "utf8",
);

console.log("==> Zip");
fs.mkdirSync(path.dirname(zipPath), { recursive: true });
fs.rmSync(zipPath, { force: true });

const portableDir = path.dirname(staging);
try {
  // Windows 10+ tar can create .zip with -a
  run(`tar -a -c -f "${zipPath}" -C "${portableDir}" "${outName}"`);
} catch {
  // Fallback: PowerShell Compress-Archive
  run(
    `powershell -NoProfile -Command "Compress-Archive -Path '${staging}' -DestinationPath '${zipPath}' -Force"`,
  );
}

console.log("");
console.log(`OK: ${zipPath}`);
console.log(`OK: ${staging}`);
for (const name of fs.readdirSync(staging)) {
  const full = path.join(staging, name);
  const size = fs.statSync(full).size;
  console.log(`  ${name.padEnd(28)} ${size}`);
}
