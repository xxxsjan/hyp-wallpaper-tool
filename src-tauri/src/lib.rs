use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
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
    pub entries: Vec<CacheEntry>,
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

#[tauri::command]
fn default_cache_path() -> String {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| {
        format!(
            "{}\\AppData\\Roaming",
            std::env::var("USERPROFILE").unwrap_or_default()
        )
    });
    PathBuf::from(appdata)
        .join("miHoYo")
        .join("HYP")
        .join("1_1")
        .join("fedata")
        .join("Cache")
        .join("Cache_Data")
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
fn scan_cache(directory: String) -> Result<ScanResult, String> {
    let dir = PathBuf::from(&directory);
    if !dir.is_dir() {
        return Err(format!("目录不存在: {}", directory));
    }

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
            Ok(entry) => entries.push(entry),
            Err(_) => continue,
        }
    }

    entries.sort_by(|a, b| b.size.cmp(&a.size));

    let exportable = entries.iter().filter(|e| e.exportable).count();
    let wallpapers = entries.iter().filter(|e| e.likely_wallpaper).count();
    let total = entries.len();

    Ok(ScanResult {
        directory,
        total,
        exportable,
        wallpapers,
        entries,
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

#[tauri::command]
fn export_entries(
    paths: Vec<String>,
    destination: String,
    only_wallpapers: bool,
) -> Result<ExportResult, String> {
    let dest = PathBuf::from(&destination);
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

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

        let out_name = format!("{}_{}.{}", entry.name, stamp, entry.extension);
        let out_path = dest.join(&out_name);
        match fs::copy(&src, &out_path) {
            Ok(_) => {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            default_cache_path,
            scan_cache,
            preview_data_url,
            export_entries,
            open_in_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
