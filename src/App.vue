<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  CacheEntry,
  CacheUrl,
  ExportResult,
  GachaUrlResult,
  ScanResult,
  VideoGroup,
} from "./types";
import { formatBytes } from "./types";

const cachePath = ref("");
const scanning = ref(false);
const exporting = ref(false);
const error = ref("");
const status = ref("");
const result = ref<ScanResult | null>(null);

const filterKind = ref<"wallpaper" | "image" | "video" | "urls">("wallpaper");
const urlExtFilter = ref<"all" | "webp" | "webm">("all");
const selected = ref<Set<string>>(new Set());
const selectedVideos = ref<Set<string>>(new Set());
const previews = ref<Record<string, string>>({});
const previewLoading = ref<Set<string>>(new Set());
const lightbox = ref<{ entry: CacheEntry; url: string } | null>(null);
const urlThumbErrors = ref<Set<string>>(new Set());

const gachaOpen = ref(false);
const gachaLoading = ref(false);
const gachaResult = ref<GachaUrlResult | null>(null);
const gachaCopied = ref(false);
const gachaError = ref("");
const gachaStatus = ref("");

const showingVideos = computed(() => filterKind.value === "video");
const showingUrls = computed(() => filterKind.value === "urls");

function isImageEntry(e: CacheEntry): boolean {
  return ["JPEG", "PNG", "WEBP", "GIF"].includes(e.kind);
}

const filtered = computed(() => {
  const entries = result.value?.entries ?? [];
  switch (filterKind.value) {
    case "wallpaper":
      return entries.filter((e) => e.likelyWallpaper);
    case "image":
      return entries.filter((e) => isImageEntry(e) && !e.likelyWallpaper);
    case "video":
    case "urls":
      return [];
  }
});

const imageCount = computed(
  () =>
    (result.value?.entries ?? []).filter(
      (e) => isImageEntry(e) && !e.likelyWallpaper,
    ).length,
);

const videoGroups = computed(() => result.value?.videoGroups ?? []);
const cacheUrls = computed(() => result.value?.cacheUrls ?? []);
const filteredUrls = computed(() => {
  const list = cacheUrls.value;
  if (urlExtFilter.value === "all") return list;
  return list.filter((u) => u.extension === urlExtFilter.value);
});
const urlWebpCount = computed(
  () => cacheUrls.value.filter((u) => u.extension === "webp").length,
);
const urlWebmCount = computed(
  () => cacheUrls.value.filter((u) => u.extension === "webm").length,
);

const selectedCount = computed(() =>
  showingVideos.value ? selectedVideos.value.size : selected.value.size,
);

const exportLabel = computed(() => {
  if (exporting.value) return "导出中…";
  if (showingVideos.value) {
    return selectedCount.value
      ? `合并导出 (${selectedCount.value})`
      : "合并导出视频";
  }
  if (selectedCount.value) return `导出选中 (${selectedCount.value})`;
  return filterKind.value === "image" ? "导出图片" : "导出壁纸";
});

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
  selectedVideos.value = new Set();
  previews.value = {};
  urlThumbErrors.value = new Set();
  urlExtFilter.value = "all";
  try {
    result.value = await invoke<ScanResult>("scan_cache", {
      directory: cachePath.value,
    });
    status.value = `扫描完成：${result.value.total} 个文件，${result.value.wallpapers} 张壁纸，${result.value.entries.filter((e) => isImageEntry(e) && !e.likelyWallpaper).length} 张图片，${result.value.videos} 个可拼接视频，${result.value.urls} 条缓存地址`;
    const targets = [
      ...result.value.entries.filter((e) => e.likelyWallpaper),
      ...result.value.entries.filter(
        (e) => isImageEntry(e) && !e.likelyWallpaper,
      ),
    ].slice(0, 24);
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
  if (previews.value[entry.path] || previewLoading.value.has(entry.path))
    return;
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

function toggleVideo(url: string) {
  const next = new Set(selectedVideos.value);
  if (next.has(url)) next.delete(url);
  else next.add(url);
  selectedVideos.value = next;
}

function selectVisible() {
  if (showingUrls.value) return;
  if (showingVideos.value) {
    const next = new Set(selectedVideos.value);
    for (const g of videoGroups.value) {
      if (g.exportable) next.add(g.url);
    }
    selectedVideos.value = next;
    return;
  }
  const next = new Set(selected.value);
  for (const e of filtered.value) {
    if (e.exportable) next.add(e.path);
  }
  selected.value = next;
}

function clearSelection() {
  selected.value = new Set();
  selectedVideos.value = new Set();
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

async function doExport() {
  if (showingUrls.value) return;
  if (showingVideos.value) {
    await doExportVideos();
    return;
  }

  const paths =
    selected.value.size > 0
      ? [...selected.value]
      : filtered.value.filter((e) => e.exportable).map((e) => e.path);

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
      onlyWallpapers: filterKind.value === "wallpaper",
    });
    status.value = `已导出 ${res.exported} 个文件到 ${res.destination}（跳过 ${res.skipped}）`;
    await invoke("open_in_explorer", { path: destination });
  } catch (e) {
    error.value = String(e);
  } finally {
    exporting.value = false;
  }
}

