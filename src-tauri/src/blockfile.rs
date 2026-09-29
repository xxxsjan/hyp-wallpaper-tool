//! Chromium blockfile cache parser (index + data_0..3 + f_*).
//! Used to recover Range_ video shards and map them back to source URLs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const BLOCK_HEADER_SIZE: usize = 8192;
const ENTRY_STORE_SIZE: usize = 256;
const KEY_INLINE_OFFSET: usize = 96;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoGroup {
    pub id: String,
    pub url: String,
    pub name: String,
    pub extension: String,
    pub kind: String,
    pub total_size: u64,
    pub shard_count: usize,
    pub exportable: bool,
    pub note: String,
    /// Present only when every shard shares the same calendar day (local).
    pub shard_date: Option<String>,
    pub shard_paths: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUrl {
    pub url: String,
    pub host: String,
    /// Date embedded in the URL path as `/YYYY/MM/DD/`, if any.
    pub date: Option<String>,
    pub is_range: bool,
    /// "image" or "video"
    pub kind: String,
    pub extension: String,
}

#[derive(Debug)]
enum CacheAddr {
    External {
        file_number: u32,
    },
    #[allow(dead_code)]
    Block {
        file_type: u32,
        block_number: u32,
        num_blocks: u32,
    },
}

fn decode_addr(addr: u32) -> Option<CacheAddr> {
    if addr & 0x8000_0000 == 0 {
        return None;
    }
    let file_type = (addr & 0x7000_0000) >> 28;
    if file_type == 0 {
        Some(CacheAddr::External {
            file_number: addr & 0x0FFF_FFFF,
        })
    } else {
        let num_blocks = ((addr & 0x0300_0000) >> 24) + 1;
        let block_number = addr & 0xFFFF;
        Some(CacheAddr::Block {
            file_type,
            block_number,
            num_blocks,
        })
    }
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i32(buf: &[u8], off: usize) -> Option<i32> {
    read_u32(buf, off).map(|v| v as i32)
}

fn extract_url_from_key(key: &str) -> Option<String> {
    let rest = key
        .strip_prefix("Range_")
        .or_else(|| key.strip_prefix("range_"))
        .unwrap_or(key);

    // Keys look like: 1/0/https://host/.../file.webm
    let url = if let Some(idx) = rest.find("https://") {
        &rest[idx..]
    } else if let Some(idx) = rest.find("http://") {
        &rest[idx..]
    } else {
        return None;
    };

    let url = url
        .split(|c: char| c.is_whitespace() || c == '\0')
        .next()
        .unwrap_or(url);

    // Prefer cutting at known media extension
    for ext in [".webm", ".mp4", ".mov", ".mkv"] {
        if let Some(i) = url.to_ascii_lowercase().find(ext) {
            return Some(url[..i + ext.len()].to_string());
        }
    }

    // Fallback: trim query/hash junk without extension
    let cleaned = url
        .split(['?', '#', '"', '\'', '<', '>'])
        .next()
        .unwrap_or(url);
    if cleaned.starts_with("http") {
        Some(cleaned.to_string())
    } else {
        None
    }
}

/// Keep query string; stop at whitespace / quotes / angle brackets.
fn extract_full_url(raw: &str) -> Option<String> {
    let start = if let Some(i) = raw.find("https://") {
        i
    } else if let Some(i) = raw.find("http://") {
        i
    } else {
        return None;
    };
    let slice = &raw[start..];
    let end = slice
        .find(|c: char| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '{' | '}')
        })
        .unwrap_or(slice.len());
    let url = slice[..end].trim_end_matches(['.', ',', ';', ':', '\\']);
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_string())
    } else {
        None
    }
}

