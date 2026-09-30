mod blockfile;
mod gacha;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use blockfile::{
    merge_shards_to_file, parse_cache_urls, parse_video_groups, shard_owner_map, CacheUrl,
    VideoGroup,
};
use gacha::GachaUrlResult;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub kind: String,
    pub extension: String,
    pub exportable: bool,
    pub likely_wallpaper: bool,
    pub note: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub directory: String,
    pub total: usize,
    pub exportable: usize,
    pub wallpapers: usize,
    pub videos: usize,
    pub urls: usize,
    pub entries: Vec<CacheEntry>,
    pub video_groups: Vec<VideoGroup>,
    pub cache_urls: Vec<CacheUrl>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub exported: usize,
    pub skipped: usize,
    pub destination: String,
    pub files: Vec<String>,
}

fn detect_kind(bytes: &[u8]) -> (&'static str, &'static str, bool, String) {
    if bytes.len() < 12 {
        return (
            "unknown",
            "",
            false,
            "文件过小，无法识别".into(),
        );
    }

    // JPEG
    if bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return ("JPEG", "jpg", true, String::new());
    }
    // PNG
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return ("PNG", "png", true, String::new());
    }
    // GIF
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return ("GIF", "gif", true, String::new());
    }
    // WEBP
    if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return ("WEBP", "webp", true, String::new());
    }
    // MP4 / MOV (ftyp)
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        return ("MP4", "mp4", true, String::new());
    }
    // WEBM / Matroska
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return ("WEBM", "webm", true, String::new());
    }

    // Chromium often stores video as exact 1MiB chunks without headers
    if bytes.len() == 1_048_576 {
        return (
            "chunk",
            "",
            false,
            "疑似视频缓存分片（1MB），无法单独播放".into(),
        );
    }

    (
        "unknown",
        "",
        false,
        "无法识别的二进制缓存".into(),
    )
}

fn read_header(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; max];
    let n = file.read(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(n);
    Ok(buf)
}

fn classify_entry(path: &Path) -> Result<CacheEntry, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let size = meta.len();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let header = read_header(path, 64)?;
    let (kind, extension, exportable, note) = detect_kind(&header);

    // Exact 1MiB without media header → chunk
    let (kind, extension, exportable, note) = if size == 1_048_576 && !exportable {
        (
            "chunk",
            "",
            false,
            "疑似视频缓存分片（1MB），无法单独播放".into(),
        )
    } else {
        (kind, extension, exportable, note)
    };

    let likely_wallpaper = exportable
        && matches!(kind, "WEBP" | "JPEG" | "PNG")
        && size >= 500_000;

    Ok(CacheEntry {
        name,
        path: path.to_string_lossy().to_string(),
        size,
        kind: kind.to_string(),
        extension: extension.to_string(),
        exportable,
        likely_wallpaper,
        note,
    })
}

fn path_name_eq(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

/// True if `dir` looks like a Chromium/HYP `Cache_Data` folder.
fn looks_like_cache_data(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    if path_name_eq(dir, "Cache_Data") {
        return true;
    }
    dir.join("index").is_file()
        || dir.join("data_0").is_file()
        || dir.join("data_1").is_file()
}

fn join_parts(base: &Path, parts: &[&str]) -> PathBuf {
    let mut p = base.to_path_buf();
    for part in parts {
        p.push(part);
    }
    p
}

/// Shallow BFS for a `Cache_Data` directory under `root` (depth-limited).
fn find_cache_data_under(root: &Path, max_depth: usize) -> Option<PathBuf> {
    use std::collections::VecDeque;
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if looks_like_cache_data(&dir) && path_name_eq(&dir, "Cache_Data") {
            return Some(dir);
        }
        if depth >= max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                queue.push_back((path, depth + 1));
            }
        }
    }
    None
}

fn appdata_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| {
        format!(
            "{}\\AppData\\Roaming",
            std::env::var("USERPROFILE").unwrap_or_default()
        )
    });
    PathBuf::from(appdata)
}

