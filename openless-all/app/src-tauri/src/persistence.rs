//! Local persistence: history JSON, user preferences JSON, vocab JSON, and
//! platform-backed credentials vault.
//!
//! Storage roots:
//! - macOS:   `~/Library/Application Support/OpenLess Unbound`
//! - Windows: `%APPDATA%\OpenLess Unbound`
//! - Linux:   `$XDG_DATA_HOME/OpenLess Unbound` or `~/.local/share/OpenLess Unbound`
//!
//! Credential storage policy: provider credentials are stored in the OS
//! credential vault (macOS Keychain, Windows Credential Manager, Linux keyring).
//! A legacy plaintext JSON file is read once as a migration source and removed
//! after a successful vault write; new writes never persist plaintext secrets.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{
    builtin_style_pack_for_mode, builtin_style_pack_id, builtin_style_packs,
    default_active_style_pack_id, CorrectionRule, CustomStylePrompts, DictationSession,
    DictionaryEntry, PolishMode, StylePack, StylePackExample, StylePackKind, UserPreferences,
    VocabPresetStore, BUILTIN_STYLE_PACK_LIGHT_ID,
};

const HISTORY_CAP: usize = 200;
const HISTORY_FILE: &str = "history.json";
const PREFERENCES_FILE: &str = "preferences.json";
const STYLE_PACKS_FILE: &str = "style-packs.json";
const STYLE_PACK_ASSETS_DIR: &str = "style-pack-assets";
/// 与 Swift `Sources/OpenLessPersistence/DictionaryStore.swift` 同名，
/// 让旧版词汇表在升级后无缝继承。**不要**改成 `vocab.json`，会丢用户数据。
const VOCAB_FILE: &str = "dictionary.json";
const CORRECTION_RULES_FILE: &str = "correction-rules.json";
const CORRECTION_NUM_TOKEN: &str = "{num}";
const VOCAB_PRESETS_FILE: &str = "vocab-presets.json";

/// 旧版 plaintext JSON 凭据路径。仅作为迁移来源；成功写入系统凭据库后会删除。
const LEGACY_CREDS_DIR: &str = ".openless-unbound";
const LEGACY_CREDS_FILE: &str = "credentials.json";
const APP_DATA_DIR_NAME: &str = "OpenLess Unbound";

const KEYRING_CREDENTIALS_ACCOUNT: &str = "credentials.v1";
const KEYRING_CREDENTIALS_CHUNK_PREFIX: &str = "credentials.v1.chunk.";
// Windows Credential Manager caps one credential blob at 2560 bytes. keyring stores
// passwords as UTF-16 on Windows, so keep each JSON chunk comfortably below that.
const KEYRING_CHUNK_MAX_UTF16_UNITS: usize = 1000;

static CREDENTIALS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn credentials_lock() -> &'static Mutex<()> {
    CREDENTIALS_LOCK.get_or_init(|| Mutex::new(()))
}

/// Process-wide credentials cache.
///
/// Without this cache every `CredentialsVault::get_*` / `snapshot` call hits
/// `load_credentials()` → `load_keyring_credentials()` which reads the
/// manifest entry plus every chunk entry from the OS keyring. On macOS each
/// distinct keychain entry has its own ACL — so an ad-hoc-signed binary (or
/// any binary whose ACL grants haven't been set up yet) prompts on every read
/// of every entry. A single dictation cycle reads credentials 5–10 times,
/// times (1 manifest + N chunks) entries → tens of "OpenLess wants to use
/// the keychain" prompts per recording.
///
/// With this cache the first read populates `Some(CredsRoot)` and every
/// subsequent read in the same process is silent. `save_credentials` keeps
/// the cache in sync after writes so Settings → Recording credential edits
/// take effect immediately.
///
/// Cross-process changes (e.g. user edits via `security` CLI, or another
/// instance of the app — single-instance is enforced but defense in depth)
/// will be invisible until the next process launch. Acceptable trade-off
/// per the credential vault contract: the keyring is owned by this app.
static CREDENTIALS_CACHE: OnceLock<Mutex<Option<CredsRoot>>> = OnceLock::new();

fn credentials_cache() -> &'static Mutex<Option<CredsRoot>> {
    CREDENTIALS_CACHE.get_or_init(|| Mutex::new(None))
}

fn store_credentials_cache(root: &CredsRoot) {
    *credentials_cache().lock() = Some(root.clone());
}

#[cfg(test)]
fn reset_credentials_cache_for_tests() {
    *credentials_cache().lock() = None;
    *credentials_manifest_cache().lock() = None;
}

/// issue #602：进程内缓存「上次成功读/写的 chunk manifest」。save_credentials 用它
/// 替代保存前的 keychain manifest 读 —— macOS 上这次读本身就要过 ACL 检查（弹窗）。
/// None = 本进程还没成功读/写过 manifest（冷启动或 keyring 不可用），此时才回
/// keychain 读真实 manifest，保证 UUID-generation 旧 chunks 的清理信息不丢。
static CREDENTIALS_MANIFEST_CACHE: OnceLock<Mutex<Option<CredsChunkManifest>>> = OnceLock::new();

fn credentials_manifest_cache() -> &'static Mutex<Option<CredsChunkManifest>> {
    CREDENTIALS_MANIFEST_CACHE.get_or_init(|| Mutex::new(None))
}

// ───────────────────────── path helpers ─────────────────────────

fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join(APP_DATA_DIR_NAME))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").context("APPDATA not set")?;
        Ok(PathBuf::from(appdata).join(APP_DATA_DIR_NAME))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.is_empty() {
                return Ok(PathBuf::from(xdg).join(APP_DATA_DIR_NAME));
            }
        }
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join(APP_DATA_DIR_NAME))
    }
}

fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("create dir failed: {}", dir.display()))?;
    Ok(())
}

/// 默认模型根目录：`<data_dir>/models/`。
pub fn default_models_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("models");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// 把用户选择的父目录转成实际模型根目录。
///
/// UI 让用户选一个普通目录；OpenLess 固定在其下创建 `OpenLess/models/`，
/// 避免把多个引擎的模型文件直接散落在用户选择目录根部。
pub fn models_root_for_base_dir(base_dir: Option<&str>) -> Result<PathBuf> {
    let trimmed = base_dir.map(str::trim).filter(|value| !value.is_empty());
    let dir = match trimmed {
        Some(base) => PathBuf::from(base).join("OpenLess").join("models"),
        None => return default_models_root(),
    };
    ensure_dir(&dir)?;
    Ok(dir)
}

fn configured_models_base_dir() -> Result<Option<String>> {
    let path = data_dir()?.join(PREFERENCES_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = serde_json::from_slice::<serde_json::Value>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))?;
    Ok(value
        .get("localAsrModelsBaseDir")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

/// 当前配置下的实际模型根目录。
pub fn models_root() -> Result<PathBuf> {
    models_root_for_base_dir(configured_models_base_dir()?.as_deref())
}

/// 校验用户选择的父目录，并返回实际模型根目录。
pub fn validate_models_base_dir(base_dir: Option<&str>) -> Result<PathBuf> {
    let root = models_root_for_base_dir(base_dir)?;
    let probe = root.join(format!(".openless-write-test-{}", Uuid::new_v4().simple()));
    fs::write(&probe, b"ok").with_context(|| format!("write probe failed: {}", probe.display()))?;
    fs::remove_file(&probe).with_context(|| format!("remove probe failed: {}", probe.display()))?;
    Ok(root)
}

/// 把旧模型根目录合并迁移到新模型根目录。目标已有内容优先，不覆盖。
pub fn migrate_models_root(old_root: &Path, new_root: &Path) -> Result<()> {
    ensure_dir(new_root)?;
    if same_existing_path(old_root, new_root) || !old_root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(old_root).with_context(|| format!("read {}", old_root.display()))? {
        let entry = entry?;
        merge_move_no_overwrite(&entry.path(), &new_root.join(entry.file_name()))?;
    }
    remove_dir_if_empty(old_root)?;
    Ok(())
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn merge_move_no_overwrite(src: &Path, dest: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            ensure_dir(parent)?;
        }
        return rename_or_copy_remove(src, dest);
    }
    if src.is_dir() && dest.is_dir() {
        for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
            let entry = entry?;
            merge_move_no_overwrite(&entry.path(), &dest.join(entry.file_name()))?;
        }
        remove_dir_if_empty(src)?;
    }
    Ok(())
}

fn rename_or_copy_remove(src: &Path, dest: &Path) -> Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) if src.is_dir() => {
            copy_dir_no_overwrite(src, dest)?;
            fs::remove_dir_all(src).with_context(|| format!("remove {}", src.display()))?;
            Ok(())
        }
        Err(_) => {
            fs::copy(src, dest)
                .with_context(|| format!("copy {} to {}", src.display(), dest.display()))?;
            fs::remove_file(src).with_context(|| format!("remove {}", src.display()))?;
            Ok(())
        }
    }
}

fn copy_dir_no_overwrite(src: &Path, dest: &Path) -> Result<()> {
    ensure_dir(dest)?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if dest_path.exists() {
            continue;
        }
        if src_path.is_dir() {
            copy_dir_no_overwrite(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copy {} to {}", src_path.display(), dest_path.display())
            })?;
        }
    }
    Ok(())
}