fn extract_date_from_url(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    for i in 0..parts.len().saturating_sub(2) {
        let y = parts[i];
        let m = parts[i + 1];
        let d = parts[i + 2];
        if y.len() != 4 || m.len() != 2 || d.len() != 2 {
            continue;
        }
        if !y.chars().all(|c| c.is_ascii_digit())
            || !m.chars().all(|c| c.is_ascii_digit())
            || !d.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let year: u32 = y.parse().ok()?;
        let month: u32 = m.parse().ok()?;
        let day: u32 = d.parse().ok()?;
        if (2000..=2100).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
        {
            return Some(format!("{year:04}-{month:02}-{day:02}"));
        }
    }
    None
}

fn host_from_url(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_string()
}

/// Returns (kind, extension) for media URLs only.
fn media_meta_from_url(url: &str) -> Option<(&'static str, &'static str)> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    const IMAGES: &[(&str, &str)] = &[
        (".jpg", "jpg"),
        (".jpeg", "jpeg"),
        (".png", "png"),
        (".webp", "webp"),
        (".gif", "gif"),
        (".bmp", "bmp"),
        (".avif", "avif"),
    ];
    const VIDEOS: &[(&str, &str)] = &[
        (".webm", "webm"),
        (".mp4", "mp4"),
        (".mov", "mov"),
        (".mkv", "mkv"),
    ];
    for (suffix, ext) in IMAGES {
        if path.ends_with(suffix) {
            return Some(("image", *ext));
        }
    }
    for (suffix, ext) in VIDEOS {
        if path.ends_with(suffix) {
            return Some(("video", *ext));
        }
    }
    None
}

fn collect_urls_from_bytes(data: &[u8], out: &mut BTreeMap<String, bool>) {
    let mut i = 0;
    while i + 8 < data.len() {
        if data[i] != b'h' && data[i] != b'H' {
            i += 1;
            continue;
        }
        let remain = data.len() - i;
        if remain < 7 {
            break;
        }
        let window = &data[i..];
        let is_https = window.len() >= 8 && window[..8].eq_ignore_ascii_case(b"https://");
        let is_http =
            !is_https && window.len() >= 7 && window[..7].eq_ignore_ascii_case(b"http://");
        if !is_https && !is_http {
            i += 1;
            continue;
        }
        let mut j = i + if is_https { 8 } else { 7 };
        while j < data.len() {
            let c = data[j];
            if c < 0x20
                || c > 0x7E
                || matches!(
                    c,
                    b'"' | b'\'' | b'<' | b'>' | b')' | b']' | b'{' | b'}' | b' ' | b'\\'
                )
            {
                break;
            }
            j += 1;
        }
        if let Ok(s) = std::str::from_utf8(&data[i..j]) {
            if let Some(url) = extract_full_url(s) {
                let lower = url.to_ascii_lowercase();
                let junk = lower.contains("digicert.")
                    || lower.contains("ocsp.")
                    || lower.contains(".crl")
                    || lower.contains("cacerts.");
                if !junk {
                    let prefix = if i >= 6 { &data[i - 6..i] } else { &[] };
                    let is_range = std::str::from_utf8(prefix)
                        .map(|p| p.contains("Range") || p.contains("range"))
                        .unwrap_or(false);
                    let entry = out.entry(url).or_insert(false);
                    *entry = *entry || is_range;
                }
            }
        }
        i = j;
    }
}

/// Parse http(s) URLs out of Chromium `data_1` (cache keys).
pub fn parse_cache_urls(cache_dir: &Path) -> Result<Vec<CacheUrl>, String> {
    let data1_path = cache_dir.join("data_1");
    if !data1_path.is_file() {
        return Ok(Vec::new());
    }
    let data1 = fs::read(&data1_path).map_err(|e| e.to_string())?;

    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    collect_urls_from_bytes(&data1, &mut found);

    let mut urls: Vec<CacheUrl> = found
        .into_iter()
        .filter_map(|(url, is_range)| {
            let (kind, extension) = media_meta_from_url(&url)?;
            let date = extract_date_from_url(&url)?;
            let host = host_from_url(&url);
            Some(CacheUrl {
                url,
                host,
                date: Some(date),
                is_range,
                kind: kind.to_string(),
                extension: extension.to_string(),
            })
        })
        .collect();

    urls.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.url.cmp(&b.url))
    });

    Ok(urls)
}

