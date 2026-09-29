//! Extract Genshin Impact wish (gacha) history URL from local game cache,
//! following the same approach as biuuu/genshin-wish-export.

use regex::Regex;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GachaUrlResult {
    pub url: String,
    pub source: String,
    pub game: String,
}

fn user_profile() -> PathBuf {
    if let Ok(home) = std::env::var("USERPROFILE") {
        return PathBuf::from(home);
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn local_app_data() -> PathBuf {
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(p);
    }
    user_profile().join("AppData").join("Local")
}

fn local_low() -> PathBuf {
    user_profile().join("AppData").join("LocalLow")
}

/// Detect installed game folders (CN / Global / Cloud), prefer locale-aware order.
fn detect_game_types() -> Vec<&'static str> {
    let mut list = Vec::new();
    let low = local_low().join("miHoYo");

    if low.join("原神").join("output_log.txt").is_file() {
        list.push("原神");
    }
    if low
        .join("Genshin Impact")
        .join("output_log.txt")
        .is_file()
    {
        list.push("Genshin Impact");
    }

    // Non-CN Windows locale → prefer Global first
    let prefer_global = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .map(|l| !l.to_lowercase().starts_with("zh"))
        .unwrap_or_else(|_| {
            // Fall back to UI language via GetUserDefaultUILanguage is heavy;
            // check system locale env commonly set on Windows.
            std::env::var("SYSTEM_LOCALE")
                .map(|l| !l.to_lowercase().starts_with("zh"))
                .unwrap_or(false)
        });
    if prefer_global && list.len() > 1 {
        list.reverse();
    }

    let cloud_log = local_app_data()
        .join("miHoYo")
        .join("GenshinImpactCloudGame")
        .join("config")
        .join("logs")
        .join("MiHoYoSDK.log");
    if cloud_log.is_file() {
        list.push("cloud");
    }

    list
}

fn gacha_url_re() -> Regex {
    // Same pattern as genshin-wish-export getData.js
    Regex::new(r"https.+?auth_appid=webview_gacha.+?authkey=.+?game_biz=hk4e_\w+")
        .expect("gacha url regex")
}

fn last_gacha_url(text: &str) -> Option<String> {
    let re = gacha_url_re();
    re.find_iter(text).last().map(|m| m.as_str().to_string())
}

fn extract_game_data_path(log_text: &str) -> Option<PathBuf> {
    let re = Regex::new(r"[A-Za-z]:/.+(?:GenshinImpact_Data|YuanShen_Data)")
        .expect("game path regex");
    re.find(log_text).map(|m| {
        // Log uses forward slashes; normalize for Windows
        PathBuf::from(m.as_str().replace('/', "\\"))
    })
}

fn file_mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Find newest `webCaches{/,/*/}Cache/Cache_Data/data_2` under game data dir.
fn find_newest_data2(game_data: &Path) -> Option<PathBuf> {
    let web_caches = game_data.join("webCaches");
    if !web_caches.is_dir() {
        return None;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    // webCaches/Cache/Cache_Data/data_2
    let direct = web_caches
        .join("Cache")
        .join("Cache_Data")
        .join("data_2");
    if direct.is_file() {
        candidates.push(direct);
    }

    // webCaches/<version>/Cache/Cache_Data/data_2
    if let Ok(entries) = fs::read_dir(&web_caches) {
        for entry in entries.flatten() {
            let path = entry
                .path()
                .join("Cache")
                .join("Cache_Data")
                .join("data_2");
            if path.is_file() {
                candidates.push(path);
            }
        }
    }

    candidates.into_iter().max_by_key(|p| file_mtime(p))
}

fn read_lossy(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("读取失败 {}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_from_pc_log(game_name: &str) -> Result<Option<GachaUrlResult>, String> {
    let log_path = local_low()
        .join("miHoYo")
        .join(game_name)
        .join("output_log.txt");
    if !log_path.is_file() {
        return Ok(None);
    }

    let log_text = read_lossy(&log_path)?;
    let Some(game_data) = extract_game_data_path(&log_text) else {
        return Ok(None);
    };
    if !game_data.is_dir() {
        return Err(format!(
            "日志中的游戏目录不存在: {}",
            game_data.display()
        ));
    }

    let Some(data2) = find_newest_data2(&game_data) else {
        return Err(format!(
            "未找到 webCaches 缓存（请先在游戏内打开祈愿历史记录）: {}",
            game_data.display()
        ));
    };

    let cache_text = read_lossy(&data2)?;
    let Some(url) = last_gacha_url(&cache_text) else {
        return Ok(None);
    };

    Ok(Some(GachaUrlResult {
        url,
        source: data2.to_string_lossy().to_string(),
        game: game_name.to_string(),
    }))
}

fn read_from_cloud() -> Result<Option<GachaUrlResult>, String> {
    let log_path = local_app_data()
        .join("miHoYo")
        .join("GenshinImpactCloudGame")
        .join("config")
        .join("logs")
        .join("MiHoYoSDK.log");
    if !log_path.is_file() {
        return Ok(None);
    }

    let text = read_lossy(&log_path)?;
    let Some(url) = last_gacha_url(&text) else {
        return Ok(None);
    };

    Ok(Some(GachaUrlResult {
        url,
        source: log_path.to_string_lossy().to_string(),
        game: "云原神".into(),
    }))
}

/// Locate the most recent wish-history URL from local Genshin logs / web cache.
pub fn find_gacha_url() -> Result<GachaUrlResult, String> {
    let games = detect_game_types();
    if games.is_empty() {
        return Err(
            "未找到原神日志。请确认已安装并运行过游戏（原神 / Genshin Impact / 云原神）。"
                .into(),
        );
    }

    let mut last_err: Option<String> = None;
    for name in games {
        let result = if name == "cloud" {
            read_from_cloud()
        } else {
            read_from_pc_log(name)
        };

        match result {
            Ok(Some(found)) => return Ok(found),
            Ok(None) => {}
            Err(e) => last_err = Some(e),
        }
    }

    if let Some(e) = last_err {
        return Err(e);
    }

    Err(
        "未找到抽卡地址。请先在游戏内打开祈愿 → 历史记录，等待页面加载完成后再试。".into(),
    )
}