fn remove_dir_if_empty(path: &Path) -> Result<()> {
    match fs::read_dir(path) {
        Ok(entries) => {
            if entries.count() == 0 {
                fs::remove_dir(path).with_context(|| format!("remove {}", path.display()))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 本地 ASR 模型根目录：`<models_root>/qwen3-asr/`。
/// 子目录 = 模型 id（如 `qwen3-asr-0.6b`），存 qwen-asr `download_model.sh`
/// 列出的 5–7 个文件。
pub fn local_models_root() -> Result<PathBuf> {
    let dir = models_root()?.join("qwen3-asr");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Recording archive dir: `<data_dir>/recordings/`.
/// Contains debug recordings plus failed/empty transcript recordings kept for replay/retranscribe.
/// Uses the same retention/count pruning policy.
pub fn recordings_root() -> Result<PathBuf> {
    let dir = data_dir()?.join("recordings");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// 双重 cap 清理 `recordings/*.wav`：
/// - `retention_days > 0` → 把超过 N 天的删掉（沿用 history 的 retention 逻辑）。
/// - `max_entries == Some(n)` → 按 mtime 倒序保留最新的 n 条（clamp 到 1..=HISTORY_CAP）；
///   `None` 时退回 HISTORY_CAP (200) 硬上限，避免无限增长。
/// 调用方：每次新建一条录音前。失败仅打 warn，避免影响主路径。
pub fn prune_recordings(retention_days: u32, max_entries: Option<u32>) -> Result<()> {
    let dir = match data_dir() {
        Ok(d) => d.join("recordings"),
        Err(_) => return Ok(()),
    };
    if !dir.exists() {
        return Ok(());
    }

    // 第一步：按天清理。仅扫 .wav，跟第二步保持一致；metadata 读不到的文件按"过期"处理
    // —— fs 损坏 / 未来格式不一致的孤儿文件应当被回收而不是无限累积。
    if retention_days > 0 {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(u64::from(retention_days) * 24 * 3600);
        for entry in fs::read_dir(&dir).context("read recordings dir")?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                continue;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            if modified < cutoff {
                if let Err(err) = fs::remove_file(&path) {
                    log::warn!("[recordings] prune (days) remove failed for {path:?}: {err}");
                }
            }
        }
    }

    // 第二步：按条数清理。剩下的 wav 按 mtime 倒序，超出 cap 的删掉。
    let cap = max_entries
        .map(|n| (n as usize).clamp(1, HISTORY_CAP))
        .unwrap_or(HISTORY_CAP);
    let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(&dir)
        .context("read recordings dir")?
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            // 只看 .wav，避免误删未来其他类型的归档文件。
            if path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((path, modified))
        })
        .collect();
    if entries.len() <= cap {
        return Ok(());
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in entries.into_iter().skip(cap) {
        if let Err(err) = fs::remove_file(&path) {
            log::warn!(
                "[recordings] prune (count) remove failed for {:?}: {err}",
                path
            );
        }
    }
    Ok(())
}

/// 单个 session 的录音文件路径。不保证文件已存在（DictationSession.has_audio_recording
/// 决定文件是否被写过）。前端用 `read_audio_recording` IPC 读字节流喂 HTMLAudio。
pub fn recording_path_for_session(session_id: &str) -> Result<PathBuf> {
    Ok(recordings_root()?.join(format!("{session_id}.wav")))
}

/// Foundry Local 下载与缓存根目录。DLL 和模型都不打进安装包，和 Qwen3-ASR
/// 一样放在 OpenLess 的 models 目录下，卸载清理用户数据时可以一起删除。
#[cfg(target_os = "windows")]
pub fn foundry_local_root() -> Result<PathBuf> {
    let dir = models_root()?.join("foundry-local");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_native_runtime_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("runtime");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn sherpa_onnx_models_root() -> Result<PathBuf> {
    let dir = models_root()?.join("sherpa-onnx");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_model_cache_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?;
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_app_data_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("app-data");
    ensure_dir(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
pub fn foundry_logs_root() -> Result<PathBuf> {
    let dir = foundry_local_root()?.join("logs");
    ensure_dir(&dir)?;
    Ok(dir)
}

/// Atomic write: write to a unique `*.tmp-<uuid>` first, then rename onto the
/// target path. The unique suffix lets concurrent writers each own their own
/// tmp file, so a parallel rename never finds its source already taken.
fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4().simple()));
    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tmp failed: {}", tmp_path.display()))?;
    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err).with_context(|| format!("rename failed: {}", path.display()));
    }
    Ok(())
}

fn read_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice::<T>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))
}

fn read_preferences(path: &Path) -> Result<UserPreferences> {
    if !path.exists() {
        return Ok(UserPreferences::default());
    }
    let bytes = fs::read(path).with_context(|| format!("read failed: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(UserPreferences::default());
    }
    let prefs = serde_json::from_slice::<UserPreferences>(&bytes)
        .with_context(|| format!("decode failed: {}", path.display()))?;

    // issue #440：老版本可能已把旧默认 `streamingInsert:false` 写进 preferences.json。
    // 反序列化会在内存里迁到 true，但还必须把迁移标记落盘，否则每次启动都停留在
    // “旧文件”状态，无法表达用户后续手动关闭后的 durable opt-out。
    let streaming_default_migrated = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("streamingInsertDefaultMigrated")
                .and_then(|flag| flag.as_bool())
        })
        .unwrap_or(false);
    if !streaming_default_migrated {
        match serde_json::to_vec_pretty(&prefs)
            .context("encode prefs failed")
            .and_then(|json| atomic_write(path, &json))
        {
            Ok(()) => log::info!("[prefs] migrated streamingInsert default marker"),
            Err(err) => log::warn!(
                "[prefs] failed to persist streamingInsert migration marker for {}: {}",
                path.display(),
                err
            ),
        }
    }

    Ok(prefs)
}

// ───────────────────────── credentials vault ─────────────────────────
//
// 正常读写走系统凭据库；旧 plaintext JSON 只作为迁移来源。为保持多 provider
// schema 与 active provider 状态，凭据库里保存一个 v1 JSON payload；payload 会按平台
// 凭据库限制拆成多个条目，避免 Windows 单条凭据 2560 bytes 限制。
//
// v1 schema：
//   {
//     "version": 1,
//     "active": { "asr": "<id>", "llm": "<id>" },
//     "providers": {
//       "asr": { "<id>": { "appKey", "accessKey", "resourceId", "apiKey", "baseURL", "model", "vocabularyId" } },
//       "llm": { "<id>": { "displayName", "apiKey", "baseURL", "model", "temperature", "extraHeaders" } }
//     }
//   }
//
// "ark.api_key"/"volcengine.app_key" 等账户名按 Swift 语义路由到 active provider。

use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsRoot {
    #[serde(default = "credsroot_default_version")]
    version: u32,
    #[serde(default)]
    active: CredsActive,
    #[serde(default)]
    providers: CredsProviders,
}

fn credsroot_default_version() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CredsActive {
    #[serde(default = "creds_default_asr")]
    asr: String,
    #[serde(default = "creds_default_llm")]
    llm: String,
}

impl Default for CredsActive {
    fn default() -> Self {
        Self {
            asr: creds_default_asr(),
            llm: creds_default_llm(),
        }
    }
}

fn creds_default_asr() -> String {
    #[cfg(target_os = "windows")]
    {
        return crate::asr::local::foundry::PROVIDER_ID.into();
    }
    #[cfg(not(target_os = "windows"))]
    {
        "volcengine".into()
    }
}
fn creds_default_llm() -> String {
    "ark".into()
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct CredsProviders {
    #[serde(default)]
    asr: HashMap<String, CredsAsrEntry>,
    #[serde(default)]
    llm: HashMap<String, CredsLlmEntry>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsAsrEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    apiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseURL: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    appKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accessKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resourceId: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vocabularyId: Option<String>,
}

impl CredsAsrEntry {
    fn is_empty(&self) -> bool {
        self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.appKey.as_deref().unwrap_or("").is_empty()
            && self.accessKey.as_deref().unwrap_or("").is_empty()
            && self.resourceId.as_deref().unwrap_or("").is_empty()
            && self.vocabularyId.as_deref().unwrap_or("").is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
struct CredsLlmEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    displayName: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apiKey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseURL: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extraHeaders: Option<HashMap<String, String>>,
}

impl CredsLlmEntry {
    fn is_empty(&self) -> bool {
        self.displayName.as_deref().unwrap_or("").is_empty()
            && self.apiKey.as_deref().unwrap_or("").is_empty()
            && self.baseURL.as_deref().unwrap_or("").is_empty()
            && self.model.as_deref().unwrap_or("").is_empty()
            && self.temperature.is_none()
            && self
                .extraHeaders
                .as_ref()
                .map(|h| h.is_empty())
                .unwrap_or(true)
    }
}

fn credentials_path() -> Result<PathBuf> {
    // macOS / Linux: ~/.openless/credentials.json (与 Swift 同源)
    // Windows: %APPDATA%\OpenLess Unbound\credentials.json (Windows 没有标准 HOME 环境变量)
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").context("APPDATA not set")?;
        return Ok(PathBuf::from(appdata)
            .join(APP_DATA_DIR_NAME)
            .join(LEGACY_CREDS_FILE));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(LEGACY_CREDS_DIR)
            .join(LEGACY_CREDS_FILE))
    }
}

fn keyring_entry() -> Result<keyring::Entry> {
    keyring_entry_for(KEYRING_CREDENTIALS_ACCOUNT)
}

fn keyring_entry_for(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(CredentialsVault::SERVICE_NAME, account)
        .context("open system credential vault")
}

fn clean_credentials(root: &CredsRoot) -> CredsRoot {
    let mut cleaned = root.clone();
    cleaned.providers.asr.retain(|_, v| !v.is_empty());
    cleaned.providers.llm.retain(|_, v| !v.is_empty());
    cleaned
}

fn read_legacy_credentials_file(path: &Path) -> Option<CredsRoot> {
    if !path.exists() {
        return None;
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[vault] read legacy {} failed: {}", path.display(), e);
            return None;
        }
    };
    match serde_json::from_slice::<CredsRoot>(&bytes) {
        Ok(root) => Some(root),
        Err(e) => {
            log::warn!("[vault] parse legacy {} failed: {}", path.display(), e);
            None
        }
    }
}

fn remove_legacy_credentials_file() -> Result<()> {
    let Ok(path) = credentials_path() else {
        return Ok(());
    };
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("remove legacy credentials file {}", path.display()))?;
    }
    Ok(())
}