fn default_hyp_cache_path_buf() -> PathBuf {
    appdata_dir()
        .join("miHoYo")
        .join("HYP")
        .join("1_1")
        .join("fedata")
        .join("Cache")
        .join("Cache_Data")
}

/// Kuro / 鸣潮 launcher WebView2 disk cache.
fn default_kr_cache_path_buf() -> PathBuf {
    let base = appdata_dir().join("KRLauncher");
    let known = join_parts(
        &base,
        &[
            "G152",
            "C10003",
            "KRWebViewUserData",
            "EBWebView",
            "Default",
            "Cache",
            "Cache_Data",
        ],
    );
    if looks_like_cache_data(&known) {
        return known;
    }
    // G*/C* layout may differ; search under KRLauncher
    if base.is_dir() {
        if let Some(found) = find_cache_data_under(&base, 8) {
            return found;
        }
    }
    known
}

const CACHE_DATA_SUFFIXES: &[&[&str]] = &[
    // miHoYo HYP
    &["fedata", "Cache", "Cache_Data"],
    &["Cache", "Cache_Data"],
    &["Cache_Data"],
    // Kuro KRLauncher WebView2
    &[
        "KRWebViewUserData",
        "EBWebView",
        "Default",
        "Cache",
        "Cache_Data",
    ],
    &["EBWebView", "Default", "Cache", "Cache_Data"],
    &["Default", "Cache", "Cache_Data"],
];