fn media_kind_from_url(url: &str) -> Option<(&'static str, &'static str)> {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".webm") {
        Some(("WEBM", "webm"))
    } else if lower.contains(".mp4") {
        Some(("MP4", "mp4"))
    } else if lower.contains(".mov") {
        Some(("MP4", "mp4"))
    } else {
        None
    }
}

fn filename_from_url(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("video")
        .split(['?', '#'])
        .next()
        .unwrap_or("video")
        .to_string()
}

fn external_path(dir: &Path, file_number: u32) -> PathBuf {
    dir.join(format!("f_{file_number:06x}"))
}

fn first_bytes_are_media(path: &Path) -> bool {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 12];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n < 8 {
        return false;
    }
    // WEBM
    if buf.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return true;
    }
    // MP4/MOV ftyp
    if n >= 8 && &buf[4..8] == b"ftyp" {
        return true;
    }
    false
}

/// Parse `data_1` EntryStores and group Range_ video shards by source URL.
pub fn parse_video_groups(cache_dir: &Path) -> Result<Vec<VideoGroup>, String> {
    let data1_path = cache_dir.join("data_1");
    if !data1_path.is_file() {
        return Ok(Vec::new());
    }

    let data1 = fs::read(&data1_path).map_err(|e| e.to_string())?;
    if data1.len() < BLOCK_HEADER_SIZE + ENTRY_STORE_SIZE {
        return Ok(Vec::new());
    }

    // url -> (file_number -> (path, size, name, modified))
    let mut by_url: BTreeMap<String, BTreeMap<u32, (PathBuf, u64, String, Option<SystemTime>)>> =
        BTreeMap::new();

    let mut offset = BLOCK_HEADER_SIZE;
    while offset + ENTRY_STORE_SIZE <= data1.len() {
        let entry = &data1[offset..offset + ENTRY_STORE_SIZE];
        offset += ENTRY_STORE_SIZE;

        let key_len = match read_i32(entry, 32) {
            Some(n) if (5..=2048).contains(&n) => n as usize,
            _ => continue,
        };

        let inline_len = (ENTRY_STORE_SIZE - KEY_INLINE_OFFSET).min(key_len);
        if entry[KEY_INLINE_OFFSET] < 0x20 || entry[KEY_INLINE_OFFSET] > 0x7E {
            continue;
        }

        let key_bytes = &entry[KEY_INLINE_OFFSET..KEY_INLINE_OFFSET + inline_len];
        let end = key_bytes.iter().position(|&b| b == 0).unwrap_or(key_bytes.len());
        let key = match std::str::from_utf8(&key_bytes[..end]) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Only Range_ shards carry the split bodies we care about.
        if !key.starts_with("Range_") && !key.starts_with("range_") {
            continue;
        }

        let Some(url) = extract_url_from_key(key) else {
            continue;
        };
        let Some(_) = media_kind_from_url(&url) else {
            continue;
        };

        // stream1 = response body
        let body_addr = match read_u32(entry, 56 + 4) {
            Some(a) => a,
            None => continue,
        };
        let Some(CacheAddr::External { file_number }) = decode_addr(body_addr) else {
            continue;
        };

        let path = external_path(cache_dir, file_number);
        let meta = match fs::metadata(&path) {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let modified = meta.modified().ok();

        by_url
            .entry(url)
            .or_default()
            .insert(file_number, (path, meta.len(), name, modified));
    }

    let mut groups = Vec::new();
    for (url, shards_map) in by_url {
        let Some((kind, extension)) = media_kind_from_url(&url) else {
            continue;
        };
        if shards_map.is_empty() {
            continue;
        }

        let mut shard_paths = Vec::new();
        let mut total_size = 0u64;
        let mut first_path: Option<PathBuf> = None;
        let mut day_set: BTreeSet<String> = BTreeSet::new();

        for (_num, (path, size, _name, modified)) in shards_map.iter() {
            if first_path.is_none() {
                first_path = Some(path.clone());
            }
            total_size += size;
            shard_paths.push(path.to_string_lossy().to_string());
            if let Some(mtime) = modified {
                let dt: chrono::DateTime<chrono::Local> = (*mtime).into();
                day_set.insert(dt.format("%Y-%m-%d").to_string());
            }
        }

        let exportable = first_path
            .as_ref()
            .map(|p| first_bytes_are_media(p))
            .unwrap_or(false);

        let name = filename_from_url(&url);
        let note = if exportable {
            format!(
                "由 {} 个 Range 分片拼接，可导出完整视频",
                shard_paths.len()
            )
        } else {
            format!(
                "找到 {} 个分片，但缺少文件头，可能不完整",
                shard_paths.len()
            )
        };

        let shard_date = if day_set.len() == 1 {
            day_set.into_iter().next()
        } else {
            None
        };

        groups.push(VideoGroup {
            id: url.clone(),
            url: url.clone(),
            name,
            extension: extension.to_string(),
            kind: kind.to_string(),
            total_size,
            shard_count: shard_paths.len(),
            exportable,
            note,
            shard_date,
            shard_paths,
        });
    }

    groups.sort_by(|a, b| b.total_size.cmp(&a.total_size));
    Ok(groups)
}

/// Build a quick lookup: shard file path → parent video URL.
pub fn shard_owner_map(groups: &[VideoGroup]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for g in groups {
        for p in &g.shard_paths {
            map.insert(p.clone(), g.url.clone());
        }
    }
    map
}

/// Concatenate shard files in order into `destination/name`.
pub fn merge_shards_to_file(
    shard_paths: &[String],
    destination: &Path,
    file_name: &str,
) -> Result<PathBuf, String> {
    if shard_paths.is_empty() {
        return Err("没有可合并的分片".into());
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let out_path = destination.join(file_name);
    let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1024 * 256];

    for path in shard_paths {
        let mut input = fs::File::open(path).map_err(|e| format!("打开分片失败 {path}: {e}"))?;
        loop {
            let n = input.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hyp_cache_urls() {
        let dir = PathBuf::from(
            r"c:\Users\admin\AppData\Roaming\miHoYo\HYP\1_1\fedata\Cache\Cache_Data",
        );
        if !dir.is_dir() {
            return;
        }
        let urls = parse_cache_urls(&dir).expect("parse urls");
        assert!(!urls.is_empty());
        assert!(
            urls.iter().all(|u| u.kind == "image" || u.kind == "video"),
            "only media URLs should be listed"
        );
        assert!(
            urls.iter().any(|u| u.date.is_some()),
            "expected some URLs to contain /YYYY/MM/DD/"
        );
        assert!(
            urls.iter().any(|u| u.url.contains("launcher-webstatic")),
            "expected launcher asset URLs"
        );
    }

    #[test]
    fn parse_hyp_cache_videos() {
        let dir = PathBuf::from(
            r"c:\Users\admin\AppData\Roaming\miHoYo\HYP\1_1\fedata\Cache\Cache_Data",
        );
        if !dir.is_dir() {
            return;
        }
        let groups = parse_video_groups(&dir).expect("parse");
        assert!(
            !groups.is_empty(),
            "expected at least one video group from Range_ entries"
        );
        assert!(
            groups.iter().any(|g| g.exportable && g.shard_count > 1),
            "expected a mergeable multi-shard video"
        );

        let g = groups.iter().find(|g| g.exportable).unwrap();
        let tmp = std::env::temp_dir().join("hyp-wallpaper-tool-test");
        let out = merge_shards_to_file(&g.shard_paths, &tmp, "test_merge.webm").unwrap();
        assert!(first_bytes_are_media(&out));
        let _ = fs::remove_file(&out);
    }
}