fn remove_legacy_credentials_file_best_effort() {
    if let Err(e) = remove_legacy_credentials_file() {
        log::warn!("[vault] remove legacy credentials file failed: {e}");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CredsChunkManifest {
    openless_credentials_storage: String,
    version: u32,
    /// 旧版本（v1 早期）每次 save 都生成新 UUID 作为 chunk account 命名前缀，
    /// 这让 macOS Keychain 的「始终允许」每次保存后失效 → 反复弹 ACL 弹窗。
    /// 现在 save 总用稳定 chunk.{index} 名，此字段仅向后兼容旧 manifest 读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<String>,
    chunks: usize,
}

/// 旧版（generation=Some）：`credentials.v1.chunk.<UUID>.{index}`
/// 新版（generation=None）：`credentials.v1.chunk.{index}` —— 稳定名，ACL 长期有效
fn chunk_account(generation: Option<&str>, index: usize) -> String {
    match generation {
        Some(gen) => format!("{KEYRING_CREDENTIALS_CHUNK_PREFIX}{gen}.{index}"),
        None => format!("{KEYRING_CREDENTIALS_CHUNK_PREFIX}{index}"),
    }
}

fn chunk_json_payload(json: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_units = 0usize;
    for ch in json.chars() {
        let units = ch.len_utf16();
        if !current.is_empty() && current_units + units > KEYRING_CHUNK_MAX_UTF16_UNITS {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        current.push(ch);
        current_units += units;
    }
    if !current.is_empty() || json.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// issue #602：给定「上次成功落盘的 JSON」与「本次要写的各 chunk」，返回每个新 chunk
/// 是否可跳过重写（与同号旧 chunk 逐字节一致）。注意 chunk 按偏移切分：靠前字段的
/// 变长改动会移动后续所有 chunk 边界（全部重写，等同旧行为）；等长改动/无改动则只
/// 写真正变化的 chunk。previous_json=None（冷缓存/旧 UUID 代际）→ 全部重写。
fn chunk_skip_mask(previous_json: Option<&str>, new_chunks: &[String]) -> Vec<bool> {
    let prev_chunks = previous_json.map(chunk_json_payload);
    new_chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            prev_chunks
                .as_ref()
                .is_some_and(|prev| prev.get(index) == Some(chunk))
        })
        .collect()
}

fn read_chunk_manifest(json: &str) -> Option<CredsChunkManifest> {
    let manifest = serde_json::from_str::<CredsChunkManifest>(json).ok()?;
    if manifest.openless_credentials_storage == "chunked" && manifest.version == 1 {
        Some(manifest)
    } else {
        None
    }
}

fn get_keyring_password(account: &str) -> Result<Option<String>> {
    match keyring_entry_for(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            Err(anyhow!(e)).with_context(|| format!("read system credential vault {account}"))
        }
    }
}

fn delete_keyring_password(account: &str) {
    match keyring_entry_for(account).and_then(|entry| {
        entry
            .delete_credential()
            .with_context(|| format!("delete system credential vault {account}"))
    }) {
        Ok(()) | Err(_) => {}
    }
}

fn load_keyring_credentials() -> Result<Option<CredsRoot>> {
    let Some(json_or_manifest) = get_keyring_password(KEYRING_CREDENTIALS_ACCOUNT)? else {
        return Ok(None);
    };

    let manifest = read_chunk_manifest(&json_or_manifest)
        .ok_or_else(|| anyhow!("invalid system credential vault manifest"))?;
    // issue #602：manifest 刚从 keychain 读出，进缓存 —— 后续 save 不必再读一次。
    *credentials_manifest_cache().lock() = Some(manifest.clone());
    let mut json = String::new();
    for index in 0..manifest.chunks {
        let account = chunk_account(manifest.generation.as_deref(), index);
        let chunk = get_keyring_password(&account)?
            .ok_or_else(|| anyhow!("missing system credential vault chunk {index}"))?;
        json.push_str(&chunk);
    }

    serde_json::from_str::<CredsRoot>(&json)
        .map(Some)
        .context("decode system credential vault payload")
}

fn load_legacy_keyring_credentials() -> CredsRoot {
    match load_legacy_keyring_credentials_for_update() {
        Ok(root) => root,
        Err(e) => {
            log::warn!("[vault] read legacy vault credentials failed: {e}");
            CredsRoot::default()
        }
    }
}

fn load_legacy_keyring_credentials_for_update() -> Result<CredsRoot> {
    let mut root = CredsRoot::default();
    for account in CredentialAccount::all() {
        let legacy_account = account.keyring_account();
        match get_keyring_password(legacy_account) {
            Ok(Some(value)) => write_account(&mut root, *account, Some(value)),
            Ok(None) => {}
            Err(e) => return Err(e.context(format!("read legacy vault {legacy_account}"))),
        }
    }
    Ok(clean_credentials(&root))
}

fn remove_legacy_keyring_credentials() {
    for account in CredentialAccount::all() {
        delete_keyring_password(account.keyring_account());
    }
}

fn load_legacy_credentials() -> Option<CredsRoot> {
    credentials_path()
        .ok()
        .and_then(|p| read_legacy_credentials_file(&p))
}

fn legacy_vault_has_credentials(root: &CredsRoot) -> bool {
    !root.providers.asr.is_empty() || !root.providers.llm.is_empty()
}

fn load_legacy_sources_without_migration() -> CredsRoot {
    if let Some(legacy) = load_legacy_credentials() {
        return legacy;
    }

    let legacy_vault = load_legacy_keyring_credentials();
    if legacy_vault_has_credentials(&legacy_vault) {
        return legacy_vault;
    }

    CredsRoot::default()
}

fn migrate_legacy_sources() -> CredsRoot {
    match migrate_legacy_sources_for_update() {
        Ok(root) => root,
        Err(e) => {
            log::warn!("[vault] legacy credential migration failed: {e}");
            load_legacy_sources_without_migration()
        }
    }
}

fn migrate_legacy_sources_for_update() -> Result<CredsRoot> {
    if let Some(legacy) = load_legacy_credentials() {
        save_credentials(&legacy)?;
        remove_legacy_keyring_credentials();
        return Ok(legacy);
    }

    let legacy_vault = load_legacy_keyring_credentials_for_update()?;
    if legacy_vault_has_credentials(&legacy_vault) {
        save_credentials(&legacy_vault)?;
        remove_legacy_keyring_credentials();
        return Ok(legacy_vault);
    }

    Ok(CredsRoot::default())
}

fn load_credentials() -> CredsRoot {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return cached;
    }
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 不在这里调 remove_legacy_keyring_credentials() —— 它内部对每个
            // 旧 account 各做一次 keyring delete，每次 delete 在 macOS Keychain
            // 上仍要触发 ACL 检查。第一次成功 load 时 legacy entries 通常已经
            // 被 migrate_legacy_sources_for_update 清理过了；这里若再无脑跑，
            // 只会反复弹「OpenLess 想删除 X」十几次。文件 legacy（plaintext
            // JSON）不需要 ACL，可继续 best-effort 删除。
            remove_legacy_credentials_file_best_effort();
            store_credentials_cache(&root);
            root
        }
        Ok(None) => {
            // 没有现成 chunked manifest —— 走 migrate（如果有 legacy 则写入并返回写后的 root）。
            // migrate_legacy_sources 内部 save_credentials 已经会刷 cache，这里再补一次
            // 是为了「无 legacy 也无 manifest」走默认 root 的路径也能进 cache。
            let root = migrate_legacy_sources();
            store_credentials_cache(&root);
            root
        }
        Err(e) => {
            // **不缓存 keyring 错误路径下的 fallback**。Keychain 可能只是临时不可读
            // （用户尚未在第一次弹窗里点同意 / DataProtection 错误 / login keychain
            // 还没 unlock）；如果在这里把 legacy fallback 写进 cache，等用户授权后
            // 我们就再也不会重读 keyring，整个进程生命周期里都拿 stale 数据。下次
            // 调用让它再尝试一次 keyring。pr_agent feedback on PR #394。
            log::warn!("[vault] system credential read failed: {e}");
            load_legacy_sources_without_migration()
        }
    }
}

fn load_credentials_for_update() -> Result<CredsRoot> {
    if let Some(cached) = credentials_cache().lock().as_ref().cloned() {
        return Ok(cached);
    }
    match load_keyring_credentials() {
        Ok(Some(root)) => {
            // 同 load_credentials：不再每次 update 都尝试 delete legacy keyring
            // entries，避免反复触发 macOS Keychain ACL 弹窗。
            remove_legacy_credentials_file_best_effort();
            store_credentials_cache(&root);
            Ok(root)
        }
        Ok(None) => {
            // migrate_legacy_sources_for_update 内部如果实际 migrate 会调
            // save_credentials，cache 会被刷新；如果只返回 default root（没 legacy），
            // 我们这里再显式 cache 一次防御性补一下。
            let root = migrate_legacy_sources_for_update()?;
            store_credentials_cache(&root);
            Ok(root)
        }
        // 错误路径不缓存 —— 同 load_credentials 注释；让下次读重试 keyring。
        Err(e) => Err(e),
    }
}

fn save_credentials(root: &CredsRoot) -> Result<()> {
    let cleaned = clean_credentials(root);
    let json = serde_json::to_string(&cleaned).context("encode credentials failed")?;
    // issue #602：上次成功读/写的 manifest 有进程缓存时不再回 keychain 读 ——
    // macOS 上这次读本身就要过 ACL 检查（一次弹窗）。冷路径才读真实 manifest。
    let previous_manifest = credentials_manifest_cache().lock().clone().or_else(|| {
        get_keyring_password(KEYRING_CREDENTIALS_ACCOUNT)
            .ok()
            .flatten()
            .and_then(|value| read_chunk_manifest(&value))
    });
    let chunks = chunk_json_payload(&json);

    // issue #602：切换供应商等小改动会触发整套「重写所有 chunks + manifest」，每个
    // keychain 条目各自 ACL、各弹一次「OpenLess 想访问钥匙串」。用进程缓存里上次
    // 成功落盘的 root 反推各 chunk 旧内容，内容没变的 chunk 跳过重写。仅当旧
    // manifest 已是稳定名（generation=None）时可跳 —— UUID 代际的旧 chunk 账户名
    // 不同，内容相同也必须写到新稳定名。缓存序列化顺序偶有差异时只会多写（回到
    // 旧行为），不会漏写。
    let previous_json: Option<String> = match &previous_manifest {
        Some(m) if m.generation.is_none() => credentials_cache()
            .lock()
            .as_ref()
            .and_then(|prev| serde_json::to_string(prev).ok()),
        _ => None,
    };
    let skip = chunk_skip_mask(previous_json.as_deref(), &chunks);

    // 先写所有 chunks（稳定名），再写 manifest —— 保证 partial-write 不会让
    // manifest 指向不完整 chunks。stable name 让 macOS Keychain ACL 一次允许后
    // 长期有效，不再因 UUID 轮换反复弹窗（这是 PR #277 早期 UUID-rotation
    // 设计的回退）。
    let mut chunks_written = 0usize;
    for (index, chunk) in chunks.iter().enumerate() {
        if skip[index] {
            continue;
        }
        let account = chunk_account(None, index);
        keyring_entry_for(&account)?
            .set_password(chunk)
            .with_context(|| format!("write system credential vault chunk {index}"))?;
        chunks_written += 1;
    }

    let manifest = CredsChunkManifest {
        openless_credentials_storage: "chunked".to_string(),
        version: 1,
        generation: None,
        chunks: chunks.len(),
    };
    // manifest 内容只由 chunks 数决定：数量没变且旧 manifest 已是稳定名时内容
    // 逐字节一致，跳过重写（又省一次 ACL 弹窗）。
    let manifest_unchanged = previous_manifest
        .as_ref()
        .is_some_and(|m| m.generation.is_none() && m.chunks == chunks.len());
    if !manifest_unchanged {
        let manifest_json =
            serde_json::to_string(&manifest).context("encode credential manifest failed")?;
        keyring_entry()?
            .set_password(&manifest_json)
            .context("write system credential vault manifest")?;
    }
    log::info!(
        "[vault] save_credentials: {chunks_written}/{} chunks written, manifest {}",
        chunks.len(),
        if manifest_unchanged {
            "unchanged"
        } else {
            "rewritten"
        }
    );

    // 清理旧 chunks：
    // 1) 旧 manifest 用 UUID generation → 那一代 chunks 全删（迁移到 stable name）
    // 2) 旧 manifest 也是 stable name，但 chunks 数量比这次多 → 删多余的 idx
    if let Some(previous) = previous_manifest {
        match previous.generation.as_deref() {
            Some(prev_gen) => {
                for index in 0..previous.chunks {
                    delete_keyring_password(&chunk_account(Some(prev_gen), index));
                }
            }
            None => {
                for index in chunks.len()..previous.chunks {
                    delete_keyring_password(&chunk_account(None, index));
                }
            }
        }
    }

    remove_legacy_credentials_file_best_effort();
    // 写完成功后立刻刷新 process cache —— 同进程后续读不再回 Keychain。
    // 见 CREDENTIALS_CACHE 的 doc。
    store_credentials_cache(&cleaned);
    *credentials_manifest_cache().lock() = Some(manifest);
    Ok(())
}

