<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { GachaUrlResult } from "../types";

export type GachaGame = "genshin" | "starrail" | "wuthering";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ (e: "update:modelValue", value: boolean): void }>();

const game = ref<GachaGame>("genshin");
const loading = ref(false);
const result = ref<GachaUrlResult | null>(null);
const copied = ref(false);
const error = ref("");
const status = ref("");
let copiedTimer: ReturnType<typeof setTimeout> | null = null;

const title = computed(() => {
  switch (game.value) {
    case "starrail": return "崩铁抽卡地址";
    case "wuthering": return "鸣潮抽卡地址";
    default: return "原神抽卡地址";
  }
});
const hint = computed(() => {
  switch (game.value) {
    case "starrail": return "先在游戏内打开「跃迁 → 详情/历史记录」并等待加载完成，再点「获取」。仅读取本地缓存，不拉取记录。";
    case "wuthering": return "先在游戏内打开「唤取记录」并等待加载完成，再点「获取」。仅读取本地 Client.log，不拉取记录。";
    default: return "先在游戏内打开「祈愿 → 历史记录」并等待加载完成，再点「获取」。仅读取本地缓存，不拉取记录。";
  }
});
const subtitle = computed(() => {
  switch (game.value) {
    case "starrail": return "从本地游戏缓存读取跃迁历史 URL";
    case "wuthering": return "从本地 Client.log 读取唤取历史 URL";
    default: return "从本地游戏缓存读取祈愿历史 URL";
  }
});

watch(() => props.modelValue, (open) => {
  if (open) { copied.value = false; error.value = ""; status.value = ""; }
});

function close() { emit("update:modelValue", false); }
function onKeydown(e: KeyboardEvent) { if (e.key === "Escape") close(); }
watch(() => props.modelValue, (open) => {
  if (open) window.addEventListener("keydown", onKeydown);
  else window.removeEventListener("keydown", onKeydown);
});
onUnmounted(() => window.removeEventListener("keydown", onKeydown));

function selectGame(next: GachaGame) {
  if (game.value === next) return;
  game.value = next;
  result.value = null;
  copied.value = false;
  error.value = "";
  status.value = "";
}

async function fetchUrl() {
  loading.value = true;
  copied.value = false;
  error.value = "";
  status.value = "";
  try {
    const cmd = game.value === "starrail" ? "get_star_rail_gacha_url" : game.value === "wuthering" ? "get_wuthering_gacha_url" : "get_gacha_url";
    result.value = await invoke<GachaUrlResult>(cmd);
    status.value = `已获取 ${result.value.game} 抽卡地址`;
  } catch (e) {
    result.value = null;
    error.value = String(e);
  } finally { loading.value = false; }
}

async function copyUrl() {
  if (!result.value?.url) return;
  try {
    await navigator.clipboard.writeText(result.value.url);
    copied.value = true;
    status.value = "抽卡地址已复制到剪贴板";
    if (copiedTimer) clearTimeout(copiedTimer);
    copiedTimer = window.setTimeout(() => { copied.value = false; copiedTimer = null; }, 2000);
  } catch (e) { error.value = String(e); }
}
</script>

<template>
  <div v-if="modelValue" class="lightbox" @click.self="close">
    <div class="lightbox-panel gacha-modal" role="dialog" aria-modal="true">
      <header>
        <div>
          <strong>{{ title }}</strong>
          <span>{{ subtitle }}</span>
        </div>
        <button class="btn ghost" type="button" @click="close">关闭</button>
      </header>
      <div class="gacha-tabs" role="tablist">
        <button type="button" role="tab" class="gacha-tab" :class="{ active: game === 'genshin' }" :aria-selected="game === 'genshin'" :disabled="loading" @click="selectGame('genshin')">原神</button>
        <button type="button" role="tab" class="gacha-tab" :class="{ active: game === 'starrail' }" :aria-selected="game === 'starrail'" :disabled="loading" @click="selectGame('starrail')">崩铁</button>
        <button type="button" role="tab" class="gacha-tab" :class="{ active: game === 'wuthering' }" :aria-selected="game === 'wuthering'" :disabled="loading" @click="selectGame('wuthering')">鸣潮</button>
      </div>
      <p class="gacha-hint">{{ hint }}</p>
      <div class="gacha-actions">
        <button class="btn primary" type="button" :disabled="loading" @click="fetchUrl">{{ loading ? "获取中…" : "获取抽卡地址" }}</button>
        <button class="btn accent" type="button" :disabled="!result?.url" @click="copyUrl">{{ copied ? "已复制" : "复制" }}</button>
      </div>
      <p v-if="status" class="status ok">{{ status }}</p>
      <p v-if="error" class="status err">{{ error }}</p>
      <div class="gacha-card" v-if="result">
        <div class="gacha-meta">
          <span class="gacha-game">{{ result.game }}</span>
          <code class="gacha-source" :title="result.source">{{ result.source }}</code>
        </div>
        <textarea class="gacha-url" readonly spellcheck="false" :value="result.url" @focus="($event.target as HTMLTextAreaElement).select()" />
      </div>
      <p v-else-if="!loading && !error" class="gacha-empty">尚未获取抽卡地址。</p>
    </div>
  </div>
