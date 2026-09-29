<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { CacheEntry, ExportResult, ScanResult } from "./types";
import { formatBytes } from "./types";

const cachePath = ref("");
const scanning = ref(false);
const exporting = ref(false);
const error = ref("");
const status = ref("");
const result = ref<ScanResult | null>(null);

const filterKind = ref<"all" | "wallpaper" | "image" | "video" | "chunk">("wallpaper");
const selected = ref<Set<string>>(new Set());
const previews = ref<Record<string, string>>({});
const previewLoading = ref<Set<string>>(new Set());
const lightbox = ref<{ entry: CacheEntry; url: string } | null>(null);

const filtered = computed(() => {
  const entries = result.value?.entries ?? [];
  switch (filterKind.value) {
    case "wallpaper":
      return entries.filter((e) => e.likelyWallpaper);
    case "image":
      return entries.filter((e) =>
        ["JPEG", "PNG", "WEBP", "GIF"].includes(e.kind),
      );
    case "video":
      return entries.filter((e) => ["WEBM", "MP4"].includes(e.kind));
    case "chunk":
      return entries.filter((e) => e.kind === "chunk" || e.kind === "unknown");
    default:
      return entries;
  }
});

const selectedCount = computed(() => selected.value.size);

async function init() {
  try {
    cachePath.value = await invoke<string>("default_cache_path");
    await scan();
  } catch (e) {
    error.value = String(e);
  }
}

async function pickFolder() {
  const dir = await open({ directory: true, multiple: false });
  if (typeof dir === "string") {
    cachePath.value = dir;
  }
}

async function scan() {
  if (!cachePath.value) return;
  scanning.value = true;
  error.value = "";
  status.value = "";
  selected.value = new Set();
  previews.value = {};
  try {
    result.value = await invoke<ScanResult>("scan_cache", {
      directory: cachePath.value,
    });
    status.value = `扫描完成：${result.value.total} 个文件，${result.value.wallpapers} 张疑似壁纸`;
    // Prefetch previews for visible wallpapers (limit concurrency)
    const targets = result.value.entries
      .filter((e) => e.likelyWallpaper)
      .slice(0, 24);
    await loadPreviews(targets);
  } catch (e) {
    error.value = String(e);
    result.value = null;
  } finally {
    scanning.value = false;
  }
}

async function loadPreviews(entries: CacheEntry[]) {
  const queue = [...entries];
  const workers = Array.from({ length: 4 }, async () => {
    while (queue.length) {
      const entry = queue.shift();
      if (!entry) break;
      await loadOnePreview(entry);
    }
  });
  await Promise.all(workers);
}

async function loadOnePreview(entry: CacheEntry) {
  if (!entry.exportable) return;
  if (previews.value[entry.path] || previewLoading.value.has(entry.path)) return;
  if (["WEBM", "MP4"].includes(entry.kind)) return;

  previewLoading.value.add(entry.path);
  try {
    const url = await invoke<string>("preview_data_url", { path: entry.path });
    previews.value = { ...previews.value, [entry.path]: url };
  } catch {
    // ignore preview failures
  } finally {
    previewLoading.value.delete(entry.path);
  }
}