fn lookup_account(root: &CredsRoot, account: CredentialAccount) -> Option<String> {
    let asr = root.providers.asr.get(&root.active.asr);
    let llm = root.providers.llm.get(&root.active.llm);
    let pick = |s: &Option<String>| s.as_ref().filter(|v| !v.is_empty()).cloned();
    match account {
        CredentialAccount::VolcengineAppKey => {
            asr.and_then(|e| pick(&e.appKey).or_else(|| pick(&e.apiKey)))
        }
        CredentialAccount::VolcengineAccessKey => asr.and_then(|e| pick(&e.accessKey)),
        CredentialAccount::VolcengineResourceId => asr.and_then(|e| pick(&e.resourceId)),
        CredentialAccount::ArkApiKey => llm.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::ArkModelId => llm.and_then(|e| pick(&e.model)),
        CredentialAccount::ArkEndpoint => llm.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::AsrApiKey => asr.and_then(|e| pick(&e.apiKey)),
        CredentialAccount::AsrEndpoint => asr.and_then(|e| pick(&e.baseURL)),
        CredentialAccount::AsrModel => asr.and_then(|e| pick(&e.model)),
        CredentialAccount::AsrVocabularyId => asr.and_then(|e| pick(&e.vocabularyId)),
    }
}

fn write_account(root: &mut CredsRoot, account: CredentialAccount, value: Option<String>) {
    let asr_id = root.active.asr.clone();
    let llm_id = root.active.llm.clone();
    let normalized = value.and_then(|v| if v.is_empty() { None } else { Some(v) });
    match account {
        CredentialAccount::VolcengineAppKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.appKey = normalized;
        }
        CredentialAccount::VolcengineAccessKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.accessKey = normalized;
        }
        CredentialAccount::VolcengineResourceId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.resourceId = normalized;
        }
        CredentialAccount::ArkApiKey => {
            let entry = root.providers.llm.entry(llm_id).or_default();
            entry.apiKey = normalized;
        }
        CredentialAccount::ArkModelId => {
            let entry = root.providers.llm.entry(llm_id).or_default();
            entry.model = normalized;
        }
        CredentialAccount::ArkEndpoint => {
            let entry = root.providers.llm.entry(llm_id).or_default();
            entry.baseURL = normalized;
        }
        CredentialAccount::AsrApiKey => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.apiKey = normalized;
        }
        CredentialAccount::AsrEndpoint => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.baseURL = normalized;
        }
        CredentialAccount::AsrModel => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.model = normalized;
        }
        CredentialAccount::AsrVocabularyId => {
            let entry = root.providers.asr.entry(asr_id).or_default();
            entry.vocabularyId = normalized;
        }
    }
}

// ───────────────────────── HistoryStore ─────────────────────────

pub struct HistoryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl HistoryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(HISTORY_FILE),
            lock: Mutex::new(()),
        })
    }

    pub fn list(&self) -> Result<Vec<DictationSession>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn append(&self, session: DictationSession) -> Result<()> {
        self.append_with_retention(session, 0, None)
    }

    /// `retention_days == 0` 跟旧 append 行为一致（不按时间清理）。
    /// `> 0` 时在写入新条目后顺手把超过 N 天的会话裁掉，写入时就完成清理，
    /// 不需要后台轮询。最后再受条数上限约束：
    /// - `max_entries == None` → HISTORY_CAP (200)
    /// - `max_entries == Some(n)` → clamp 到 5..=HISTORY_CAP，避免用户填 0 / 极大值。
    pub fn append_with_retention(
        &self,
        session: DictationSession,
        retention_days: u32,
        max_entries: Option<u32>,
    ) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        // Prepend so the newest session is at index 0, matching the Swift impl.
        sessions.insert(0, session);
        if retention_days > 0 {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(retention_days));
            sessions.retain(|s| {
                chrono::DateTime::parse_from_rfc3339(&s.created_at)
                    .map(|t| t.with_timezone(&chrono::Utc) >= cutoff)
                    // 解析失败时保守保留——避免错误的时间戳让用户丢历史。
                    .unwrap_or(true)
            });
        }
        let cap = max_entries
            .map(|n| (n as usize).clamp(5, HISTORY_CAP))
            .unwrap_or(HISTORY_CAP);
        if sessions.len() > cap {
            sessions.truncate(cap);
        }
        self.write_locked(&sessions)
    }

    /// 返回最近 N 分钟内的会话（newest-first）。`minutes == 0` → 空 Vec，
    /// 调用方据此跳过对话感知 polish 路径。
    pub fn recent_within_minutes(&self, minutes: u32) -> Result<Vec<DictationSession>> {
        if minutes == 0 {
            return Ok(Vec::new());
        }
        let _guard = self.lock.lock();
        let sessions = self.read_locked()?;
        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(i64::from(minutes));
        // sessions 是 newest-first，超出窗口的会话之后的都更老，take_while 即可。
        let filtered: Vec<DictationSession> = sessions
            .into_iter()
            .take_while(|s| {
                chrono::DateTime::parse_from_rfc3339(&s.created_at)
                    .map(|t| t.with_timezone(&chrono::Utc) >= cutoff)
                    .unwrap_or(false)
            })
            .collect();
        Ok(filtered)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        let original_len = sessions.len();
        sessions.retain(|s| s.id != id);
        if sessions.len() == original_len {
            return Ok(());
        }
        self.write_locked(&sessions)
    }

    pub fn update_entry(&self, session: DictationSession) -> Result<bool> {
        let _guard = self.lock.lock();
        let mut sessions = self.read_locked()?;
        let Some(existing) = sessions.iter_mut().find(|s| s.id == session.id) else {
            return Ok(false);
        };
        *existing = session;
        self.write_locked(&sessions)?;
        Ok(true)
    }

    pub fn clear(&self) -> Result<()> {
        let _guard = self.lock.lock();
        self.write_locked(&Vec::<DictationSession>::new())
    }

    fn read_locked(&self) -> Result<Vec<DictationSession>> {
        read_or_default::<Vec<DictationSession>>(&self.path)
    }

    fn write_locked(&self, sessions: &[DictationSession]) -> Result<()> {
        let json = serde_json::to_vec_pretty(sessions).context("encode history failed")?;
        atomic_write(&self.path, &json)
    }
}

// ───────────────────────── PreferencesStore ─────────────────────────

pub struct PreferencesStore {
    path: PathBuf,
    state: Mutex<UserPreferences>,
}

impl PreferencesStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(PREFERENCES_FILE);
        let prefs = if path.exists() {
            read_preferences(&path).unwrap_or_else(|e| {
                log::warn!(
                    "[prefs] load {} failed, using defaults: {}",
                    path.display(),
                    e
                );
                UserPreferences::default()
            })
        } else {
            UserPreferences::default()
        };
        Ok(Self {
            path,
            state: Mutex::new(prefs),
        })
    }

    pub fn get(&self) -> UserPreferences {
        self.state.lock().clone()
    }

    pub fn set(&self, prefs: UserPreferences) -> Result<()> {
        let json = serde_json::to_vec_pretty(&prefs).context("encode prefs failed")?;
        atomic_write(&self.path, &json)?;
        let mut guard = self.state.lock();
        *guard = prefs;
        Ok(())
    }

    pub fn set_preserving_current_style_preferences(
        &self,
        mut prefs: UserPreferences,
    ) -> Result<()> {
        let mut guard = self.state.lock();
        prefs.preserve_style_preferences_from(&guard);
        let json = serde_json::to_vec_pretty(&prefs).context("encode prefs failed")?;
        atomic_write(&self.path, &json)?;
        *guard = prefs;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StylePackArchiveManifest {
    schema_version: u32,
    id: String,
    name: String,
    description: String,
    author: Option<String>,
    version: String,
    base_mode: PolishMode,
    tags: Vec<String>,
    prompt_file: String,
    examples_file: String,
    icon_file: Option<String>,
    recommended_model: Option<String>,
    compatible_app_version: Option<String>,
    /// Marketplace 上游关系。旧 ZIP 没有此字段时自动为 None；
    /// 兼容早期口误/拼写包里可能出现的 `orion*` 字段名。
    #[serde(
        default,
        alias = "orionPackId",
        alias = "orion_pack_id",
        alias = "origin_pack_id"
    )]
    origin_pack_id: Option<String>,
    #[serde(
        default,
        alias = "orionAuthorLogin",
        alias = "orion_author_login",
        alias = "origin_author_login"
    )]
    origin_author_login: Option<String>,
}

pub struct StylePackStore {
    path: PathBuf,
    asset_root: PathBuf,
    state: Mutex<Vec<StylePack>>,
}

impl StylePackStore {
    pub fn new(prefs: &PreferencesStore) -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        let path = dir.join(STYLE_PACKS_FILE);
        let asset_root = dir.join(STYLE_PACK_ASSETS_DIR);
        ensure_dir(&asset_root)?;

        let mut packs = if path.exists() {
            read_or_default::<Vec<StylePack>>(&path).unwrap_or_else(|error| {
                log::warn!(
                    "[style-packs] load {} failed, using builtin defaults: {}",
                    path.display(),
                    error
                );
                Vec::new()
            })
        } else {
            Vec::new()
        };