fn try_cache_suffixes(base: &Path) -> Option<PathBuf> {
    for parts in CACHE_DATA_SUFFIXES {
        let candidate = join_parts(base, parts);
        if looks_like_cache_data(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// From a launcher root / channel folder, try nested version layouts.
fn try_nested_launcher_layouts(base: &Path) -> Option<PathBuf> {
    let Ok(entries) = fs::read_dir(base) else {
        return None;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    children.sort_by(|a, b| b.cmp(a));

    for child in &children {
        if let Some(found) = try_cache_suffixes(child) {
            return Some(found);
        }
        // HYP: version → fedata/Cache/Cache_Data
        let hyp = join_parts(child, &["fedata", "Cache", "Cache_Data"]);
        if looks_like_cache_data(&hyp) {
            return Some(hyp);
        }
    }

    // KRLauncher: G*/C*/KRWebViewUserData/...
    for game in &children {
        let Ok(channels) = fs::read_dir(game) else {
            continue;
        };
        for channel in channels.flatten() {
            let channel_path = channel.path();
            if !channel_path.is_dir() {
                continue;
            }
            if let Some(found) = try_cache_suffixes(&channel_path) {
                return Some(found);
            }
        }
    }
    None
}

fn try_resolve_from_base(base: &Path) -> Option<PathBuf> {
    if looks_like_cache_data(base) {
        return Some(base.to_path_buf());
    }
    if let Some(found) = try_cache_suffixes(base) {
        return Some(found);
    }
    if let Some(found) = try_nested_launcher_layouts(base) {
        return Some(found);
    }
    None
}

/// Resolve a user-picked / pasted launcher path to the real `Cache_Data`.
/// Accepts HYP / KRLauncher trees, including side folders like `...\C10003\log`.
fn resolve_cache_data_dir(input: &Path) -> Result<PathBuf, String> {
    if !input.exists() {
        return Err(format!("目录不存在: {}", input.display()));
    }
    if !input.is_dir() {
        return Err(format!("不是目录: {}", input.display()));
    }

    // Walk upward so pasting `...\C10003\log` / `...\fedata\...` still works.
    let mut cur = input.to_path_buf();
    for _ in 0..12 {
        if let Some(found) = try_resolve_from_base(&cur) {
            return Ok(found);
        }
        // Shallow search under this ancestor (covers odd nesting)
        let depth = if path_name_eq(&cur, "KRLauncher") || path_name_eq(&cur, "HYP") {
            8
        } else {
            3
        };
        if let Some(found) = find_cache_data_under(&cur, depth) {
            return Ok(found);
        }
        match cur.parent() {
            Some(parent) if parent != cur.as_path() => cur = parent.to_path_buf(),
            _ => break,
        }
    }

    Err(format!(
        "未找到 Cache_Data。可粘贴 HYP\\1_1、KRLauncher\\G152\\C10003、log 旁路目录，或完整 Cache_Data 路径：{}",
        input.display()
    ))
}

/// `source`: `"hyp"` (default) or `"kr"` / `"wuthering"` / `"鸣潮"`.
#[tauri::command]
fn default_cache_path(source: Option<String>) -> String {
    let key = source
        .as_deref()
        .unwrap_or("hyp")
        .trim()
        .to_ascii_lowercase();
    let path = match key.as_str() {
        "kr" | "kuro" | "wuthering" | "ww" | "鸣潮" => default_kr_cache_path_buf(),
        _ => default_hyp_cache_path_buf(),
    };
    path.to_string_lossy().to_string()
}

#[tauri::command]
fn resolve_cache_path(directory: String) -> Result<String, String> {
    let trimmed = directory.trim().trim_matches('"').trim_matches('\'');
    resolve_cache_data_dir(Path::new(trimmed)).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn scan_cache(directory: String) -> Result<ScanResult, String> {
    let trimmed = directory.trim().trim_matches('"').trim_matches('\'');
    let dir = resolve_cache_data_dir(Path::new(trimmed))?;
    let directory = dir.to_string_lossy().to_string();

    let video_groups = parse_video_groups(&dir)?;
    let cache_urls = parse_cache_urls(&dir)?;
    let owners = shard_owner_map(&video_groups);

    let mut entries = Vec::new();
    for item in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        // Skip Chromium block/index files
        if name == "index" || name.starts_with("data_") {
            continue;
        }
        if !(name.starts_with("f_") || name.starts_with("F_")) {
            // still try classify other files without extension
            if path.extension().is_some() {
                continue;
            }
        }
        match classify_entry(&path) {
            Ok(mut entry) => {
                let key = entry.path.clone();
                if let Some(url) = owners.get(&key) {
                    let short = url.rsplit('/').next().unwrap_or(url.as_str());
                    if entry.kind == "chunk" || !entry.exportable {
                        entry.kind = "chunk".into();
                        entry.exportable = false;
                        entry.note = format!("视频分片 → {short}");
                    } else if entry.note.is_empty() {
                        entry.note = format!("属于视频 {short}");
                    }
                }
                entries.push(entry);
            }
            Err(_) => continue,
        }
    }

    entries.sort_by(|a, b| b.size.cmp(&a.size));

    let exportable = entries.iter().filter(|e| e.exportable).count();
    let wallpapers = entries.iter().filter(|e| e.likely_wallpaper).count();
    let videos = video_groups.len();
    let urls = cache_urls.len();
    let total = entries.len();

    Ok(ScanResult {
        directory,
        total,
        exportable,
        wallpapers,
        videos,
        urls,
        entries,
        video_groups,
        cache_urls,
    })
}

#[tauri::command]
fn preview_data_url(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let meta = fs::metadata(&p).map_err(|e| e.to_string())?;
    // Cap preview at 12MB to avoid blowing up the webview
    if meta.len() > 12 * 1024 * 1024 {
        return Err("文件过大，无法预览（>12MB）".into());
    }
    let bytes = fs::read(&p).map_err(|e| e.to_string())?;
    let (kind, ext, exportable, _) = detect_kind(&bytes);
    if !exportable {
        return Err(format!("无法预览: {}", kind));
    }
    let mime = match ext {
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    };
    Ok(format!("data:{};base64,{}", mime, STANDARD.encode(bytes)))
}

fn convert_webp_file_to_png(src: &Path, dest: &Path) -> Result<(), String> {
    let bytes = fs::read(src).map_err(|e| format!("读取失败: {e}"))?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("解码 WebP 失败: {e}"))?;
    img.save_with_format(dest, image::ImageFormat::Png)
        .map_err(|e| format!("写入 PNG 失败: {e}"))
}

fn find_ffmpeg() -> Option<PathBuf> {
    for candidate in bundled_ffmpeg_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    which_ffmpeg("ffmpeg").or_else(|| which_ffmpeg("ffmpeg.exe"))
}

fn bundled_ffmpeg_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("ffmpeg.exe"));
            out.push(dir.join("ffmpeg"));
            // NSIS / some install layouts keep extras one level up or in resources
            out.push(dir.join("resources").join("ffmpeg.exe"));
            if let Some(parent) = dir.parent() {
                out.push(parent.join("ffmpeg.exe"));
                out.push(parent.join("resources").join("ffmpeg.exe"));
            }
        }
    }

    // Dev / packaging folder: src-tauri/binaries
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    out.push(manifest.join("ffmpeg.exe"));
    out.push(manifest.join("ffmpeg-x86_64-pc-windows-msvc.exe"));

    out
}