async function doExportVideos() {
  const groups = videoGroups.value;
  const urls =
    selectedVideos.value.size > 0
      ? [...selectedVideos.value]
      : groups.filter((g) => g.exportable).map((g) => g.url);

  if (!urls.length) {
    error.value = "没有可导出的完整视频";
    return;
  }

  const destination = await open({
    directory: true,
    multiple: false,
    title: "选择视频导出目录",
  });
  if (typeof destination !== "string") return;

  exporting.value = true;
  error.value = "";
  try {
    const res = await invoke<ExportResult>("export_videos", {
      directory: cachePath.value,
      urls,
      destination,
    });
    status.value = `已合并导出 ${res.exported} 个视频到 ${res.destination}（跳过 ${res.skipped}）`;
    await invoke("open_in_explorer", { path: destination });
  } catch (e) {
    error.value = String(e);
  } finally {
    exporting.value = false;
  }
}

function videoLabel(g: VideoGroup): string {
  return g.name.length > 42 ? `${g.name.slice(0, 40)}…` : g.name;
}

async function openCacheUrl(item: CacheUrl) {
  try {
    await openUrl(item.url);
  } catch (e) {
    error.value = String(e);
  }
}

function onUrlThumbError(url: string) {
  const next = new Set(urlThumbErrors.value);
  next.add(url);
  urlThumbErrors.value = next;
}

function openGachaModal() {
  gachaOpen.value = true;
  gachaCopied.value = false;
  gachaError.value = "";
  gachaStatus.value = "";
}

function closeGachaModal() {
  gachaOpen.value = false;
}

async function fetchGachaUrl() {
  gachaLoading.value = true;
  gachaCopied.value = false;
  gachaError.value = "";
  gachaStatus.value = "";
  try {
    gachaResult.value = await invoke<GachaUrlResult>("get_gacha_url");
    gachaStatus.value = `已获取 ${gachaResult.value.game} 抽卡地址`;
  } catch (e) {
    gachaResult.value = null;
    gachaError.value = String(e);
  } finally {
    gachaLoading.value = false;
  }
}

async function copyGachaUrl() {
  if (!gachaResult.value?.url) return;
  try {
    await navigator.clipboard.writeText(gachaResult.value.url);
    gachaCopied.value = true;
    gachaStatus.value = "抽卡地址已复制到剪贴板";
    window.setTimeout(() => {
      gachaCopied.value = false;
    }, 2000);
  } catch (e) {
    gachaError.value = String(e);
  }
}