        let mut prefs_snapshot = prefs.get();
        let mut changed = migrate_style_packs_from_preferences(&mut packs, &prefs_snapshot);
        if ensure_at_least_one_style_pack_enabled(&mut packs) {
            changed = true;
        }
        let active_pref_for_log = prefs_snapshot.active_style_pack_id.clone();
        let enabled_modes_for_log = prefs_snapshot.enabled_modes.clone();
        if sync_style_pack_preferences(&mut prefs_snapshot, &packs) {
            prefs.set(prefs_snapshot)?;
        }
        if changed {
            write_style_packs_file(&path, &packs)?;
        }
        log::info!(
            "[style-pack] store ready: file={} packs={} changed={} active_pref={} enabled_modes={:?}",
            path.display(),
            packs.len(),
            changed,
            active_pref_for_log,
            enabled_modes_for_log
        );

        Ok(Self {
            path,
            asset_root,
            state: Mutex::new(packs),
        })
    }

    pub fn list(&self) -> Result<Vec<StylePack>> {
        Ok(self.state.lock().clone())
    }

    pub fn list_with_active(&self, active_style_pack_id: &str) -> Result<Vec<StylePack>> {
        let mut packs = self.list()?;
        for pack in &mut packs {
            pack.active = pack.id == active_style_pack_id;
        }
        Ok(packs)
    }

    pub fn get(&self, id: &str) -> Result<StylePack> {
        self.state
            .lock()
            .iter()
            .find(|pack| pack.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("style pack {} not found", id))
    }

    pub fn get_or_default_active(&self, active_style_pack_id: &str) -> Result<StylePack> {
        let packs = self.state.lock().clone();
        if let Some(pack) = packs
            .iter()
            .find(|pack| pack.id == active_style_pack_id && pack.enabled)
            .cloned()
        {
            return Ok(pack);
        }
        if let Some(pack) = packs
            .iter()
            .find(|pack| pack.id == BUILTIN_STYLE_PACK_LIGHT_ID && pack.enabled)
            .cloned()
        {
            return Ok(pack);
        }
        packs
            .into_iter()
            .find(|pack| pack.enabled)
            .ok_or_else(|| anyhow!("no enabled style pack available"))
    }

    /// 从模板新建一个 imported 风格包（"+"按钮路径）。
    /// 跟 ZIP 导入不同：没有 manifest.json、没有 assets，纯空白模板。
    /// 调用方负责 set `prefs.active_style_pack_id` 等高层 wiring（这里只管落盘）。
    pub fn create_from_template(&self, template: StylePack) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let base_id = if template.id.trim().is_empty() {
            format!("imported-{}", Uuid::new_v4().simple())
        } else {
            template.id.clone()
        };
        let assigned_id = unique_imported_style_pack_id(&packs, &base_id);
        let now = Utc::now().to_rfc3339();
        let mut pack = template;
        pack.id = assigned_id;
        pack.kind = StylePackKind::Imported;
        pack.created_at = Some(now.clone());
        pack.updated_at = Some(now);
        pack.active = false;
        pack.enabled = true;
        packs.push(pack.clone());
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] created from template id={} base_mode={:?} prompt_chars={} examples={}",
            pack.id,
            pack.base_mode,
            pack.prompt.chars().count(),
            pack.examples.len()
        );
        Ok(pack)
    }

    pub fn upsert(&self, incoming: StylePack) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == incoming.id)
            .ok_or_else(|| anyhow!("style pack {} not found", incoming.id))?;
        let existing = packs[index].clone();
        let updated = merge_style_pack_update(existing, incoming)?;
        packs[index] = updated.clone();
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] saved id={} kind={:?} base_mode={:?} prompt_chars={} examples={} tags={} version={}",
            updated.id,
            updated.kind,
            updated.base_mode,
            updated.prompt.chars().count(),
            updated.examples.len(),
            updated.tags.len(),
            updated.version
        );
        Ok(updated)
    }

    /// 设置衍生关系；marketplace_install 安装本地包后绑定 upstream id + author。
    /// 单独走这里是为了不让前端通用 save 路径误清这两字段。
    pub fn set_origin(
        &self,
        id: &str,
        origin_pack_id: Option<String>,
        origin_author_login: Option<String>,
    ) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        packs[index].origin_pack_id = normalize_optional_text(origin_pack_id);
        packs[index].origin_author_login = normalize_optional_text(origin_author_login);
        packs[index].updated_at = Some(Utc::now().to_rfc3339());
        let updated = packs[index].clone();
        write_style_packs_file(&self.path, &packs)?;
        Ok(updated)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<StylePack> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        packs[index].enabled = enabled;
        packs[index].updated_at = Some(Utc::now().to_rfc3339());
        if ensure_at_least_one_style_pack_enabled(&mut packs) {
            packs[index].updated_at = Some(Utc::now().to_rfc3339());
        }
        let updated = packs[index].clone();
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] set_enabled id={} enabled={} base_mode={:?}",
            updated.id,
            updated.enabled,
            updated.base_mode
        );
        Ok(updated)
    }

    pub fn reset_builtin(&self, id: &str) -> Result<StylePack> {
        let mode = builtin_mode_from_style_pack_id(id)
            .ok_or_else(|| anyhow!("style pack {} is not a builtin pack", id))?;
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        let existing = packs[index].clone();
        let mut reset = builtin_style_pack_for_mode(mode);
        reset.enabled = existing.enabled;
        reset.created_at = existing
            .created_at
            .or_else(|| Some(Utc::now().to_rfc3339()));
        reset.updated_at = Some(Utc::now().to_rfc3339());
        packs[index] = reset.clone();
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] reset_builtin id={} base_mode={:?} prompt_chars={} examples={}",
            reset.id,
            reset.base_mode,
            reset.prompt.chars().count(),
            reset.examples.len()
        );
        Ok(reset)
    }

    pub fn remove_imported(&self, id: &str) -> Result<()> {
        let mut packs = self.state.lock();
        let index = packs
            .iter()
            .position(|pack| pack.id == id)
            .ok_or_else(|| anyhow!("style pack {} not found", id))?;
        if packs[index].kind == StylePackKind::Builtin {
            return Err(anyhow!("builtin style pack cannot be deleted"));
        }
        let removed = packs[index].clone();
        remove_style_pack_assets(&self.asset_root, &packs[index]);
        packs.remove(index);
        if ensure_at_least_one_style_pack_enabled(&mut packs) {
            // write updated fallback state as well
        }
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] removed imported id={} base_mode={:?}",
            removed.id,
            removed.base_mode
        );
        Ok(())
    }

    pub fn import_from_zip(&self, zip_path: &Path) -> Result<StylePack> {
        let file = fs::File::open(zip_path)
            .with_context(|| format!("open style pack zip failed: {}", zip_path.display()))?;
        let mut archive = zip::ZipArchive::new(file).context("open style pack zip archive")?;
        let manifest: StylePackArchiveManifest =
            read_zip_json_entry(&mut archive, "manifest.json")?;
        let prompt = read_zip_string_entry(&mut archive, &manifest.prompt_file)?;
        let examples =
            read_zip_json_entry::<Vec<StylePackExample>>(&mut archive, &manifest.examples_file)?;

        let mut packs = self.state.lock();
        let now = Utc::now().to_rfc3339();
        let pack_id = unique_imported_style_pack_id(&packs, &manifest.id);
        let icon_path = if let Some(icon_file) = manifest.icon_file.as_deref() {
            extract_style_pack_icon(&mut archive, &self.asset_root, &pack_id, icon_file)?
        } else {
            None
        };
        let pack = StylePack {
            id: pack_id,
            name: manifest.name.trim().to_string(),
            description: manifest.description.trim().to_string(),
            author: manifest
                .author
                .and_then(|value| normalize_optional_text(Some(value))),
            version: normalize_version(&manifest.version),
            kind: StylePackKind::Imported,
            base_mode: manifest.base_mode,
            prompt,
            examples,
            tags: normalize_tags(&manifest.tags),
            icon_path,
            created_at: Some(now.clone()),
            updated_at: Some(now),
            enabled: true,
            active: false,
            recommended_model: manifest
                .recommended_model
                .and_then(|value| normalize_optional_text(Some(value))),
            compatible_app_version: manifest
                .compatible_app_version
                .and_then(|value| normalize_optional_text(Some(value))),
            origin_pack_id: normalize_optional_text(manifest.origin_pack_id),
            origin_author_login: normalize_optional_text(manifest.origin_author_login),
        };
        packs.insert(0, pack.clone());
        write_style_packs_file(&self.path, &packs)?;
        log::info!(
            "[style-pack] imported source={} installed_id={} manifest_id={} base_mode={:?} prompt_chars={} examples={} tags={} icon={}",
            zip_path.display(),
            pack.id,
            manifest.id,
            pack.base_mode,
            pack.prompt.chars().count(),
            pack.examples.len(),
            pack.tags.len(),
            pack.icon_path.is_some()
        );
        Ok(pack)
    }

    pub fn export_to_zip(&self, id: &str, target_path: &Path) -> Result<()> {
        let pack = self.get(id)?;
        if let Some(parent) = target_path.parent() {
            ensure_dir(parent)?;
        }
        let file = fs::File::create(target_path)
            .with_context(|| format!("create style pack zip failed: {}", target_path.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let icon_file = pack
            .icon_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|file_name| file_name.to_str())
            .map(|name| format!("assets/{name}"));

        let manifest = StylePackArchiveManifest {
            schema_version: 1,
            id: pack.id.clone(),
            name: pack.name.clone(),
            description: pack.description.clone(),
            author: pack.author.clone(),
            version: pack.version.clone(),
            base_mode: pack.base_mode,
            tags: pack.tags.clone(),
            prompt_file: "prompt.md".into(),
            examples_file: "examples.json".into(),
            icon_file: icon_file.clone(),
            recommended_model: pack.recommended_model.clone(),
            compatible_app_version: pack.compatible_app_version.clone(),
            origin_pack_id: pack.origin_pack_id.clone(),
            origin_author_login: pack.origin_author_login.clone(),
        };

        zip.start_file("manifest.json", options)
            .context("write style pack manifest entry")?;
        zip.write_all(
            serde_json::to_string_pretty(&manifest)
                .context("encode style pack manifest")?
                .as_bytes(),
        )
        .context("write style pack manifest body")?;

        zip.start_file("prompt.md", options)
            .context("write style pack prompt entry")?;
        zip.write_all(pack.prompt.as_bytes())
            .context("write style pack prompt body")?;

        zip.start_file("examples.json", options)
            .context("write style pack examples entry")?;
        zip.write_all(
            serde_json::to_string_pretty(&pack.examples)
                .context("encode style pack examples")?
                .as_bytes(),
        )
        .context("write style pack examples body")?;

        if let (Some(source_icon_path), Some(zip_icon_path)) = (&pack.icon_path, &icon_file) {
            let icon_source = Path::new(source_icon_path);
            if icon_source.exists() {
                zip.start_file(zip_icon_path, options)
                    .context("write style pack icon entry")?;
                let bytes = fs::read(icon_source).with_context(|| {
                    format!("read style pack icon failed: {}", icon_source.display())
                })?;
                zip.write_all(&bytes)
                    .context("write style pack icon body")?;
            }
        }

        zip.finish().context("finalize style pack zip")?;
        log::info!(
            "[style-pack] exported id={} target={} base_mode={:?} prompt_chars={} examples={} icon={}",
            pack.id,
            target_path.display(),
            pack.base_mode,
            pack.prompt.chars().count(),
            pack.examples.len(),
            pack.icon_path.is_some()
        );
        Ok(())
    }
}