fn which_ffmpeg(name: &str) -> Option<PathBuf> {
    let Ok(path_env) = std::env::var("PATH") else {
        return None;
    };
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn ffmpeg_missing_msg() -> String {
    "未找到 ffmpeg。正式版安装包应已内置；开发环境请先运行 npm run fetch-ffmpeg。".into()
}

fn convert_webm_to_mp4(src: &Path, dest: &Path) -> Result<(), String> {
    let ffmpeg = find_ffmpeg().ok_or_else(ffmpeg_missing_msg)?;
    let status = std::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-i",
            &src.to_string_lossy(),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
            &dest.to_string_lossy(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("启动 ffmpeg 失败: {e}"))?;
    if !status.success() {
        return Err("ffmpeg 转换 MP4 失败".into());
    }
    Ok(())
}

/// `convert_to`: `None` / `"original"` keep source format; `"png"` / `"mp4"` convert.
#[tauri::command]
fn export_entries(
    paths: Vec<String>,
    destination: String,
    only_wallpapers: bool,
    convert_to: Option<String>,
) -> Result<ExportResult, String> {
    let dest = PathBuf::from(&destination);
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let convert = convert_to
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s != "original");

    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let mut exported = 0usize;
    let mut skipped = 0usize;
    let mut files = Vec::new();

    for path_str in paths {
        let src = PathBuf::from(&path_str);
        let entry = match classify_entry(&src) {
            Ok(e) => e,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        if !entry.exportable {
            skipped += 1;
            continue;
        }
        if only_wallpapers && !entry.likely_wallpaper {
            skipped += 1;
            continue;
        }

        let result = match convert.as_deref() {
            Some("png") => {
                if entry.extension != "webp" {
                    skipped += 1;
                    continue;
                }
                let out_name = format!("{}_{}.png", entry.name, stamp);
                let out_path = dest.join(&out_name);
                convert_webp_file_to_png(&src, &out_path).map(|_| out_path)
            }
            Some("mp4") => {
                if entry.extension != "webm" {
                    skipped += 1;
                    continue;
                }
                let out_name = format!("{}_{}.mp4", entry.name, stamp);
                let out_path = dest.join(&out_name);
                convert_webm_to_mp4(&src, &out_path).map(|_| out_path)
            }
            Some(other) => {
                return Err(format!("不支持的导出格式: {other}"));
            }
            None => {
                let out_name = format!("{}_{}.{}", entry.name, stamp, entry.extension);
                let out_path = dest.join(&out_name);
                fs::copy(&src, &out_path)
                    .map(|_| out_path)
                    .map_err(|e| e.to_string())
            }
        };

        match result {
            Ok(out_path) => {
                exported += 1;
                files.push(out_path.to_string_lossy().to_string());
            }
            Err(_) => skipped += 1,
        }
    }

    Ok(ExportResult {
        exported,
        skipped,
        destination,
        files,
    })
}

#[tauri::command]
fn export_videos(
    directory: String,
    urls: Vec<String>,
    destination: String,
    convert_to: Option<String>,
) -> Result<ExportResult, String> {
    let dir = resolve_cache_data_dir(Path::new(&directory))?;
    let dest = PathBuf::from(&destination);
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let convert = convert_to
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty() && s != "original");
    if let Some(fmt) = convert.as_deref() {
        if fmt != "mp4" {
            return Err(format!("视频不支持导出为 {fmt}"));
        }
        if find_ffmpeg().is_none() {
            return Err(ffmpeg_missing_msg());
        }
    }

    let groups = parse_video_groups(&dir)?;
    let wanted: HashSet<&str> = urls.iter().map(|s| s.as_str()).collect();
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");

    let mut exported = 0usize;
    let mut skipped = 0usize;
    let mut files = Vec::new();

    for group in groups {
        if !wanted.is_empty() && !wanted.contains(group.url.as_str()) {
            continue;
        }
        if !group.exportable || group.shard_paths.is_empty() {
            skipped += 1;
            continue;
        }

        let stem = group
            .name
            .trim_end_matches(&format!(".{}", group.extension));

        if convert.as_deref() == Some("mp4") {
            if group.extension != "webm" {
                skipped += 1;
                continue;
            }
            let temp_name = format!("{stem}_{stamp}_tmp.webm");
            let out_name = format!("{stem}_{stamp}.mp4");
            let out_path = dest.join(&out_name);
            match merge_shards_to_file(&group.shard_paths, &dest, &temp_name) {
                Ok(temp_path) => {
                    let converted = convert_webm_to_mp4(&temp_path, &out_path);
                    let _ = fs::remove_file(&temp_path);
                    match converted {
                        Ok(()) => {
                            exported += 1;
                            files.push(out_path.to_string_lossy().to_string());
                        }
                        Err(_) => skipped += 1,
                    }
                }
                Err(_) => skipped += 1,
            }
            continue;
        }

        let out_name = format!("{stem}_{stamp}.{}", group.extension);
        match merge_shards_to_file(&group.shard_paths, &dest, &out_name) {
            Ok(path) => {
                exported += 1;
                files.push(path.to_string_lossy().to_string());
            }
            Err(_) => skipped += 1,
        }
    }

    if wanted.is_empty() && exported == 0 && skipped == 0 {
        return Err("没有可导出的完整视频".into());
    }

    Ok(ExportResult {
        exported,
        skipped,
        destination,
        files,
    })
}

#[tauri::command]
fn open_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("仅支持 Windows 打开资源管理器".into())
    }
}

#[tauri::command]
fn get_gacha_url() -> Result<GachaUrlResult, String> {
    gacha::find_gacha_url()
}

#[tauri::command]
fn get_star_rail_gacha_url() -> Result<GachaUrlResult, String> {
    gacha::find_star_rail_gacha_url()
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn resolve_kr_log_side_folder() {
        let log = appdata_dir()
            .join("KRLauncher")
            .join("G152")
            .join("C10003")
            .join("log");
        if !log.is_dir() {
            return;
        }
        let resolved = resolve_cache_data_dir(&log).expect("resolve from log");
        assert!(
            path_name_eq(&resolved, "Cache_Data"),
            "expected Cache_Data, got {}",
            resolved.display()
        );
        assert!(
            resolved.to_string_lossy().contains("KRWebViewUserData"),
            "expected WebView cache path"
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_icon(tauri::include_image!("icons/128x128.png"));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            default_cache_path,
            resolve_cache_path,
            scan_cache,
            preview_data_url,
            export_entries,
            export_videos,
            open_in_explorer,
            get_gacha_url,
            get_star_rail_gacha_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