watch(filterKind, async () => {
  selected.value = new Set();
  selectedVideos.value = new Set();
  if (showingVideos.value || showingUrls.value) return;
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
          识别启动器缓存里的壁纸、图片与视频分片，预览并导出，或合并还原完整视频。
        </p>
      </div>
      <div class="hero-side">
        <button class="btn accent" type="button" @click="openGachaModal">
          抽卡地址
        </button>
        <div class="stats" v-if="result">
          <div class="stat">
            <span class="num">{{ result.total }}</span>
            <span class="lbl">缓存文件</span>
          </div>
          <div class="stat">
            <span class="num">{{ result.wallpapers }}</span>
            <span class="lbl">壁纸</span>
          </div>
          <div class="stat">
            <span class="num">{{ imageCount }}</span>
            <span class="lbl">图片</span>
          </div>
          <div class="stat">
            <span class="num">{{ result.videos }}</span>
            <span class="lbl">可拼视频</span>
          </div>
          <div class="stat">
            <span class="num">{{ result.urls }}</span>
            <span class="lbl">缓存地址</span>
          </div>
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
        <button class="btn ghost" type="button" @click="pickFolder">
          浏览
        </button>
        <button
          class="btn primary"
          type="button"
          :disabled="scanning"
          @click="scan"
        >
          {{ scanning ? "扫描中…" : "扫描" }}
        </button>
      </div>

      <div class="actions">
        <div class="filters">
          <button
            v-for="f in [
              { id: 'wallpaper', label: '壁纸' },
              { id: 'image', label: '图片（小）' },
              { id: 'video', label: '视频' },
              { id: 'urls', label: '地址' },
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
        <div class="export-row" v-if="!showingUrls">
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
            @click="doExport()"
          >
            {{ exportLabel }}
          </button>
        </div>
        <div class="export-row" v-else>
          <span class="url-meta">点击打开浏览器预览</span>
        </div>
      </div>

      <p v-if="status" class="status ok">{{ status }}</p>
      <p v-if="error" class="status err">{{ error }}</p>
    </section>

    <section class="url-panel" v-if="showingUrls && cacheUrls.length">
      <div class="filters url-ext-filters">
        <button
          v-for="f in [
            { id: 'all', label: `全部 (${cacheUrls.length})` },
            { id: 'webp', label: `WebP图片 (${urlWebpCount})` },
            { id: 'webm', label: `WebM视频 (${urlWebmCount})` },
          ]"
          :key="f.id"
          type="button"
          class="chip"
          :class="{ active: urlExtFilter === f.id }"
          @click="urlExtFilter = f.id as typeof urlExtFilter"
        >
          {{ f.label }}
        </button>
      </div>

      <div class="url-list" v-if="filteredUrls.length">
        <div class="url-list-head">
          <span>预览</span>
          <span>日期</span>
          <span>格式</span>
          <span>地址</span>
        </div>
        <article
          v-for="item in filteredUrls"
          :key="item.url"
          class="url-row"
          role="button"
          tabindex="0"
          @click="openCacheUrl(item)"
          @keydown.enter="openCacheUrl(item)"
        >
          <div class="url-thumb">
            <img
              v-if="item.kind === 'image' && !urlThumbErrors.has(item.url)"
              :src="item.url"
              :alt="item.extension"
              loading="lazy"
              referrerpolicy="no-referrer"
              @error="onUrlThumbError(item.url)"
            />
            <span v-else class="url-thumb-fallback">{{
              item.extension.toUpperCase()
            }}</span>
          </div>
          <span class="url-date">{{ item.date }}</span>
          <span class="url-kind">{{ item.extension.toUpperCase() }}</span>
          <div class="url-main">
            <code class="url-host">{{ item.host }}</code>
            <span class="url-full" :title="item.url">{{ item.url }}</span>
          </div>
        </article>
      </div>
      <section v-else class="empty">
        <p>当前筛选下没有地址。</p>
      </section>
    </section>

    <section class="grid" v-else-if="showingVideos && videoGroups.length">
      <article
        v-for="group in videoGroups"
        :key="group.id"
        class="card video-card"
        :class="{
          selected: selectedVideos.has(group.url),
          muted: !group.exportable,
        }"
      >
        <div class="thumb video-thumb">
          <div class="placeholder">
            <span>{{ group.kind }}</span>
            <small>{{ group.shardCount }} 个分片</small>
          </div>
          <span class="badge">{{
            group.exportable ? "可合并" : "不完整"
          }}</span>
        </div>

        <div class="meta">
          <label class="check">
            <input
              type="checkbox"
              :checked="selectedVideos.has(group.url)"
              :disabled="!group.exportable"
              @change="toggleVideo(group.url)"
            />
            <code :title="group.url">{{ videoLabel(group) }}</code>
          </label>
          <div class="sub">
            <span>{{ group.kind }}</span>
            <span>{{ formatBytes(group.totalSize) }}</span>
          </div>
          <p class="note">{{ group.note }}</p>
          <p v-if="group.shardDate" class="note date">
            缓存日期 {{ group.shardDate }}
          </p>
        </div>
      </article>
    </section>

    <section
      class="grid"
      v-else-if="!showingVideos && !showingUrls && filtered.length"
    >
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
            <small v-else-if="entry.note && entry.kind !== 'chunk'">{{
              entry.note
            }}</small>
          </div>
          <span class="badge">{{
            filterKind === "image" ? "图片" : "壁纸"
          }}</span>
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
      <p v-if="showingVideos">
        没有识别到可拼接的视频分片。确认目录含有 data_1 与 f_* 文件。
      </p>
      <p v-else-if="showingUrls">没有带日期的 WebP / WebM 地址。</p>
      <p v-else-if="filterKind === 'image'">没有小于 500KB 的图片。</p>
      <p v-else>当前筛选下没有壁纸。试着重新扫描缓存目录。</p>
    </section>

    <div v-if="lightbox" class="lightbox" @click.self="lightbox = null">
      <div class="lightbox-panel">
        <header>
          <div>
            <strong
              >{{ lightbox.entry.name }}.{{ lightbox.entry.extension }}</strong
            >
            <span
              >{{ lightbox.entry.kind }} ·
              {{ formatBytes(lightbox.entry.size) }}</span
            >
          </div>
          <button class="btn ghost" type="button" @click="lightbox = null">
            关闭
          </button>
        </header>
        <img
          v-if="!['WEBM', 'MP4'].includes(lightbox.entry.kind)"
          :src="lightbox.url"
          :alt="lightbox.entry.name"
        />
        <video v-else :src="lightbox.url" controls autoplay />
      </div>
    </div>

    <div
      v-if="gachaOpen"
      class="lightbox"
      @click.self="closeGachaModal"
      @keydown.esc="closeGachaModal"
    >
      <div class="lightbox-panel gacha-modal" role="dialog" aria-modal="true">
        <header>
          <div>
            <strong>原神抽卡地址</strong>
            <span>从本地游戏缓存读取祈愿历史 URL</span>
          </div>
          <button class="btn ghost" type="button" @click="closeGachaModal">
            关闭
          </button>
        </header>

        <p class="gacha-hint">
          先在游戏内打开「祈愿 → 历史记录」并等待加载完成，再点「获取」。仅读取本地缓存，不拉取记录。
        </p>

        <div class="gacha-actions">
          <button
            class="btn primary"
            type="button"
            :disabled="gachaLoading"
            @click="fetchGachaUrl"
          >
            {{ gachaLoading ? "获取中…" : "获取抽卡地址" }}
          </button>
          <button
            class="btn accent"
            type="button"
            :disabled="!gachaResult?.url"
            @click="copyGachaUrl"
          >
            {{ gachaCopied ? "已复制" : "复制" }}
          </button>
        </div>

        <p v-if="gachaStatus" class="status ok">{{ gachaStatus }}</p>
        <p v-if="gachaError" class="status err">{{ gachaError }}</p>

        <div class="gacha-card" v-if="gachaResult">
          <div class="gacha-meta">
            <span class="gacha-game">{{ gachaResult.game }}</span>
            <code class="gacha-source" :title="gachaResult.source">{{
              gachaResult.source
            }}</code>
          </div>
          <textarea
            class="gacha-url"
            readonly
            spellcheck="false"
            :value="gachaResult.url"
            @focus="($event.target as HTMLTextAreaElement).select()"
          />
        </div>
        <p v-else-if="!gachaLoading && !gachaError" class="gacha-empty">
          尚未获取抽卡地址。
        </p>
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
    radial-gradient(
      1200px 600px at 10% -10%,
      rgba(62, 207, 178, 0.12),
      transparent 55%
    ),
    radial-gradient(
      900px 500px at 100% 0%,
      rgba(232, 164, 92, 0.1),
      transparent 50%
    ),
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

