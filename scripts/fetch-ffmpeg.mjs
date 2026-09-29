#!/usr/bin/env node
/**
 * Download Windows ffmpeg essentials for Tauri externalBin packaging.
 * Output: src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { createWriteStream } from "node:fs";
import { fileURLToPath } from "node:url";
import { Readable } from "node:stream";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const binDir = path.join(root, "src-tauri", "binaries");
const targetName = "ffmpeg-x86_64-pc-windows-msvc.exe";
const targetPath = path.join(binDir, targetName);
const url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

fs.mkdirSync(binDir, { recursive: true });

if (fs.existsSync(targetPath)) {
  console.log(`Already exists: ${targetPath}`);
  process.exit(0);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "hyp-ffmpeg-"));
const zipPath = path.join(tmp, "ffmpeg-essentials.zip");

async function download(from, to) {
  console.log(`Downloading ${from} ...`);
  const res = await fetch(from, { redirect: "follow" });
  if (!res.ok) {
    throw new Error(`Download failed: ${res.status} ${res.statusText}`);
  }
  await pipeline(Readable.fromWeb(res.body), createWriteStream(to));
}

function findFfmpegExe(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const ent of entries) {
    const full = path.join(dir, ent.name);
    if (ent.isDirectory()) {
      const found = findFfmpegExe(full);
      if (found) return found;
      continue;
    }
    if (
      ent.name.toLowerCase() === "ffmpeg.exe" &&
      /[/\\]bin[/\\]ffmpeg\.exe$/i.test(full)
    ) {
      return full;
    }
  }
  return null;
}

try {
  await download(url, zipPath);

  console.log("Extracting...");
  try {
    execSync(`tar -xf "${zipPath}" -C "${tmp}"`, { stdio: "inherit" });
  } catch {
    execSync(
      `powershell -NoProfile -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${tmp}' -Force"`,
      { stdio: "inherit" },
    );
  }

  const ffmpeg = findFfmpegExe(tmp);
  if (!ffmpeg) {
    throw new Error("ffmpeg.exe not found inside the downloaded archive");
  }

  fs.copyFileSync(ffmpeg, targetPath);
  fs.copyFileSync(ffmpeg, path.join(binDir, "ffmpeg.exe"));

  console.log(`OK: ${targetPath}`);
  console.log(`OK: ${path.join(binDir, "ffmpeg.exe")}`);
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