</template>

<style scoped>
.lightbox { position: fixed; inset: 0; background: rgba(8,10,14,0.78); backdrop-filter: blur(8px); display: grid; place-items: center; padding: 24px; z-index: 50; }
.lightbox-panel { width: min(960px, 100%); max-height: 90vh; overflow: auto; background: var(--bg1); border: 1px solid var(--line); border-radius: 18px; box-shadow: var(--shadow); padding: 14px; }
.lightbox-panel header { display: flex; justify-content: space-between; gap: 12px; align-items: center; margin-bottom: 12px; }
.lightbox-panel header span { display: block; color: var(--muted); font-size: 0.85rem; margin-top: 2px; }
.btn { border: 1px solid var(--line); border-radius: 12px; padding: 0.65rem 1rem; background: var(--bg2); color: var(--text); cursor: pointer; transition: 0.15s ease; }
.btn:hover:not(:disabled) { border-color: rgba(62,207,178,0.45); }
.btn:disabled { opacity: 0.55; cursor: not-allowed; }
.btn.primary { background: var(--text); color: #12151a; border-color: transparent; font-weight: 600; }
.btn.accent { background: var(--accent); color: #0b1c18; border-color: transparent; font-weight: 600; }
.btn.ghost { background: transparent; }
.status { margin: 12px 0 0; font-size: 0.9rem; }
.status.ok { color: var(--accent); }
.status.err { color: var(--danger); }
code { font-family: "IBM Plex Mono", monospace; font-size: 0.82rem; }
.gacha-modal { width: min(640px, 100%); }
.gacha-tabs { display: flex; gap: 6px; margin: 0 0 14px; padding: 4px; border-radius: 12px; background: rgba(18,21,26,0.72); border: 1px solid var(--line); }
.gacha-tab { flex: 1; border: 0; border-radius: 9px; padding: 8px 12px; background: transparent; color: var(--muted); font: inherit; font-size: 0.9rem; cursor: pointer; transition: background 0.15s ease, color 0.15s ease; }
.gacha-tab:hover:not(:disabled) { color: var(--text); }
.gacha-tab.active { background: var(--bg2); color: var(--text); }
.gacha-tab:disabled { opacity: 0.55; cursor: not-allowed; }
.gacha-hint { margin: 0 0 14px; color: var(--muted); line-height: 1.55; font-size: 0.92rem; }
.gacha-actions { display: flex; flex-wrap: wrap; gap: 10px; margin-bottom: 12px; }
.gacha-card { margin-top: 8px; padding: 14px; border: 1px solid var(--line); border-radius: 14px; background: rgba(26,31,39,0.78); display: flex; flex-direction: column; gap: 12px; }
.gacha-meta { display: flex; flex-wrap: wrap; gap: 10px 14px; align-items: baseline; }
.gacha-game { color: var(--accent); font-weight: 600; font-size: 0.9rem; }
.gacha-source { color: var(--muted); font-size: 0.75rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
.gacha-url { width: 100%; min-height: 140px; resize: vertical; padding: 12px; border-radius: 12px; border: 1px solid var(--line); background: var(--bg0); color: var(--text); font-family: "IBM Plex Mono", monospace; font-size: 0.78rem; line-height: 1.45; word-break: break-all; }
.gacha-empty { margin: 8px 0 0; color: var(--muted); font-size: 0.9rem; }
</style>

