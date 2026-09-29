# HYP Wallpaper Tool

Tauri + Vue 工具：扫描马哈鱼启动器缓存，识别壁纸/图片并导出；解析 `data_*` 中的 URL 与 Range 分片并合并还原视频。

## 功能

- 默认打开 `%APPDATA%\miHoYo\HYP\1_1\fedata\Cache\Cache_Data`
- 按文件头识别 JPEG / PNG / WEBP / GIF / WEBM / MP4
- **壁纸**：完整图片且 ≥ 500KB
- **图片**：小于 500KB 的图片（单独分组展示）
- **地址**：解析 `data_1` 中的缓存 URL，并提取路径里的日期（`/YYYY/MM/DD/`）
- 预览、多选、导出到指定目录（自动补后缀）
- 解析 Range 分片并在「视频」页合并导出完整 WEBM / MP4

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