function toggleSelect(path: string) {
  const next = new Set(selected.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  selected.value = next;
}

function selectVisible() {
  const next = new Set(selected.value);
  for (const e of filtered.value) {
    if (e.exportable) next.add(e.path);
  }
  selected.value = next;
}

function clearSelection() {
  selected.value = new Set();
}

async function openPreview(entry: CacheEntry) {
  if (!entry.exportable) return;
  let url = previews.value[entry.path];
  if (!url) {
    try {
      url = await invoke<string>("preview_data_url", { path: entry.path });
      previews.value = { ...previews.value, [entry.path]: url };
    } catch (e) {
      error.value = String(e);
      return;
    }
  }
  lightbox.value = { entry, url };
}

async function doExport(onlyWallpapers: boolean) {
  const paths =
    selected.value.size > 0
      ? [...selected.value]
      : (result.value?.entries ?? [])
          .filter((e) => (onlyWallpapers ? e.likelyWallpaper : e.exportable))
          .map((e) => e.path);

  if (!paths.length) {
    error.value = "没有可导出的文件";
    return;
  }

  const destination = await open({
    directory: true,
    multiple: false,
    title: "选择导出目录",
  });
  if (typeof destination !== "string") return;

  exporting.value = true;
  error.value = "";
  try {
    const res = await invoke<ExportResult>("export_entries", {
      paths,
      destination,
      onlyWallpapers,
    });
    status.value = `已导出 ${res.exported} 个文件到 ${res.destination}（跳过 ${res.skipped}）`;
    await invoke("open_in_explorer", { path: destination });
  } catch (e) {
    error.value = String(e);
  } finally {
    exporting.value = false;
  }
}

watch(filterKind, async () => {
  const need = filtered.value
    .filter((e) => e.exportable && !previews.value[e.path])
    .filter((e) => !["WEBM", "MP4"].includes(e.kind))
    .slice(0, 24);
  if (need.length) await loadPreviews(need);
});

onMounted(init);
</script>

<template>
  <div class="app">
    <header class="hero">
      <div class="brand">
        <p class="eyebrow">HoYoPlay Cache</p>
        <h1>Wallpaper Tool</h1>
        <p class="tagline">
          识别启动器缓存里的完整图片，预览并导出为可用壁纸。
        </p>
      </div>
      <div class="stats" v-if="result">
        <div class="stat">
          <span class="num">{{ result.total }}</span>
          <span class="lbl">缓存文件</span>
        </div>
        <div class="stat">
          <span class="num">{{ result.wallpapers }}</span>
          <span class="lbl">疑似壁纸</span>
        </div>
        <div class="stat">
          <span class="num">{{ result.exportable }}</span>
          <span class="lbl">可导出</span>
        </div>
      </div>
    </header>

    <section class="toolbar">
      <div class="path-row">
        <input
          class="path"
          v-model="cachePath"
          spellcheck="false"
          placeholder="Cache_Data 目录路径"
        />
        <button class="btn ghost" type="button" @click="pickFolder">浏览</button>
        <button class="btn primary" type="button" :disabled="scanning" @click="scan">
          {{ scanning ? "扫描中…" : "扫描" }}
        </button>
      </div>

      <div class="actions">
        <div class="filters">
          <button
            v-for="f in [
              { id: 'wallpaper', label: '壁纸' },
              { id: 'image', label: '图片' },
              { id: 'video', label: '视频' },
              { id: 'chunk', label: '分片' },
              { id: 'all', label: '全部' },
            ]"
            :key="f.id"
            type="button"
            class="chip"
            :class="{ active: filterKind === f.id }"
            @click="filterKind = f.id as typeof filterKind"
          >
            {{ f.label }}
          </button>
        </div>
        <div class="export-row">
          <button class="btn ghost" type="button" @click="selectVisible">
            全选当前
          </button>
          <button class="btn ghost" type="button" @click="clearSelection">
            清除
          </button>
          <button
            class="btn accent"
            type="button"
            :disabled="exporting"
            @click="doExport(true)"
          >
            {{ exporting ? "导出中…" : selectedCount ? `导出选中 (${selectedCount})` : "导出壁纸" }}
          </button>
        </div>
      </div>

      <p v-if="status" class="status ok">{{ status }}</p>
      <p v-if="error" class="status err">{{ error }}</p>
    </section>

    <section class="grid" v-if="filtered.length">
      <article
        v-for="entry in filtered"
        :key="entry.path"
        class="card"
        :class="{
          selected: selected.has(entry.path),
          muted: !entry.exportable,
        }"
      >
        <button
          class="thumb"
          type="button"
          @click="openPreview(entry)"
          :disabled="!entry.exportable"
        >
          <img
            v-if="previews[entry.path]"
            :src="previews[entry.path]"
            :alt="entry.name"
            loading="lazy"
          />
          <div v-else class="placeholder">
            <span>{{ entry.kind }}</span>
            <small v-if="previewLoading.has(entry.path)">加载预览…</small>
            <small v-else-if="entry.note">{{ entry.note }}</small>
          </div>
          <span v-if="entry.likelyWallpaper" class="badge">壁纸</span>
        </button>

        <div class="meta">
          <label class="check">
            <input
              type="checkbox"
              :checked="selected.has(entry.path)"
              :disabled="!entry.exportable"
              @change="toggleSelect(entry.path)"
            />
            <code>{{ entry.name }}</code>
          </label>
          <div class="sub">
            <span>{{ entry.kind }}</span>
            <span>{{ formatBytes(entry.size) }}</span>
          </div>
        </div>
      </article>
    </section>

    <section v-else-if="!scanning" class="empty">
      <p>当前筛选下没有文件。试着切换筛选或重新扫描缓存目录。</p>
    </section>

    <div v-if="lightbox" class="lightbox" @click.self="lightbox = null">
      <div class="lightbox-panel">
        <header>
          <div>
            <strong>{{ lightbox.entry.name }}.{{ lightbox.entry.extension }}</strong>
            <span
              >{{ lightbox.entry.kind }} · {{ formatBytes(lightbox.entry.size) }}</span
            >
          </div>
          <button class="btn ghost" type="button" @click="lightbox = null">关闭</button>
        </header>
        <img
          v-if="!['WEBM', 'MP4'].includes(lightbox.entry.kind)"
          :src="lightbox.url"
          :alt="lightbox.entry.name"
        />
        <video v-else :src="lightbox.url" controls autoplay />
      </div>
    </div>
  </div>
