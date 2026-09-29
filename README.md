# HYP Wallpaper Tool

Tauri + Vue 工具：扫描米哈游启动器（HoYoPlay）Chromium 缓存，识别完整图片并导出为壁纸。

## 功能

- 默认打开 `%APPDATA%\miHoYo\HYP\1_1\fedata\Cache\Cache_Data`
- 按文件头识别 JPEG / PNG / WEBP / GIF / WEBM / MP4
- 标记疑似壁纸（完整图片且 ≥ 500KB）
- 预览、多选、导出到指定目录（自动补后缀）
- 识别 1MB 视频缓存分片（不可单独播放）

## 开发

```bash
cd hyp-wallpaper-tool
npm install
npm run tauri dev
```

## 打包

```bash
npm run tauri build
```
