#!/usr/bin/env node
/**
 * Build a portable zip: app exe + bundled ffmpeg (no installer).
 * Optionally bump patch version (last number +1), sync project files, then git commit.
 * Output: release/portable/<product>-portable-vX.Y.Z.zip
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline/promises";
import { stdin as input, stdout as output } from "node:process";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
process.chdir(root);

const product = "Wallpaper";
const binDir = path.join(root, "src-tauri", "binaries");
const releaseDir = path.join(root, "src-tauri", "target", "release");

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

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, data) {
  fs.writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function bumpPatch(version) {
  const parts = version.split(".");
  if (parts.length === 0 || parts.some((p) => !/^\d+$/.test(p))) {
    throw new Error(`无效版本号: ${version}`);
  }
  const nums = parts.map((p) => Number(p));
  nums[nums.length - 1] += 1;
  return nums.join(".");
}

async function askBumpVersion(current) {
  if (!input.isTTY) {
    console.log(`非交互环境：跳过升版（当前 ${current}）`);
    return false;
  }
  const rl = readline.createInterface({ input, output });
  try {
    const answer = await rl.question(
      `当前版本 ${current}，是否升级版本号？[y/N] `,
    );
    const a = answer.trim().toLowerCase();
    return a === "y" || a === "yes";
  } finally {
    rl.close();
  }
}

function updateProjectVersions(oldVersion, newVersion) {
  const packageJsonPath = path.join(root, "package.json");
  const packageLockPath = path.join(root, "package-lock.json");
  const tauriConfPath = path.join(root, "src-tauri", "tauri.conf.json");
  const cargoTomlPath = path.join(root, "src-tauri", "Cargo.toml");
  const cargoLockPath = path.join(root, "src-tauri", "Cargo.lock");

  const pkg = readJson(packageJsonPath);
  pkg.version = newVersion;
  writeJson(packageJsonPath, pkg);

  if (exists(packageLockPath)) {
    const lock = readJson(packageLockPath);
    lock.version = newVersion;
    if (lock.packages?.[""]) {
      lock.packages[""].version = newVersion;
    }
    writeJson(packageLockPath, lock);
  }

  const tauriConf = readJson(tauriConfPath);
  tauriConf.version = newVersion;
  writeJson(tauriConfPath, tauriConf);

  let cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
  cargoToml = cargoToml.replace(
    /^version\s*=\s*"[^"]+"/m,
    `version = "${newVersion}"`,
  );
  fs.writeFileSync(cargoTomlPath, cargoToml, "utf8");

  if (exists(cargoLockPath)) {
    let cargoLock = fs.readFileSync(cargoLockPath, "utf8");
    cargoLock = cargoLock.replace(
      /(\[\[package\]\]\r?\nname = "hyp-wallpaper-tool"\r?\n)version = "[^"]+"/,
      `$1version = "${newVersion}"`,
    );
    fs.writeFileSync(cargoLockPath, cargoLock, "utf8");
  }

  console.log(`版本已更新: ${oldVersion} → ${newVersion}`);
  return [
    packageJsonPath,
    packageLockPath,
    tauriConfPath,
    cargoTomlPath,
    cargoLockPath,
  ].filter(exists);
}

function gitCommitVersion(files, newVersion) {
  for (const file of files) {
    run(`git add -- "${file}"`);
  }
  // Avoid -i / amend; simple commit for version bump only
  run(`git commit -m "chore: bump version to ${newVersion}"`);
  console.log(`已提交版本号变更: chore: bump version to ${newVersion}`);
}

async function main() {
  const pkg = readJson(path.join(root, "package.json"));
  let version = pkg.version;

  const shouldBump = await askBumpVersion(version);
  if (shouldBump) {
    const oldVersion = version;
    version = bumpPatch(oldVersion);
    const files = updateProjectVersions(oldVersion, version);
    gitCommitVersion(files, version);
  } else {
    console.log(`保持版本 ${version}`);
  }

  const outName = `${product}-portable-v${version}`;
  const staging = path.join(root, "release", "portable", outName);
  const zipPath = path.join(root, "release", "portable", `${outName}.zip`);

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

  fs.copyFileSync(exeSrc, path.join(staging, "Wallpaper.exe"));
  fs.copyFileSync(ffmpegSrc, path.join(staging, "ffmpeg.exe"));

  console.log("==> Zip");
  fs.mkdirSync(path.dirname(zipPath), { recursive: true });
  fs.rmSync(zipPath, { force: true });

  const portableDir = path.dirname(staging);
  try {
    run(`tar -a -c -f "${zipPath}" -C "${portableDir}" "${outName}"`);
  } catch {
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
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