fn write_style_packs_file(path: &Path, packs: &[StylePack]) -> Result<()> {
    let json = serde_json::to_vec_pretty(packs).context("encode style packs failed")?;
    atomic_write(path, &json)
}

fn migrate_style_packs_from_preferences(
    packs: &mut Vec<StylePack>,
    prefs: &UserPreferences,
) -> bool {
    let mut changed = false;
    let legacy_prompts = prefs.style_system_prompts.clone();
    for builtin in builtin_style_packs() {
        if let Some(index) = packs.iter().position(|pack| pack.id == builtin.id) {
            let pack = &mut packs[index];
            if pack.kind != StylePackKind::Builtin {
                pack.kind = StylePackKind::Builtin;
                changed = true;
            }
            if pack.name.trim().is_empty() {
                pack.name = builtin.name.clone();
                changed = true;
            }
            if pack.description.trim().is_empty() {
                pack.description = builtin.description.clone();
                changed = true;
            }
            if pack.prompt.trim().is_empty() {
                pack.prompt = builtin.prompt.clone();
                changed = true;
            }
            if pack.examples.is_empty() {
                pack.examples = builtin.examples.clone();
                changed = true;
            }
            if pack.tags.is_empty() {
                pack.tags = builtin.tags.clone();
                changed = true;
            }
            if pack.version.trim().is_empty() {
                pack.version = builtin.version.clone();
                changed = true;
            }
            if pack.author.is_none() {
                pack.author = builtin.author.clone();
                changed = true;
            }
            if pack.compatible_app_version.is_none() {
                pack.compatible_app_version = builtin.compatible_app_version.clone();
                changed = true;
            }
            if pack.created_at.is_none() {
                pack.created_at = Some(Utc::now().to_rfc3339());
                changed = true;
            }
            if pack.base_mode != builtin.base_mode {
                pack.base_mode = builtin.base_mode;
                changed = true;
            }
        } else {
            let mut pack = builtin.clone();
            pack.prompt = legacy_prompts.for_mode(pack.base_mode).to_string();
            pack.enabled = prefs.enabled_modes.contains(&pack.base_mode);
            pack.created_at = Some(Utc::now().to_rfc3339());
            pack.updated_at = Some(Utc::now().to_rfc3339());
            packs.push(pack);
            changed = true;
        }
    }
    packs.sort_by(|left, right| {
        style_pack_sort_key(left)
            .cmp(&style_pack_sort_key(right))
            .then_with(|| left.name.cmp(&right.name))
    });
    changed
}

fn style_pack_sort_key(pack: &StylePack) -> (u8, u8) {
    let kind_rank = match pack.kind {
        StylePackKind::Builtin => 0,
        StylePackKind::Imported => 1,
    };
    let mode_rank = match pack.base_mode {
        PolishMode::Raw => 0,
        PolishMode::Light => 1,
        PolishMode::Structured => 2,
        PolishMode::Formal => 3,
    };
    (kind_rank, mode_rank)
}

fn ensure_at_least_one_style_pack_enabled(packs: &mut [StylePack]) -> bool {
    if packs.iter().any(|pack| pack.enabled) {
        return false;
    }
    if let Some(pack) = packs
        .iter_mut()
        .find(|pack| pack.id == default_active_style_pack_id())
    {
        pack.enabled = true;
        pack.updated_at = Some(Utc::now().to_rfc3339());
        return true;
    }
    if let Some(first) = packs.first_mut() {
        first.enabled = true;
        first.updated_at = Some(Utc::now().to_rfc3339());
        return true;
    }
    false
}

pub fn sync_style_pack_preferences(prefs: &mut UserPreferences, packs: &[StylePack]) -> bool {
    let previous_active_style_pack_id = prefs.active_style_pack_id.clone();
    let previous_default_mode = prefs.default_mode;
    let previous_enabled_modes = prefs.enabled_modes.clone();
    let enabled: Vec<&StylePack> = packs.iter().filter(|pack| pack.enabled).collect();
    let active = packs
        .iter()
        .find(|pack| pack.id == prefs.active_style_pack_id && pack.enabled)
        .or_else(|| {
            packs
                .iter()
                .find(|pack| pack.id == builtin_style_pack_id(prefs.default_mode) && pack.enabled)
        })
        .or_else(|| enabled.first().copied());

    let Some(active_pack) = active else {
        return false;
    };

    let mut changed = false;
    if prefs.active_style_pack_id != active_pack.id {
        prefs.active_style_pack_id = active_pack.id.clone();
        changed = true;
    }
    if prefs.default_mode != active_pack.base_mode {
        prefs.default_mode = active_pack.base_mode;
        changed = true;
    }

    let next_enabled_modes = enabled_modes_from_style_packs(packs);
    if prefs.enabled_modes != next_enabled_modes {
        prefs.enabled_modes = next_enabled_modes;
        changed = true;
    }

    if sync_builtin_style_prompt_preferences(prefs, packs) {
        changed = true;
    }

    if changed {
        log::info!(
            "[style-pack] sync_prefs active:{}->{} default_mode:{:?}->{:?} enabled_modes:{:?}->{:?}",
            previous_active_style_pack_id,
            prefs.active_style_pack_id,
            previous_default_mode,
            prefs.default_mode,
            previous_enabled_modes,
            prefs.enabled_modes
        );
    }

    changed
}

fn sync_builtin_style_prompt_preferences(prefs: &mut UserPreferences, packs: &[StylePack]) -> bool {
    let mut changed = false;
    let mut saw_builtin = false;
    for mode in [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ] {
        let Some(pack) = packs
            .iter()
            .find(|pack| pack.kind == StylePackKind::Builtin && pack.base_mode == mode)
        else {
            continue;
        };
        saw_builtin = true;
        let next_prompt = pack.prompt.clone();
        let current_prompt = prefs.style_system_prompts.for_mode(mode);
        if current_prompt == next_prompt {
            continue;
        }
        match mode {
            PolishMode::Raw => prefs.style_system_prompts.raw = next_prompt,
            PolishMode::Light => prefs.style_system_prompts.light = next_prompt,
            PolishMode::Structured => prefs.style_system_prompts.structured = next_prompt,
            PolishMode::Formal => prefs.style_system_prompts.formal = next_prompt,
        }
        changed = true;
    }

    if saw_builtin && prefs.custom_style_prompts != CustomStylePrompts::default() {
        prefs.custom_style_prompts = CustomStylePrompts::default();
        changed = true;
    }

    changed
}

pub fn enabled_modes_from_style_packs(packs: &[StylePack]) -> Vec<PolishMode> {
    let mut modes = Vec::new();
    for mode in [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ] {
        if packs
            .iter()
            .any(|pack| pack.enabled && pack.base_mode == mode)
        {
            modes.push(mode);
        }
    }
    modes
}

fn builtin_mode_from_style_pack_id(id: &str) -> Option<PolishMode> {
    for mode in [
        PolishMode::Raw,
        PolishMode::Light,
        PolishMode::Structured,
        PolishMode::Formal,
    ] {
        if builtin_style_pack_id(mode) == id {
            return Some(mode);
        }
    }
    None
}

fn merge_style_pack_update(existing: StylePack, incoming: StylePack) -> Result<StylePack> {
    if existing.id != incoming.id {
        return Err(anyhow!("style pack id cannot be changed"));
    }
    let mut updated = existing;
    updated.name = normalize_required_text(&incoming.name, "style pack name")?;
    updated.description = incoming.description.trim().to_string();
    updated.author = normalize_optional_text(incoming.author);
    updated.version = normalize_version(&incoming.version);
    updated.prompt = incoming.prompt;
    updated.examples = normalize_examples(incoming.examples);
    updated.tags = normalize_tags(&incoming.tags);
    updated.recommended_model = normalize_optional_text(incoming.recommended_model);
    updated.compatible_app_version = normalize_optional_text(incoming.compatible_app_version);
    // origin 字段是 marketplace_install 之后的「衍生关系绑定」，**不能**走通用 save 路径覆盖
    // ——否则前端 save 时丢失 originPackId 就会清掉关联。要写 origin 走专用的 set_origin。
    updated.updated_at = Some(Utc::now().to_rfc3339());
    Ok(updated)
}

fn normalize_examples(examples: Vec<StylePackExample>) -> Vec<StylePackExample> {
    examples
        .into_iter()
        .filter_map(|example| {
            let input = example.input.trim().to_string();
            let output = example.output.trim().to_string();
            if input.is_empty() && output.is_empty() {
                return None;
            }
            Some(StylePackExample {
                title: normalize_optional_text(example.title),
                input,
                output,
            })
        })
        .collect()
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() || normalized.iter().any(|existing| existing == trimmed) {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_required_text(value: &str, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("{field} is empty"));
    }
    Ok(trimmed.to_string())
}

fn normalize_version(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "1.0.0".into()
    } else {
        trimmed.to_string()
    }
}

fn unique_imported_style_pack_id(existing: &[StylePack], requested_id: &str) -> String {
    let base = sanitize_style_pack_id(requested_id);
    if !existing.iter().any(|pack| pack.id == base) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing.iter().any(|pack| pack.id == candidate) {
            return candidate;
        }
        index = index.saturating_add(1);
    }
}

fn sanitize_style_pack_id(requested_id: &str) -> String {
    let mut output = String::new();
    for ch in requested_id.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            output.push(ch);
        } else if matches!(ch, ' ' | '/' | '\\') {
            output.push('-');
        }
    }
    let compact = output.trim_matches('-').trim_matches('.').trim_matches('_');
    if compact.is_empty() {
        format!("imported-{}", Uuid::new_v4().simple())
    } else if compact.starts_with("builtin.") {
        format!("imported.{compact}")
    } else {
        compact.to_string()
    }
}

