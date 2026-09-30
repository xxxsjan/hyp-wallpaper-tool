# 马哈鱼壁纸工具

<p align="center">
  <img src="src/assets/logo.png" alt="logo" width="96" height="96" />
</p>

<p align="center">
  <strong>HYP Wallpaper Tool</strong><br />
  从启动器缓存里捞壁纸、图片与视频，顺带取出抽卡链接。
</p>

<p align="center">
  <img alt="version" src="https://img.shields.io/badge/version-0.1.4-3ECFB2?style=flat-square" />
  <img alt="platform" src="https://img.shields.io/badge/platform-Windows-0078D4?style=flat-square" />
  <img alt="stack" src="https://img.shields.io/badge/Tauri%202%20%2B%20Vue%203-blueviolet?style=flat-square" />
</p>

---

## 功能

- **壁纸 / 小图**：扫描启动器 Chromium 缓存，按文件头识别 JPEG / PNG / WEBP / GIF；≥ 500KB 归为壁纸，更小的单独分组
- **视频**：识别 Range 分片并合并为完整 WEBM / MP4，支持导出
- **缓存地址**：从缓存索引提取媒体 URL（WebP / WebM / MP4 / PNG / JPG 等）
- **预览与导出**：缩略图预览、多选，导出到指定目录（WebP → PNG、WebM → MP4）

### 支持的启动器

- **马哈鱼**：壁纸、小图、视频、缓存地址
- **鸣潮 KRLauncher**：同上；可粘贴旁路目录（如 `log`），自动定位到 `Cache_Data`

### 抽卡地址

从本机缓存 / 日志读取历史记录链接（不请求游戏服务器）：

| 游戏 | 说明 |
|------|------|
| 原神 | 祈愿历史 URL |
| 崩铁 | 跃迁历史 URL |
| 鸣潮 | 唤取历史 URL |