.hero-side {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 12px;
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
  min-width: 84px;
  padding: 12px 12px;
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
  background: linear-gradient(
    180deg,
    rgba(36, 43, 54, 0.9),
    rgba(26, 31, 39, 0.75)
  );
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
  transition:
    border-color 0.15s ease,
    transform 0.15s ease;
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

.note {
  margin: 8px 0 0;
  color: var(--muted);
  font-size: 0.72rem;
  line-height: 1.35;
  word-break: break-all;
}

.note.date {
  margin-top: 4px;
  color: var(--warn);
  word-break: normal;
}

.url-meta {
  color: var(--muted);
  font-size: 0.85rem;
}

.gacha-modal {
  width: min(640px, 100%);
}

.gacha-hint {
  margin: 0 0 14px;
  color: var(--muted);
  line-height: 1.55;
  font-size: 0.92rem;
}

.gacha-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 12px;
}

.gacha-card {
  margin-top: 8px;
  padding: 14px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: rgba(26, 31, 39, 0.78);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.gacha-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 14px;
  align-items: baseline;
}

.gacha-game {
  color: var(--accent);
  font-weight: 600;
  font-size: 0.9rem;
}

.gacha-source {
  color: var(--muted);
  font-size: 0.75rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.gacha-url {
  width: 100%;
  min-height: 140px;
  resize: vertical;
  padding: 12px;
  border-radius: 12px;
  border: 1px solid var(--line);
  background: var(--bg0);
  color: var(--text);
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.78rem;
  line-height: 1.45;
  word-break: break-all;
}

.gacha-empty {
  margin: 8px 0 0;
  color: var(--muted);
  font-size: 0.9rem;
}

.url-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.url-ext-filters {
  margin-bottom: 2px;
}

.url-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.url-list-head,
.url-row {
  display: grid;
  grid-template-columns: 64px 7.5rem 3.5rem 1fr;
  gap: 12px;
  align-items: center;
}

.url-list-head {
  padding: 0 12px 6px;
  color: var(--muted);
  font-size: 0.75rem;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.url-row {
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: rgba(26, 31, 39, 0.72);
  cursor: pointer;
  transition:
    border-color 0.15s ease,
    background 0.15s ease;
}

.url-row:hover,
.url-row:focus-visible {
  border-color: rgba(62, 207, 178, 0.45);
  background: rgba(36, 43, 54, 0.9);
  outline: none;
}

.url-thumb {
  width: 56px;
  height: 40px;
  border-radius: 8px;
  overflow: hidden;
  background: var(--bg0);
  display: grid;
  place-items: center;
}

.url-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.url-thumb-fallback {
  font-size: 0.68rem;
  color: var(--muted);
  font-family: "IBM Plex Mono", monospace;
}

.url-date {
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.82rem;
  color: var(--warn);
}

.url-kind {
  font-size: 0.78rem;
  color: var(--accent);
}

.url-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.url-host {
  font-size: 0.78rem;
  color: var(--muted);
}

.url-full {
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.72rem;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 860px) {
  .url-list-head {
    display: none;
  }

  .url-row {
    grid-template-columns: 56px 1fr;
    grid-template-areas:
      "thumb date"
      "thumb kind"
      "thumb main";
  }

  .url-thumb {
    grid-area: thumb;
  }

  .url-date {
    grid-area: date;
  }

  .url-kind {
    grid-area: kind;
  }

  .url-main {
    grid-area: main;
  }
}

.video-thumb {
  cursor: default;
}

.video-card .check code {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

  .hero-side {
    align-items: flex-start;
    width: 100%;
  }
}
</style>