fn read_zip_json_entry<T: for<'de> Deserialize<'de>>(
    archive: &mut zip::ZipArchive<fs::File>,
    entry_name: &str,
) -> Result<T> {
    let text = read_zip_string_entry(archive, entry_name)?;
    serde_json::from_str(&text)
        .with_context(|| format!("decode style pack zip entry failed: {entry_name}"))
}

fn read_zip_string_entry(
    archive: &mut zip::ZipArchive<fs::File>,
    entry_name: &str,
) -> Result<String> {
    let mut file = archive
        .by_name(entry_name)
        .with_context(|| format!("missing style pack zip entry: {entry_name}"))?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)
        .with_context(|| format!("read style pack zip entry failed: {entry_name}"))?;
    Ok(buffer)
}

fn extract_style_pack_icon(
    archive: &mut zip::ZipArchive<fs::File>,
    asset_root: &Path,
    pack_id: &str,
    entry_name: &str,
) -> Result<Option<String>> {
    let mut file = archive
        .by_name(entry_name)
        .with_context(|| format!("missing style pack icon entry: {entry_name}"))?;
    let file_name = Path::new(entry_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid style pack icon file name"))?;
    let target_dir = asset_root.join(pack_id);
    ensure_dir(&target_dir)?;
    let target_path = target_dir.join(file_name);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .with_context(|| format!("read style pack icon failed: {entry_name}"))?;
    fs::write(&target_path, &bytes)
        .with_context(|| format!("write style pack icon failed: {}", target_path.display()))?;
    Ok(Some(target_path.to_string_lossy().to_string()))
}

fn remove_style_pack_assets(asset_root: &Path, pack: &StylePack) {
    if let Some(icon_path) = pack.icon_path.as_deref() {
        let path = Path::new(icon_path);
        let _ = fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    } else {
        let dir = asset_root.join(&pack.id);
        let _ = fs::remove_dir_all(dir);
    }
}

// ───────────────────────── DictionaryStore ─────────────────────────

pub struct DictionaryStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl DictionaryStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(VOCAB_FILE),
            lock: Mutex::new(()),
        })
    }

    pub fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn add(&self, phrase: String, note: Option<String>) -> Result<DictionaryEntry> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let entry = DictionaryEntry {
            id: Uuid::new_v4().to_string(),
            phrase,
            note,
            enabled: true,
            hits: 0,
            created_at: Utc::now().to_rfc3339(),
        };
        entries.insert(0, entry.clone());
        self.write_locked(&entries)?;
        Ok(entry)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() == before {
            return Ok(());
        }
        self.write_locked(&entries)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        let mut found = false;
        for entry in entries.iter_mut() {
            if entry.id == id {
                entry.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Err(anyhow!("dictionary entry {} not found", id));
        }
        self.write_locked(&entries)
    }

    /// 扫描一段最终文本，对每个 enabled 词条按出现次数累加 `hits`。
    ///
    /// 匹配是大小写不敏感的子串扫描：「Hello hello HELLO」算 3 次。
    /// 返回本次累加的总命中数，方便调用方记录到 history.dictionary_entry_count。
    pub fn record_hits(&self, text: &str) -> Result<u64> {
        if text.is_empty() {
            return Ok(0);
        }
        let _guard = self.lock.lock();
        let mut entries = self.read_locked()?;
        if entries.is_empty() {
            return Ok(0);
        }
        let haystack = text.to_lowercase();
        let mut total: u64 = 0;
        let mut changed = false;
        for entry in entries.iter_mut() {
            if !entry.enabled {
                continue;
            }
            let needle = entry.phrase.trim().to_lowercase();
            if needle.is_empty() {
                continue;
            }
            let count = count_occurrences(&haystack, &needle);
            if count > 0 {
                entry.hits = entry.hits.saturating_add(count);
                total = total.saturating_add(count);
                changed = true;
            }
        }
        if changed {
            self.write_locked(&entries)?;
        }
        Ok(total)
    }

    fn read_locked(&self) -> Result<Vec<DictionaryEntry>> {
        read_or_default::<Vec<DictionaryEntry>>(&self.path)
    }

    fn write_locked(&self, entries: &[DictionaryEntry]) -> Result<()> {
        let json = serde_json::to_vec_pretty(entries).context("encode vocab failed")?;
        atomic_write(&self.path, &json)
    }
}

/// 统计 `needle` 在 `haystack` 中的非重叠出现次数。两侧调用前都应已转小写。
fn count_occurrences(haystack: &str, needle: &str) -> u64 {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count: u64 = 0;
    let mut start = 0usize;
    while let Some(pos) = haystack[start..].find(needle) {
        count = count.saturating_add(1);
        start = start + pos + needle.len();
        if start >= haystack.len() {
            break;
        }
    }
    count
}

pub fn list_vocab_presets() -> Result<VocabPresetStore> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    read_or_default::<VocabPresetStore>(&dir.join(VOCAB_PRESETS_FILE))
}

pub fn save_vocab_presets(store: &VocabPresetStore) -> Result<()> {
    let dir = data_dir()?;
    ensure_dir(&dir)?;
    let path = dir.join(VOCAB_PRESETS_FILE);
    let json = serde_json::to_vec_pretty(store).context("encode vocab presets failed")?;
    atomic_write(&path, &json)
}

// ───────────────────────── CorrectionRuleStore ─────────────────────────

pub struct CorrectionRuleStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl CorrectionRuleStore {
    pub fn new() -> Result<Self> {
        let dir = data_dir()?;
        ensure_dir(&dir)?;
        Ok(Self {
            path: dir.join(CORRECTION_RULES_FILE),
            lock: Mutex::new(()),
        })
    }

    pub fn list(&self) -> Result<Vec<CorrectionRule>> {
        let _guard = self.lock.lock();
        self.read_locked()
    }

    pub fn add(&self, pattern: String, replacement: String) -> Result<CorrectionRule> {
        let pattern = pattern.trim().to_string();
        let replacement = replacement.trim().to_string();
        validate_correction_rule_syntax(&pattern, &replacement)?;
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let rule = CorrectionRule {
            id: Uuid::new_v4().to_string(),
            pattern,
            replacement,
            enabled: true,
            created_at: Utc::now().to_rfc3339(),
        };
        rules.insert(0, rule.clone());
        self.write_locked(&rules)?;
        Ok(rule)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let before = rules.len();
        rules.retain(|r| r.id != id);
        if rules.len() == before {
            return Ok(());
        }
        self.write_locked(&rules)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let _guard = self.lock.lock();
        let mut rules = self.read_locked()?;
        let mut found = false;
        for rule in rules.iter_mut() {
            if rule.id == id {
                rule.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Err(anyhow!("correction rule {} not found", id));
        }
        self.write_locked(&rules)
    }

    fn read_locked(&self) -> Result<Vec<CorrectionRule>> {
        read_or_default::<Vec<CorrectionRule>>(&self.path)
    }

    fn write_locked(&self, rules: &[CorrectionRule]) -> Result<()> {
        let json = serde_json::to_vec_pretty(rules).context("encode correction rules failed")?;
        atomic_write(&self.path, &json)
    }
}

fn validate_correction_rule_syntax(pattern: &str, replacement: &str) -> Result<()> {
    if pattern.is_empty() {
        return Err(anyhow!("correction rule pattern is empty"));
    }
    let pattern_token_count = pattern.matches(CORRECTION_NUM_TOKEN).count();
    if pattern_token_count > 1 {
        return Err(anyhow!("unsupported correction rule syntax"));
    }
    if replacement.contains(CORRECTION_NUM_TOKEN) && pattern_token_count == 0 {
        return Err(anyhow!("unsupported correction rule syntax"));
    }
    if pattern_token_count == 1 {
        let Some((prefix, suffix)) = pattern.split_once(CORRECTION_NUM_TOKEN) else {
            return Err(anyhow!("unsupported correction rule syntax"));
        };
        if prefix.is_empty() && suffix.is_empty() {
            return Err(anyhow!("unsupported correction rule syntax"));
        }
    }
    Ok(())
}

// ───────────────────────── CredentialsVault ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CredentialAccount {
    VolcengineAppKey,
    VolcengineAccessKey,
    VolcengineResourceId,
    ArkApiKey,
    ArkModelId,
    ArkEndpoint,
    /// Active ASR provider's API key (used by Whisper-compatible providers).
    AsrApiKey,
    /// Active ASR provider's base URL.
    AsrEndpoint,
    /// Active ASR provider's model name.
    AsrModel,
    /// Active ASR provider's optional hotword vocabulary ID.
    AsrVocabularyId,
}