</template>

<style>
:root {
  --bg0: #12151a;
  --bg1: #1a1f27;
  --bg2: #242b36;
  --line: rgba(232, 220, 196, 0.12);
  --text: #efe7d8;
  --muted: #9aa3b2;
  --accent: #3ecfb2;
  --accent-dim: rgba(62, 207, 178, 0.16);
  --warn: #e8a45c;
  --danger: #e06b6b;
  --shadow: 0 18px 50px rgba(0, 0, 0, 0.35);
  font-family: "Outfit", sans-serif;
  color: var(--text);
  background:
    radial-gradient(1200px 600px at 10% -10%, rgba(62, 207, 178, 0.12), transparent 55%),
    radial-gradient(900px 500px at 100% 0%, rgba(232, 164, 92, 0.1), transparent 50%),
    var(--bg0);
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
}

* {
  box-sizing: border-box;
}

html,
body,
#app {
  margin: 0;
  min-height: 100%;
}

body {
  min-height: 100vh;
}

button,
input {
  font: inherit;
}

code {
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.82rem;
}
</style>

<style scoped>
.app {
  max-width: 1200px;
  margin: 0 auto;
  padding: 28px 24px 48px;
}

.hero {
  display: flex;
  justify-content: space-between;
  gap: 24px;
  align-items: end;
  margin-bottom: 22px;
}

.eyebrow {
  margin: 0 0 6px;
  color: var(--accent);
  letter-spacing: 0.14em;
  text-transform: uppercase;
  font-size: 0.75rem;
  font-weight: 600;
}

h1 {
  margin: 0;
  font-size: clamp(2rem, 4vw, 2.8rem);
  letter-spacing: -0.03em;
  line-height: 1.05;
}

.tagline {
  margin: 10px 0 0;
  color: var(--muted);
  max-width: 34rem;
  line-height: 1.5;
}

.stats {
  display: flex;
  gap: 10px;
}

.stat {
  min-width: 96px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: rgba(26, 31, 39, 0.8);
  backdrop-filter: blur(8px);
}

.num {
  display: block;
  font-size: 1.4rem;
  font-weight: 700;
  letter-spacing: -0.03em;
}

.lbl {
  color: var(--muted);
  font-size: 0.78rem;
}

.toolbar {
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(36, 43, 54, 0.9), rgba(26, 31, 39, 0.75));
  box-shadow: var(--shadow);
  margin-bottom: 22px;
}

.path-row,
.actions,
.export-row,
.filters {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  align-items: center;
}

.path-row {
  margin-bottom: 12px;
}

