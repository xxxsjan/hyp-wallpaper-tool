#!/usr/bin/env node
/**
 * Create git tag vX.Y.Z from package.json, push it, and upload the local
 * portable zip to a GitHub Release.
 *
 * Expects: release/portable/hyp-wallpaper-tool-portable-vX.Y.Z.zip
 * (produced by `npm run pack:portable`)
 *
 * Usage:
 *   node ./scripts/release-tag.mjs
 *   node ./scripts/release-tag.mjs --notes "修复说明"
 *   node ./scripts/release-tag.mjs --zip path/to/file.zip
 *   node ./scripts/release-tag.mjs --dry-run
 */
import { execSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
process.chdir(root);

const product = "hyp-wallpaper-tool";

function run(cmd, opts = {}) {
  console.log(`$ ${cmd}`);
  execSync(cmd, { stdio: "inherit", shell: true, ...opts });
}

function runCapture(cmd) {
  return execSync(cmd, { encoding: "utf8", shell: true }).trim();
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

function parseArgs(argv) {
  const out = { dryRun: false, notes: "", zip: "", pushBranch: true };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--dry-run") out.dryRun = true;
    else if (a === "--no-push-branch") out.pushBranch = false;
    else if (a === "--notes") out.notes = argv[++i] ?? "";
    else if (a === "--zip") out.zip = argv[++i] ?? "";
    else if (a === "--help" || a === "-h") out.help = true;
    else throw new Error(`未知参数: ${a}`);
  }
  return out;
}

function ensureCleanEnough() {
  const status = runCapture("git status --porcelain");
  if (!status) return;
  console.warn("警告: 工作区有未提交改动，仍会继续打 tag（指向当前 HEAD）:");
  console.warn(status);
}

function resolveZip(version, override) {
  if (override) {
    const abs = path.resolve(override);
    if (!exists(abs)) throw new Error(`指定的 zip 不存在: ${abs}`);
    return abs;
  }
  const expected = path.join(
    root,
    "release",
    "portable",
    `${product}-portable-v${version}.zip`,
  );
  if (exists(expected)) return expected;

  const dir = path.join(root, "release", "portable");
  if (!exists(dir)) {
    throw new Error(
      `未找到便携包。请先运行 npm run pack:portable\n期望: ${expected}`,
    );
  }
  const zips = fs
    .readdirSync(dir)
    .filter((n) => n.toLowerCase().endsWith(".zip"))
    .map((n) => {
      const full = path.join(dir, n);
      return { full, mtime: fs.statSync(full).mtimeMs, name: n };
    })
    .sort((a, b) => b.mtime - a.mtime);

  if (zips.length === 0) {
    throw new Error(
      `未找到便携包。请先运行 npm run pack:portable\n期望: ${expected}`,
    );
  }

  const match = zips.find((z) => z.name.includes(`v${version}`));
  if (match) return match.full;

  console.warn(
    `未找到 v${version} 对应 zip，将使用最新的: ${zips[0].name}`,
  );
  return zips[0].full;
}

function defaultNotes(tag) {
  let prev = "";
  try {
    prev = runCapture(`git describe --tags --abbrev=0 ${tag}^ 2>nul`);
  } catch {
    try {
      prev = runCapture("git describe --tags --abbrev=0 HEAD^ 2>nul");
    } catch {
      prev = "";
    }
  }
  if (!prev) {
    return `Release ${tag}`;
  }
  try {
    const log = runCapture(`git log --pretty=format:"- %s (%h)" ${prev}..HEAD`);
    return log ? `## Changes\n\n${log}` : `Release ${tag}`;
  } catch {
    return `Release ${tag}`;
  }
}

function ensureGhAuth() {
  const r = spawnSync("gh", ["auth", "status"], {
    encoding: "utf8",
    shell: true,
  });
  if (r.status !== 0) {
    throw new Error(
      "GitHub CLI 未登录。请先执行: gh auth login\n或设置环境变量 GH_TOKEN",
    );
  }
}

function tagExistsLocal(tag) {
  try {
    runCapture(`git rev-parse -q --verify refs/tags/${tag}`);
    return true;
  } catch {
    return false;
  }
}

function releaseExists(tag) {
  const r = spawnSync("gh", ["release", "view", tag], {
    encoding: "utf8",
    shell: true,
  });
  return r.status === 0;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(`Usage:
  npm run release:tag
  npm run release:tag -- --notes "说明"
  npm run release:tag -- --zip ./release/portable/xxx.zip
  npm run release:tag -- --dry-run
  npm run release:tag -- --no-push-branch`);
    return;
  }

  const version = readJson(path.join(root, "package.json")).version;
  if (!/^\d+\.\d+\.\d+/.test(version)) {
    throw new Error(`无效版本号: ${version}`);
  }
  const tag = `v${version}`;
  const zipPath = resolveZip(version, args.zip);
  const notes = args.notes || defaultNotes(tag);

  console.log(`版本:   ${version}`);
  console.log(`Tag:    ${tag}`);
  console.log(`Zip:    ${zipPath}`);
  console.log(`大小:   ${(fs.statSync(zipPath).size / 1024 / 1024).toFixed(1)} MB`);
  console.log(`DryRun: ${args.dryRun}`);
  console.log("");

  ensureCleanEnough();

  if (args.dryRun) {
    console.log("[dry-run] 将执行:");
    console.log(`  git tag -a ${tag} -m "Release ${tag}"`);
    console.log(`  git push origin HEAD`);
    console.log(`  git push origin ${tag}`);
    console.log(`  gh release create ${tag} "${zipPath}" ...`);
    return;
  }

  ensureGhAuth();

  if (!tagExistsLocal(tag)) {
    console.log(`==> 创建 annotated tag ${tag}`);
    run(`git tag -a "${tag}" -m "Release ${tag}"`);
  } else {
    console.log(`Tag ${tag} 已存在，跳过创建`);
  }

  if (args.pushBranch) {
    console.log("==> 推送当前分支");
    run("git push -u origin HEAD");
  }

  console.log(`==> 推送 tag ${tag}`);
  run(`git push origin "refs/tags/${tag}"`);

  const notesFile = path.join(root, "release", `.release-notes-${tag}.md`);
  fs.mkdirSync(path.dirname(notesFile), { recursive: true });
  fs.writeFileSync(notesFile, `${notes.trim()}\n`, "utf8");

  try {
    if (releaseExists(tag)) {
      console.log(`==> Release ${tag} 已存在，上传/覆盖资源`);
      run(
        `gh release upload "${tag}" "${zipPath}" --clobber`,
      );
    } else {
      console.log(`==> 创建 GitHub Release ${tag} 并上传 zip`);
      run(
        `gh release create "${tag}" "${zipPath}" --title "${tag}" --notes-file "${notesFile}"`,
      );
    }
  } finally {
    fs.rmSync(notesFile, { force: true });
  }

  const url = runCapture(`gh release view "${tag}" --json url -q .url`);
  console.log("");
  console.log(`OK: ${url}`);
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