impl CredentialAccount {
    /// Account names match the Swift `CredentialAccount` constants exactly so
    /// existing Keychain entries written by the macOS Swift app remain
    /// readable after upgrade.
    pub fn keyring_account(&self) -> &'static str {
        match self {
            CredentialAccount::VolcengineAppKey => "volcengine.app_key",
            CredentialAccount::VolcengineAccessKey => "volcengine.access_key",
            CredentialAccount::VolcengineResourceId => "volcengine.resource_id",
            CredentialAccount::ArkApiKey => "ark.api_key",
            CredentialAccount::ArkModelId => "ark.model_id",
            CredentialAccount::ArkEndpoint => "ark.endpoint",
            CredentialAccount::AsrApiKey => "asr.api_key",
            CredentialAccount::AsrEndpoint => "asr.endpoint",
            CredentialAccount::AsrModel => "asr.model",
            CredentialAccount::AsrVocabularyId => "asr.vocabulary_id",
        }
    }

    pub fn all() -> &'static [CredentialAccount] {
        &[
            CredentialAccount::VolcengineAppKey,
            CredentialAccount::VolcengineAccessKey,
            CredentialAccount::VolcengineResourceId,
            CredentialAccount::ArkApiKey,
            CredentialAccount::ArkModelId,
            CredentialAccount::ArkEndpoint,
            CredentialAccount::AsrApiKey,
            CredentialAccount::AsrEndpoint,
            CredentialAccount::AsrModel,
            CredentialAccount::AsrVocabularyId,
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsSnapshot {
    pub volcengine_app_key: Option<String>,
    pub volcengine_access_key: Option<String>,
    pub volcengine_resource_id: Option<String>,
    pub asr_api_key: Option<String>,
    pub asr_endpoint: Option<String>,
    pub asr_model: Option<String>,
    pub ark_api_key: Option<String>,
    pub ark_model_id: Option<String>,
    pub ark_endpoint: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LlmProviderCredentials {
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

/// 凭据存储——系统凭据库；旧 JSON 文件只作为迁移来源。
pub struct CredentialsVault;

impl CredentialsVault {
    /// 系统凭据库 service name；macOS 下对应 Keychain service。
    pub const SERVICE_NAME: &'static str = "com.openless.unbound";

    pub fn get(account: CredentialAccount) -> Result<Option<String>> {
        let _guard = credentials_lock().lock();
        Ok(lookup_account(&load_credentials(), account))
    }

    pub fn set(account: CredentialAccount, value: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        let v = if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        };
        write_account(&mut root, account, v);
        save_credentials(&root)
    }

    pub fn remove(account: CredentialAccount) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        write_account(&mut root, account, None);
        save_credentials(&root)
    }

    pub fn get_active_asr() -> String {
        let _guard = credentials_lock().lock();
        load_credentials().active.asr
    }

    pub fn set_active_asr_provider(id: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.active.asr = id.to_string();
        save_credentials(&root)
    }

    pub fn set_active_llm_provider(id: &str) -> Result<()> {
        let _guard = credentials_lock().lock();
        let mut root = load_credentials_for_update()?;
        root.active.llm = id.to_string();
        save_credentials(&root)
    }

    pub fn get_active_llm() -> String {
        let _guard = credentials_lock().lock();
        load_credentials().active.llm
    }

    pub fn get_llm_provider_credentials(id: &str) -> Result<LlmProviderCredentials> {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        let pick = |s: &Option<String>| s.as_ref().filter(|v| !v.trim().is_empty()).cloned();
        Ok(root
            .providers
            .llm
            .get(id)
            .map(|entry| LlmProviderCredentials {
                api_key: pick(&entry.apiKey),
                model: pick(&entry.model),
                base_url: pick(&entry.baseURL),
            })
            .unwrap_or_default())
    }

    pub fn snapshot() -> CredentialsSnapshot {
        let _guard = credentials_lock().lock();
        let root = load_credentials();
        CredentialsSnapshot {
            volcengine_app_key: lookup_account(&root, CredentialAccount::VolcengineAppKey),
            volcengine_access_key: lookup_account(&root, CredentialAccount::VolcengineAccessKey),
            volcengine_resource_id: lookup_account(&root, CredentialAccount::VolcengineResourceId),
            asr_api_key: lookup_account(&root, CredentialAccount::AsrApiKey),
            asr_endpoint: lookup_account(&root, CredentialAccount::AsrEndpoint),
            asr_model: lookup_account(&root, CredentialAccount::AsrModel),
            ark_api_key: lookup_account(&root, CredentialAccount::ArkApiKey),
            ark_model_id: lookup_account(&root, CredentialAccount::ArkModelId),
            ark_endpoint: lookup_account(&root, CredentialAccount::ArkEndpoint),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        chunk_json_payload, chunk_skip_mask, list_vocab_presets, read_preferences,
        save_vocab_presets, sync_style_pack_preferences, validate_correction_rule_syntax,
        PreferencesStore, KEYRING_CHUNK_MAX_UTF16_UNITS,
    };
    use crate::types::{
        builtin_style_packs, CustomStylePrompts, PolishMode, UserPreferences, VocabPreset,
        VocabPresetStore,
    };
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn credential_payload_chunks_stay_under_windows_blob_limit() {
        let payload = format!(
            "{}{}{}",
            "a".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS + 25),
            "😀".repeat(20),
            "b".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS + 25)
        );
        let chunks = chunk_json_payload(&payload);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), payload);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= KEYRING_CHUNK_MAX_UTF16_UNITS));
    }

    #[test]
    fn chunk_skip_mask_skips_unchanged_and_rewrites_changed() {
        // issue #602：内容完全一致 → 全部跳过（no-op save 不再碰 keychain chunks）。
        let json = format!(
            "{}{}",
            "a".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS),
            "b".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS)
        );
        let chunks = chunk_json_payload(&json);
        assert_eq!(chunks.len(), 2);
        assert!(chunk_skip_mask(Some(&json), &chunks).iter().all(|s| *s));

        // 等长改动只落在第 2 个 chunk → 第 1 个跳过、第 2 个重写。
        let changed = format!(
            "{}{}",
            "a".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS),
            "c".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS)
        );
        let changed_chunks = chunk_json_payload(&changed);
        assert_eq!(
            chunk_skip_mask(Some(&json), &changed_chunks),
            vec![true, false]
        );

        // 冷缓存（None）→ 全部重写，等同旧行为。
        assert!(chunk_skip_mask(None, &chunks).iter().all(|s| !*s));

        // 旧内容更短 → 超出部分无旧 chunk 可比，必须写。
        let shorter = "a".repeat(KEYRING_CHUNK_MAX_UTF16_UNITS);
        assert_eq!(chunk_skip_mask(Some(&shorter), &chunks), vec![true, false]);
    }

    #[test]
    fn legacy_streaming_insert_false_is_migrated_and_marker_is_persisted() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        let path = tmp.join("preferences.json");
        fs::write(
            &path,
            r#"{
                "streamingInsert": false,
                "streamingInsertSaveClipboard": true
            }"#,
        )
        .expect("write legacy prefs");

        let prefs = read_preferences(&path).expect("read prefs");
        assert!(prefs.streaming_insert);
        assert!(prefs.streaming_insert_default_migrated);

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read saved prefs"))
                .expect("decode saved prefs");
        assert_eq!(
            saved
                .get("streamingInsert")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            saved
                .get("streamingInsertDefaultMigrated")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn settings_save_preserves_current_style_preferences() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-style-prefs-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        let path = tmp.join("preferences.json");
        let current = UserPreferences {
            default_mode: PolishMode::Light,
            active_style_pack_id: "local.light-cleanup".to_string(),
            ..UserPreferences::default()
        };
        let store = PreferencesStore {
            path,
            state: parking_lot::Mutex::new(current),
        };
        let incoming = UserPreferences {
            default_mode: PolishMode::Formal,
            active_style_pack_id: "builtin.formal".to_string(),
            microphone_device_name: "External Mic".to_string(),
            ..UserPreferences::default()
        };

        store
            .set_preserving_current_style_preferences(incoming)
            .expect("save prefs");

        let saved = store.get();
        assert_eq!(saved.default_mode, PolishMode::Light);
        assert_eq!(saved.active_style_pack_id, "local.light-cleanup");
        assert_eq!(saved.microphone_device_name, "External Mic");
        let saved_on_disk = read_preferences(&store.path).expect("read saved prefs");
        assert_eq!(saved_on_disk.active_style_pack_id, "local.light-cleanup");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn vocab_presets_roundtrip_json_file() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).expect("create temp dir");
        // Linux path helper uses XDG_DATA_HOME first.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &tmp);
        }
        let store = VocabPresetStore {
            custom: vec![VocabPreset {
                id: "test".into(),
                name: "测试".into(),
                phrases: vec!["PR".into(), "CI".into()],
            }],
            overrides: vec![],
            disabled_builtin_preset_ids: vec!["chef".into()],
        };
        save_vocab_presets(&store).expect("save presets");
        let loaded = list_vocab_presets().expect("list presets");
        assert_eq!(loaded.custom.len(), 1);
        assert_eq!(loaded.custom[0].id, "test");
        assert_eq!(
            loaded.custom[0].phrases,
            vec!["PR".to_string(), "CI".to_string()]
        );
        assert_eq!(loaded.disabled_builtin_preset_ids, vec!["chef".to_string()]);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn custom_models_root_uses_openless_models_suffix() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-model-root-{}", uuid::Uuid::new_v4()));
        let root = super::models_root_for_base_dir(Some(tmp.to_string_lossy().as_ref()))
            .expect("build custom models root");

        assert_eq!(root, tmp.join("OpenLess").join("models"));
        assert!(root.is_dir());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn migrate_models_root_merges_without_overwriting_target_files() {
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("openless-model-migrate-{}", uuid::Uuid::new_v4()));
        let old_root = tmp.join("old");
        let new_root = tmp.join("new");
        fs::create_dir_all(old_root.join("qwen3-asr")).expect("create old qwen dir");
        fs::create_dir_all(new_root.join("qwen3-asr")).expect("create new qwen dir");
        fs::write(old_root.join("qwen3-asr").join("moved.bin"), b"old").expect("write moved");
        fs::write(old_root.join("qwen3-asr").join("conflict.bin"), b"old")
            .expect("write old conflict");
        fs::write(new_root.join("qwen3-asr").join("conflict.bin"), b"new")
            .expect("write new conflict");

        super::migrate_models_root(&old_root, &new_root).expect("migrate models root");

        assert_eq!(
            fs::read(new_root.join("qwen3-asr").join("moved.bin")).expect("read moved"),
            b"old"
        );
        assert_eq!(
            fs::read(new_root.join("qwen3-asr").join("conflict.bin")).expect("read new conflict"),
            b"new"
        );
        assert_eq!(
            fs::read(old_root.join("qwen3-asr").join("conflict.bin")).expect("read old conflict"),
            b"old"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn correction_rule_syntax_rejects_silent_noops() {
        assert!(validate_correction_rule_syntax("{num}粒", "{num}例").is_ok());
        assert!(validate_correction_rule_syntax("几粒", "几例").is_ok());
        assert!(validate_correction_rule_syntax("", "几例").is_err());
        assert!(validate_correction_rule_syntax("{num}", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("{num}到{num}粒", "{num}例").is_err());
        assert!(validate_correction_rule_syntax("几粒", "{num}例").is_err());
    }

    #[test]
    fn sync_style_pack_preferences_uses_builtin_store_prompts_as_source_of_truth() {
        let mut prefs = crate::types::UserPreferences {
            style_system_prompts: crate::types::StyleSystemPrompts {
                raw: "stale raw".into(),
                light: "stale light".into(),
                structured: "stale structured".into(),
                formal: "stale formal".into(),
            },
            custom_style_prompts: CustomStylePrompts {
                raw: String::new(),
                light: "legacy extra instruction".into(),
                structured: String::new(),
                formal: String::new(),
            },
            ..Default::default()
        };
        let mut packs = builtin_style_packs();
        let light = packs
            .iter_mut()
            .find(|pack| pack.id == "builtin.light")
            .expect("builtin light pack");
        light.prompt = "fresh light prompt from store".into();

        assert!(sync_style_pack_preferences(&mut prefs, &packs));
        assert_eq!(prefs.style_system_prompts.raw, packs[0].prompt);
        assert_eq!(
            prefs.style_system_prompts.light,
            "fresh light prompt from store"
        );
        assert_eq!(prefs.style_system_prompts.structured, packs[2].prompt);
        assert_eq!(prefs.style_system_prompts.formal, packs[3].prompt);
        assert_eq!(prefs.custom_style_prompts, CustomStylePrompts::default());
    }
}