.path {
  flex: 1;
  min-width: 220px;
  padding: 0.7rem 0.9rem;
  border-radius: 12px;
  border: 1px solid var(--line);
  background: var(--bg0);
  color: var(--text);
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.82rem;
}

.actions {
  justify-content: space-between;
}

.btn {
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 0.65rem 1rem;
  background: var(--bg2);
  color: var(--text);
  cursor: pointer;
  transition: 0.15s ease;
}

.btn:hover:not(:disabled) {
  border-color: rgba(62, 207, 178, 0.45);
}

.btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.btn.primary {
  background: var(--text);
  color: #12151a;
  border-color: transparent;
  font-weight: 600;
}

.btn.accent {
  background: var(--accent);
  color: #0b1c18;
  border-color: transparent;
  font-weight: 600;
}

.btn.ghost {
  background: transparent;
}

.chip {
  border: 1px solid var(--line);
  background: transparent;
  color: var(--muted);
  border-radius: 999px;
  padding: 0.4rem 0.85rem;
  cursor: pointer;
}

.chip.active {
  color: var(--accent);
  border-color: rgba(62, 207, 178, 0.45);
  background: var(--accent-dim);
}

.status {
  margin: 12px 0 0;
  font-size: 0.9rem;
}

.status.ok {
  color: var(--accent);
}

.status.err {
  color: var(--danger);
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 14px;
}

.card {
  border: 1px solid var(--line);
  border-radius: 16px;
  overflow: hidden;
  background: rgba(26, 31, 39, 0.88);
  transition: border-color 0.15s ease, transform 0.15s ease;
}

.card:hover {
  transform: translateY(-2px);
  border-color: rgba(232, 220, 196, 0.22);
}

.card.selected {
  border-color: rgba(62, 207, 178, 0.65);
  box-shadow: 0 0 0 1px rgba(62, 207, 178, 0.25);
}

.card.muted {
  opacity: 0.72;
}

.thumb {
  position: relative;
  display: block;
  width: 100%;
  aspect-ratio: 16 / 10;
  padding: 0;
  border: 0;
  background: #0d1014;
  cursor: pointer;
  overflow: hidden;
}

.thumb:disabled {
  cursor: default;
}

.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.placeholder {
  width: 100%;
  height: 100%;
  display: grid;
  place-content: center;
  gap: 6px;
  color: var(--muted);
  text-align: center;
  padding: 12px;
}

.placeholder span {
  font-weight: 600;
  color: var(--warn);
}

.badge {
  position: absolute;
  top: 10px;
  left: 10px;
  padding: 0.2rem 0.55rem;
  border-radius: 999px;
  background: rgba(62, 207, 178, 0.9);
  color: #06231c;
  font-size: 0.72rem;
  font-weight: 700;
}

.meta {
  padding: 10px 12px 12px;
}

.check {
  display: flex;
  gap: 8px;
  align-items: center;
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
}

.sub {
  margin-top: 6px;
  display: flex;
  justify-content: space-between;
  color: var(--muted);
  font-size: 0.78rem;
}

.empty {
  padding: 48px 20px;
  text-align: center;
  color: var(--muted);
  border: 1px dashed var(--line);
  border-radius: 16px;
}

.lightbox {
  position: fixed;
  inset: 0;
  background: rgba(8, 10, 14, 0.78);
  backdrop-filter: blur(8px);
  display: grid;
  place-items: center;
  padding: 24px;
  z-index: 50;
}

.lightbox-panel {
  width: min(960px, 100%);
  max-height: 90vh;
  overflow: auto;
  background: var(--bg1);
  border: 1px solid var(--line);
  border-radius: 18px;
  box-shadow: var(--shadow);
  padding: 14px;
}

.lightbox-panel header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
  margin-bottom: 12px;
}

.lightbox-panel header span {
  display: block;
  color: var(--muted);
  font-size: 0.85rem;
  margin-top: 2px;
}

.lightbox-panel img,
.lightbox-panel video {
  width: 100%;
  max-height: 72vh;
  object-fit: contain;
  border-radius: 12px;
  background: #000;
}

@media (max-width: 720px) {
  .hero {
    flex-direction: column;
    align-items: start;
  }
}
</style>
