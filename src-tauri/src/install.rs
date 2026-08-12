use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::asset_conflicts::{self, AssetConflictState};

use md5::{Digest, Md5};

use crate::auth::{self, AuthState};
use crate::game;
use crate::nexus;
use crate::preferences;
use crate::storage;
use crate::utoc;

const GAME_DOMAIN: &str = "marvelrivals";
const BASE_URL: &str = "https://api.nexusmods.com/v1/games";
const INSTALLED_FILE: &str = "installed.json";
const UNDO_FILE: &str = "last-install-change.json";
const PENDING_CHANGE_FILE: &str = "pending-mod-change.json";
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const MOD_INFO_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const MAX_METADATA_CACHE_ENTRIES: usize = 128;
const DISK_SPACE_MARGIN: u64 = 16 * 1024 * 1024;
static INSTALL_PLAN_ID: AtomicU64 = AtomicU64::new(0);

type ModInfoCache = HashMap<u32, (std::time::Instant, ModInfo)>;
static MOD_INFO_CACHE: OnceLock<Mutex<ModInfoCache>> = OnceLock::new();
type ModFilesCache = HashMap<u32, (std::time::Instant, Vec<ModFile>)>;
static MOD_FILES_CACHE: OnceLock<Mutex<ModFilesCache>> = OnceLock::new();
type ArchiveMd5Cache = HashMap<PathBuf, (std::time::SystemTime, u64, String)>;
static ARCHIVE_MD5_CACHE: OnceLock<Mutex<ArchiveMd5Cache>> = OnceLock::new();

fn mod_info_cache() -> &'static Mutex<ModInfoCache> {
    MOD_INFO_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mod_files_cache() -> &'static Mutex<ModFilesCache> {
    MOD_FILES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn archive_md5_cache() -> &'static Mutex<ArchiveMd5Cache> {
    ARCHIVE_MD5_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_timed_cache<T>(cache: &mut HashMap<u32, (std::time::Instant, T)>) {
    cache.retain(|_, (cached_at, _)| cached_at.elapsed() < MOD_INFO_CACHE_TTL);
    if cache.len() < MAX_METADATA_CACHE_ENTRIES {
        return;
    }
    if let Some(oldest) = cache
        .iter()
        .min_by_key(|(_, (cached_at, _))| *cached_at)
        .map(|(mod_id, _)| *mod_id)
    {
        cache.remove(&oldest);
    }
}

#[derive(Default)]
pub struct InstallState {
    prepared: Mutex<HashMap<String, PreparedInstall>>,
    mutation: Mutex<()>,
}

struct PreparedInstall {
    preview: InstallPreview,
    created_at: std::time::Instant,
    inventory_fingerprint: String,
    path_snapshot: Vec<PreviewPathState>,
    temp_dir: PathBuf,
    source_archives: Vec<PathBuf>,
    delete_source_archives: bool,
    game_path: String,
    plan: Vec<(PathBuf, PathBuf)>,
    identities: Vec<ArchiveIdentity>,
    info: ModInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewPathState {
    path: String,
    active: bool,
    disabled: bool,
}

struct PrepareArchiveRequest {
    mod_id: u32,
    game_path: String,
    temp_dir: PathBuf,
    source_archives: Vec<PathBuf>,
    verified_identities: Option<Vec<ArchiveIdentity>>,
    archive_verified: Vec<bool>,
    expected_file_ids: Option<Vec<u32>>,
    delete_source_archives: bool,
}

impl Drop for PreparedInstall {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.temp_dir);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallPreviewAction {
    Add,
    Replace,
    Remove,
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallPreviewFile {
    pub name: String,
    pub size_bytes: u64,
    pub action: InstallPreviewAction,
    pub owner_mod_id: Option<u32>,
    pub owner_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallPreview {
    pub plan_id: String,
    pub mod_id: u32,
    pub mod_name: String,
    pub version: String,
    pub archive_name: String,
    pub archive_md5: String,
    pub archive_verified: bool,
    pub required_bytes: u64,
    pub available_bytes: Option<u64>,
    pub enough_space: bool,
    pub adds: usize,
    pub replaces: usize,
    pub removes: usize,
    pub blocked_files: usize,
    pub asset_conflicts: asset_conflicts::PreviewAssetConflictReport,
    pub can_install: bool,
    pub archives: Vec<InstallPreviewArchive>,
    pub files: Vec<InstallPreviewFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallPreviewArchive {
    pub file_id: Option<u32>,
    pub file_name: String,
    pub md5: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UndoBackup {
    original: String,
    backup: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UndoFingerprint {
    path: String,
    size_bytes: u64,
    #[serde(default)]
    modified_at_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallUndoRecord {
    transaction_id: String,
    created_at: i64,
    label: String,
    mod_id: u32,
    game_path: String,
    previous: Option<InstalledMod>,
    installed: InstalledMod,
    backups: Vec<UndoBackup>,
    after_files: Vec<UndoFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PendingChangeState {
    Prepared,
    BackedUp,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingChange {
    transaction_id: String,
    game_path: String,
    state: PendingChangeState,
    before_inventory: Vec<InstalledMod>,
    backups: Vec<UndoBackup>,
    new_files: Vec<String>,
    retain_backups_on_commit: bool,
    #[serde(default)]
    clear_undo_on_commit: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoStatus {
    pub available: bool,
    pub label: Option<String>,
    pub created_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMod {
    pub mod_id: u32,
    pub name: String,
    #[serde(default)]
    pub version: String,
    // `alias = "paks"` migrates entries written by older builds.
    #[serde(default, alias = "paks")]
    pub files: Vec<String>,
    #[serde(default)]
    pub installed_at: i64,
    #[serde(default)]
    pub nexus_file_id: Option<u32>,
    #[serde(default)]
    pub archive_name: Option<String>,
    #[serde(default)]
    pub archive_md5: Option<String>,
    #[serde(default)]
    pub parts: Vec<InstalledPart>,
    #[serde(default)]
    pub picture_url: Option<String>,
    /// Derived from the filesystem whenever the installed list is read.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledStats {
    pub mod_count: usize,
    pub enabled_count: usize,
    pub disabled_count: usize,
    pub missing_count: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPart {
    pub nexus_file_id: Option<u32>,
    pub archive_name: String,
    pub archive_md5: String,
}

/// Path used to disable a mod file by renaming, e.g. `Mod.pak` -> `Mod.pak.disabled`.
fn disabled_path(path: &Path) -> PathBuf {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) if !name.ends_with(".disabled") => {
            path.with_file_name(format!("{name}.disabled"))
        }
        _ => path.to_path_buf(),
    }
}

/// Derive the enabled / missing state of an installed mod from its files.
fn compute_state(installed: &InstalledMod) -> (bool, bool) {
    if installed.files.is_empty() {
        return (false, true);
    }
    let states: Vec<(bool, bool)> = installed
        .files
        .iter()
        .map(|file| {
            let active = Path::new(file);
            (active.exists(), disabled_path(active).exists())
        })
        .collect();
    let enabled = states.iter().all(|(active, disabled)| *active && !disabled);
    let broken = states.iter().any(|(active, disabled)| active == disabled);
    (enabled, broken)
}

/// Move every tracked file to the requested state. Preflight prevents
/// collisions, and a failed rename rolls back files already moved.
fn rename_mod_files(files: &[String], enabled: bool) -> Result<(), InstallError> {
    let action = if enabled { "enable" } else { "disable" };
    let mut plan = Vec::new();

    for file in files {
        let active = PathBuf::from(file);
        let disabled = disabled_path(&active);
        let active_exists = active.exists();
        let disabled_exists = disabled.exists();

        match (active_exists, disabled_exists) {
            (true, true) => {
                return Err(InstallError::Install(format!(
                    "Could not {action} the mod because both '{}' and '{}' exist.",
                    active.display(),
                    disabled.display()
                )));
            }
            (false, false) => {
                return Err(InstallError::Install(format!(
                    "Could not {action} the mod because '{}' is missing.",
                    active.display()
                )));
            }
            (true, false) if !enabled => plan.push((active, disabled)),
            (false, true) if enabled => plan.push((disabled, active)),
            _ => {}
        }
    }

    let mut completed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (source, destination) in plan {
        if let Err(error) = std::fs::rename(&source, &destination) {
            let mut rollback_failures = Vec::new();
            for (previous_source, previous_destination) in completed.iter().rev() {
                if let Err(rollback_error) = std::fs::rename(previous_destination, previous_source)
                {
                    rollback_failures.push(format!(
                        "'{}': {rollback_error}",
                        previous_destination.display()
                    ));
                }
            }
            let rollback_note = if rollback_failures.is_empty() {
                " No files were left partially toggled.".to_string()
            } else {
                format!(
                    " Rollback also failed for {}. Reinstall the mod to repair it.",
                    rollback_failures.join(", ")
                )
            };
            return Err(InstallError::Install(format!(
                "Could not {action} '{}': {error}. Close Marvel Rivals and try again.{rollback_note}",
                source.display()
            )));
        }
        completed.push((source, destination));
    }

    let complete = files.iter().all(|file| {
        let active = Path::new(file);
        let disabled = disabled_path(active);
        if enabled {
            active.exists() && !disabled.exists()
        } else {
            !active.exists() && disabled.exists()
        }
    });
    if !complete {
        return Err(InstallError::Install(format!(
            "Could not fully {action} the mod. Reinstall it to repair its files."
        )));
    }
    Ok(())
}

/// A missing inventory means no mods have been installed. Corrupt or
/// unreadable metadata must stop mutations so it cannot be overwritten.
fn read_installed_list(path: &Path) -> Result<Vec<InstalledMod>, InstallError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
            InstallError::Storage(format!(
                "Could not parse installed mod metadata. Restore or remove '{}': {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(InstallError::Storage(format!(
            "Could not read installed mod metadata: {error}"
        ))),
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum InstallError {
    #[serde(rename = "setup_required")]
    SetupRequired(String),
    #[serde(rename = "not_authenticated")]
    NotAuthenticated(String),
    #[serde(rename = "game_not_found")]
    GameNotFound,
    #[serde(rename = "network")]
    Network(String),
    #[serde(rename = "api")]
    Api(String),
    #[serde(rename = "storage")]
    Storage(String),
    #[serde(rename = "no_files")]
    NoFiles,
    #[serde(rename = "no_download_link")]
    NoDownloadLink,
    #[serde(rename = "download")]
    Download(String),
    #[serde(rename = "extract")]
    Extract(String),
    #[serde(rename = "no_paks")]
    NoPaks,
    #[serde(rename = "install")]
    Install(String),
    #[serde(rename = "archive_mismatch")]
    ArchiveMismatch(String),
    #[serde(rename = "game_files_locked")]
    GameFilesLocked,
}

fn ensure_game_files_mutable() -> Result<(), InstallError> {
    game::ensure_game_files_mutable().map_err(|error| match error {
        game::GameError::GameFilesLocked => InstallError::GameFilesLocked,
        other => InstallError::Storage(other.to_string()),
    })
}

#[derive(Debug, Clone, Deserialize)]
struct ModInfo {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    picture_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModFile {
    file_id: u32,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    // The API can return both `size_kb` and `size`; an alias would collide with
    // the real `size_kb` key and make every files response fail to parse.
    #[serde(default)]
    size_kb: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FilesResponse {
    files: Vec<ModFile>,
}

#[derive(Debug, Deserialize)]
struct DownloadLink {
    #[serde(rename = "URI", alias = "uri")]
    uri: String,
}

#[derive(Debug, Clone)]
struct ArchiveIdentity {
    file_id: Option<u32>,
    file_name: Option<String>,
    md5: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModInstallFileOption {
    pub file_id: u32,
    pub display_name: String,
    pub file_name: String,
    pub version: Option<String>,
    pub category_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub part_number: Option<u32>,
    pub contents: Vec<ModInstallContentFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModInstallContentFile {
    pub path: String,
    pub file_name: String,
    pub size_bytes: Option<u64>,
    pub action: InstallPreviewAction,
    pub owner_mod_id: Option<u32>,
    pub owner_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModInstallOption {
    pub id: String,
    pub label: String,
    pub multipart: bool,
    pub recommended: bool,
    pub total_size_bytes: Option<u64>,
    pub content_preview_available: bool,
    pub predicted_adds: usize,
    pub predicted_replaces: usize,
    pub predicted_removes: usize,
    pub predicted_blocked_files: usize,
    pub predicted_removed_files: Vec<String>,
    pub files: Vec<ModInstallFileOption>,
}

fn map_utoc(e: utoc::UtocError) -> InstallError {
    match e {
        utoc::UtocError::NotAuthenticated(msg) => InstallError::NotAuthenticated(msg),
        utoc::UtocError::GameNotFound => InstallError::GameNotFound,
        utoc::UtocError::Network(msg) => InstallError::Network(msg),
        utoc::UtocError::Api(msg) => InstallError::Api(msg),
        utoc::UtocError::Storage(msg) => InstallError::Storage(msg),
        utoc::UtocError::NoFiles => InstallError::NoFiles,
        utoc::UtocError::NoDownloadLink => InstallError::NoDownloadLink,
        utoc::UtocError::Download(msg) => InstallError::Download(msg),
        utoc::UtocError::Extract(msg) => InstallError::Extract(msg),
        other => InstallError::Api(format!("{other:?}")),
    }
}

fn mods_dir(game_path: &str) -> PathBuf {
    PathBuf::from(game_path)
        .join("MarvelGame")
        .join("Marvel")
        .join("Content")
        .join("Paks")
        .join("~mods")
}

fn emit(app: &AppHandle, phase: &str) {
    let _ = app.emit("mod-install-progress", phase);
}

fn ensure_setup(game_path: &str) -> Result<(), InstallError> {
    let status = utoc::check_installed(game_path);
    if !status.installed {
        return Err(InstallError::SetupRequired(
            "UTOC Signature Bypass is not installed. Install it in Settings first.".to_string(),
        ));
    }
    std::fs::create_dir_all(mods_dir(game_path))
        .map_err(|e| InstallError::Install(format!("Could not create the ~mods folder: {e}")))?;
    Ok(())
}

fn resolve_api_key(
    app: &AppHandle,
    state: &State<'_, AuthState>,
    api_key: &str,
) -> Result<String, InstallError> {
    if !api_key.trim().is_empty() {
        Ok(api_key.trim().to_string())
    } else {
        auth::resolve_api_key(app, state)
            .map_err(|e| InstallError::NotAuthenticated(format!("{e:?}")))
    }
}

async fn nexus_get_json(api_key: &str, url: &str) -> Result<serde_json::Value, InstallError> {
    if let Some(message) = nexus::rate_limit_cooldown() {
        return Err(InstallError::Api(message));
    }
    let response = nexus::http_client()
        .get(url)
        .header("apikey", api_key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| InstallError::Network(e.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .await
            .map_err(|e| InstallError::Api(format!("Invalid response: {e}")));
    }
    let rate_limit =
        (status.as_u16() == 429).then(|| nexus::rate_limit_message(response.headers()));
    let body = response.text().await.unwrap_or_default();
    match status.as_u16() {
        401 | 403 => Err(InstallError::NotAuthenticated(format!(
            "HTTP {status}: {body}"
        ))),
        429 => Err(InstallError::Api(rate_limit.unwrap_or_else(|| {
            "Nexus Mods rate limit reached. Please try again later.".to_string()
        }))),
        s => Err(InstallError::Api(format!(
            "Nexus Mods returned HTTP {s}: {body}"
        ))),
    }
}

async fn nexus_post_json(
    api_key: &str,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, InstallError> {
    if let Some(message) = nexus::rate_limit_cooldown() {
        return Err(InstallError::Api(message));
    }
    let response = nexus::http_client()
        .post(url)
        .header("apikey", api_key)
        .json(body)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| InstallError::Network(error.to_string()))?;
    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .await
            .map_err(|error| InstallError::Api(format!("Invalid response: {error}")));
    }
    let rate_limit =
        (status.as_u16() == 429).then(|| nexus::rate_limit_message(response.headers()));
    let body = response.text().await.unwrap_or_default();
    match status.as_u16() {
        401 | 403 => Err(InstallError::NotAuthenticated(format!(
            "HTTP {status}: {body}"
        ))),
        429 => Err(InstallError::Api(rate_limit.unwrap_or_else(|| {
            "Nexus Mods rate limit reached. Please try again later.".to_string()
        }))),
        code => Err(InstallError::Api(format!(
            "Nexus Mods returned HTTP {code}: {body}"
        ))),
    }
}

#[derive(Debug, Clone)]
struct IndexedArchiveContent {
    path: String,
    file_name: String,
    size_bytes: Option<u64>,
}

fn json_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str()?.parse::<u64>().ok())
    })
}

async fn get_indexed_archive_contents(
    api_key: &str,
    file_ids: &[u32],
) -> Result<HashMap<u32, Vec<IndexedArchiveContent>>, InstallError> {
    const GRAPHQL_URL: &str = "https://api.nexusmods.com/v2/graphql";
    const PAGE_SIZE: u64 = 500;
    const MAX_PAGES: u64 = 4;
    const QUERY: &str = r#"
        query ModFileContents($filter: ModFileContentSearchFilter, $offset: Int, $count: Int) {
          modFileContents(filter: $filter, offset: $offset, count: $count) {
            nodes { fileId filePath fileName fileExtension fileSize }
            totalCount
            nodesCount
          }
        }
    "#;
    let wanted: std::collections::HashSet<u32> = file_ids.iter().copied().collect();
    let mut result: HashMap<u32, Vec<IndexedArchiveContent>> = wanted
        .iter()
        .map(|file_id| (*file_id, Vec::new()))
        .collect();
    let filters: Vec<serde_json::Value> = wanted
        .iter()
        .map(|file_id| serde_json::json!({ "op": "EQUALS", "value": file_id.to_string() }))
        .collect();
    let mut offset = 0_u64;
    for _ in 0..MAX_PAGES {
        let response = nexus_post_json(
            api_key,
            GRAPHQL_URL,
            &serde_json::json!({
                "query": QUERY,
                "variables": {
                    "filter": { "fileId": filters },
                    "offset": offset,
                    "count": PAGE_SIZE,
                }
            }),
        )
        .await?;
        if response.get("errors").is_some() && response.pointer("/data/modFileContents").is_none() {
            return Err(InstallError::Api(
                "Nexus could not provide the indexed archive contents.".into(),
            ));
        }
        let page = response
            .pointer("/data/modFileContents")
            .ok_or_else(|| InstallError::Api("Invalid Nexus content preview response.".into()))?;
        let nodes = page
            .get("nodes")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        for node in &nodes {
            let Some(file_id) = json_u32(node.get("fileId")) else {
                continue;
            };
            if !wanted.contains(&file_id) {
                continue;
            }
            let extension = node
                .get("fileExtension")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "pak" | "utoc" | "ucas" | "sig") {
                continue;
            }
            let path = node
                .get("filePath")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .replace('\\', "/");
            let file_name = node
                .get("fileName")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .or_else(|| {
                    Path::new(&path)
                        .file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            if file_name.is_empty() {
                continue;
            }
            result
                .entry(file_id)
                .or_default()
                .push(IndexedArchiveContent {
                    path,
                    file_name,
                    size_bytes: json_u64(node.get("fileSize")),
                });
        }
        let total = json_u64(page.get("totalCount")).unwrap_or(nodes.len() as u64);
        offset = offset.saturating_add(nodes.len() as u64);
        if nodes.is_empty() || offset >= total {
            return Ok(result);
        }
    }
    Err(InstallError::Api(
        "The Nexus content preview is too large to display safely.".into(),
    ))
}

async fn get_mod_info(api_key: &str, mod_id: u32) -> Result<ModInfo, InstallError> {
    if let Some((cached_at, info)) = mod_info_cache()
        .lock()
        .expect("mod info cache mutex poisoned")
        .get(&mod_id)
    {
        if cached_at.elapsed() < MOD_INFO_CACHE_TTL {
            return Ok(info.clone());
        }
    }
    let url = format!("{BASE_URL}/{GAME_DOMAIN}/mods/{mod_id}.json");
    let value = nexus_get_json(api_key, &url).await?;
    let info: ModInfo = serde_json::from_value(value)
        .map_err(|e| InstallError::Api(format!("Invalid mod info: {e}")))?;
    let mut cache = mod_info_cache()
        .lock()
        .expect("mod info cache mutex poisoned");
    prune_timed_cache(&mut cache);
    cache.insert(mod_id, (std::time::Instant::now(), info.clone()));
    Ok(info)
}

async fn get_mod_files(api_key: &str, mod_id: u32) -> Result<Vec<ModFile>, InstallError> {
    if let Some((cached_at, files)) = mod_files_cache()
        .lock()
        .expect("mod files cache mutex poisoned")
        .get(&mod_id)
    {
        if cached_at.elapsed() < MOD_INFO_CACHE_TTL {
            return Ok(files.clone());
        }
    }
    let url = format!("{BASE_URL}/{GAME_DOMAIN}/mods/{mod_id}/files.json");
    let value = nexus_get_json(api_key, &url).await?;
    let parsed: FilesResponse = serde_json::from_value(value)
        .map_err(|e| InstallError::Api(format!("Invalid files: {e}")))?;
    let mut cache = mod_files_cache()
        .lock()
        .expect("mod files cache mutex poisoned");
    prune_timed_cache(&mut cache);
    cache.insert(mod_id, (std::time::Instant::now(), parsed.files.clone()));
    Ok(parsed.files)
}

fn json_u32(value: Option<&serde_json::Value>) -> Option<u32> {
    value.and_then(|value| {
        value
            .as_u64()
            .map(|number| number as u32)
            .or_else(|| value.as_str()?.parse().ok())
    })
}

fn archive_md5(path: &Path) -> Result<String, InstallError> {
    let metadata = path.metadata().map_err(|e| {
        InstallError::ArchiveMismatch(format!("Could not inspect the selected archive: {e}"))
    })?;
    let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
    if let Some((cached_modified, cached_size, md5)) = archive_md5_cache()
        .lock()
        .expect("archive MD5 cache mutex poisoned")
        .get(path)
    {
        if *cached_modified == modified && *cached_size == metadata.len() {
            return Ok(md5.clone());
        }
    }
    let mut file = std::fs::File::open(path).map_err(|e| {
        InstallError::ArchiveMismatch(format!("Could not read the selected archive: {e}"))
    })?;
    let mut hasher = Md5::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| {
            InstallError::ArchiveMismatch(format!("Could not read the selected archive: {e}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let md5 = format!("{:x}", hasher.finalize());
    let mut cache = archive_md5_cache()
        .lock()
        .expect("archive MD5 cache mutex poisoned");
    if cache.len() >= 128 {
        cache.clear();
    }
    cache.insert(path.to_path_buf(), (modified, metadata.len(), md5.clone()));
    Ok(md5)
}

async fn verify_archive_identity(
    api_key: &str,
    mod_id: u32,
    archive_path: &Path,
    expected_file_ids: Option<&[u32]>,
) -> Result<ArchiveIdentity, InstallError> {
    let md5 = archive_md5(archive_path)?;
    let url = format!("{BASE_URL}/{GAME_DOMAIN}/mods/md5_search/{md5}.json");
    let value = nexus_get_json(api_key, &url).await?;

    if let Some(identity) = extract_archive_identity(&value, mod_id, &md5) {
        return Ok(identity);
    }

    // Nexus' global MD5 index can lag behind or omit valid files. The mod's
    // own file list is authoritative and provides a reliable fallback.
    let files = get_mod_files(api_key, mod_id).await?;
    files
        .into_iter()
        .find(|file| {
            file.md5.as_deref().is_some_and(|candidate| candidate.eq_ignore_ascii_case(&md5))
                && expected_file_ids
                    .map(|expected| expected.is_empty() || expected.contains(&file.file_id))
                    .unwrap_or(true)
        })
        .map(|file| ArchiveIdentity {
            file_id: Some(file.file_id),
            file_name: file.file_name,
            md5,
        })
        .ok_or_else(|| {
            InstallError::ArchiveMismatch(format!(
                "The selected archive is not a Nexus Mods file for mod {mod_id}. Choose the file downloaded from that mod's Files page."
            ))
        })
}

fn extract_archive_identity(
    value: &serde_json::Value,
    mod_id: u32,
    md5: &str,
) -> Option<ArchiveIdentity> {
    let matches = value
        .as_array()
        .cloned()
        .or_else(|| value.get("results").and_then(|v| v.as_array()).cloned())
        .unwrap_or_default();

    for item in matches {
        let matched_mod_id = json_u32(
            item.get("mod")
                .and_then(|mod_info| mod_info.get("mod_id"))
                .or_else(|| item.get("mod_id")),
        );
        if matched_mod_id != Some(mod_id) {
            continue;
        }
        let details = item.get("file_details").unwrap_or(&item);
        return Some(ArchiveIdentity {
            file_id: json_u32(details.get("file_id")),
            file_name: details
                .get("file_name")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            md5: md5.to_string(),
        });
    }
    None
}

async fn get_download_links(
    api_key: &str,
    mod_id: u32,
    file_id: u32,
) -> Result<Vec<DownloadLink>, InstallError> {
    let url = format!("{BASE_URL}/{GAME_DOMAIN}/mods/{mod_id}/files/{file_id}/download_link.json");
    let value = nexus_get_json(api_key, &url).await?;
    serde_json::from_value(value)
        .map_err(|e| InstallError::Api(format!("Invalid download links: {e}")))
}

fn pick_file(files: &[ModFile]) -> Option<&ModFile> {
    files
        .iter()
        .find(|f| {
            f.category_name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("main"))
                .unwrap_or(false)
        })
        .or_else(|| files.first())
}

fn explicit_part(file: &ModFile) -> Option<(String, u32)> {
    let source = file
        .name
        .as_deref()
        .or(file.file_name.as_deref())
        .unwrap_or_default();
    let stem = Path::new(source)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(source);
    let tokens: Vec<String> = stem
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    for (index, token) in tokens.iter().enumerate() {
        let (part_number, consumed) = if token == "part" || token == "pt" {
            (tokens.get(index + 1)?.parse::<u32>().ok()?, 2)
        } else if let Some(number) = token
            .strip_prefix("part")
            .or_else(|| token.strip_prefix("pt"))
            .and_then(|value| value.parse::<u32>().ok())
        {
            (number, 1)
        } else {
            continue;
        };
        if part_number == 0 {
            continue;
        }
        let mut kept = Vec::new();
        for (token_index, value) in tokens.iter().enumerate() {
            if (index..index + consumed).contains(&token_index) {
                continue;
            }
            if token_index == index + consumed && value == "of" {
                continue;
            }
            if token_index == index + consumed + 1
                && tokens
                    .get(index + consumed)
                    .is_some_and(|value| value == "of")
                && value.parse::<u32>().is_ok()
            {
                continue;
            }
            kept.push(value.clone());
        }
        let base = kept.join(" ");
        if !base.is_empty() {
            return Some((base, part_number));
        }
    }
    None
}

fn file_option(file: &ModFile, part_number: Option<u32>) -> ModInstallFileOption {
    let file_name = file
        .file_name
        .clone()
        .unwrap_or_else(|| format!("Nexus file {}", file.file_id));
    ModInstallFileOption {
        file_id: file.file_id,
        display_name: file.name.clone().unwrap_or_else(|| file_name.clone()),
        file_name,
        version: file.version.clone(),
        category_name: file.category_name.clone(),
        size_bytes: file.size_kb.map(|size| size.saturating_mul(1024)),
        part_number,
        contents: Vec::new(),
    }
}

fn install_options(files: &[ModFile]) -> Vec<ModInstallOption> {
    let recommended_file_id = pick_file(files).map(|file| file.file_id);
    let usable: Vec<&ModFile> = {
        let main: Vec<&ModFile> = files
            .iter()
            .filter(|file| {
                file.category_name
                    .as_deref()
                    .is_some_and(|category| category.eq_ignore_ascii_case("main"))
            })
            .collect();
        if main.is_empty() {
            files
                .iter()
                .filter(|file| {
                    !file.category_name.as_deref().is_some_and(|category| {
                        category.to_ascii_lowercase().contains("old")
                            || category.to_ascii_lowercase().contains("archiv")
                    })
                })
                .collect()
        } else {
            main
        }
    };

    let mut grouped: HashMap<String, Vec<(u32, &ModFile)>> = HashMap::new();
    let mut singles = Vec::new();
    for file in usable {
        if let Some((base, part_number)) = explicit_part(file) {
            grouped.entry(base).or_default().push((part_number, file));
        } else {
            singles.push(file);
        }
    }

    let mut options = Vec::new();
    for (base, mut parts) in grouped {
        parts.sort_by_key(|(part, _)| *part);
        let continuous = parts.len() > 1
            && parts
                .iter()
                .enumerate()
                .all(|(index, (part, _))| *part == index as u32 + 1);
        if !continuous {
            singles.extend(parts.into_iter().map(|(_, file)| file));
            continue;
        }
        let files: Vec<ModInstallFileOption> = parts
            .iter()
            .map(|(part, file)| file_option(file, Some(*part)))
            .collect();
        let total_size_bytes = files.iter().try_fold(0_u64, |total, file| {
            Some(total.saturating_add(file.size_bytes?))
        });
        let recommended = recommended_file_id
            .is_some_and(|file_id| files.iter().any(|file| file.file_id == file_id));
        options.push(ModInstallOption {
            id: files
                .iter()
                .map(|file| file.file_id.to_string())
                .collect::<Vec<_>>()
                .join("-"),
            label: format!("{} ({} parts)", base, files.len()),
            multipart: true,
            recommended,
            total_size_bytes,
            content_preview_available: false,
            predicted_adds: 0,
            predicted_replaces: 0,
            predicted_removes: 0,
            predicted_blocked_files: 0,
            predicted_removed_files: Vec::new(),
            files,
        });
    }
    for file in singles {
        let option = file_option(file, None);
        options.push(ModInstallOption {
            id: option.file_id.to_string(),
            label: option.display_name.clone(),
            multipart: false,
            recommended: recommended_file_id == Some(option.file_id),
            total_size_bytes: option.size_bytes,
            content_preview_available: false,
            predicted_adds: 0,
            predicted_replaces: 0,
            predicted_removes: 0,
            predicted_blocked_files: 0,
            predicted_removed_files: Vec::new(),
            files: vec![option],
        });
    }
    options.sort_by(|left, right| {
        right
            .recommended
            .cmp(&left.recommended)
            .then_with(|| right.multipart.cmp(&left.multipart))
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });
    options
}

fn enrich_install_options(
    app: &AppHandle,
    mod_id: u32,
    options: &mut [ModInstallOption],
    indexed: &HashMap<u32, Vec<IndexedArchiveContent>>,
) -> Result<(), InstallError> {
    let config_dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    let installed = read_installed_list(&config_dir.join(INSTALLED_FILE))?;
    let current = installed.iter().find(|item| item.mod_id == mod_id);
    let target = game::load_location(app)
        .ok()
        .flatten()
        .map(|location| mods_dir(&location.path));
    let claimed_elsewhere: std::collections::HashSet<String> = installed
        .iter()
        .filter(|item| item.mod_id != mod_id)
        .flat_map(|item| item.files.iter())
        .map(|file| path_key(Path::new(file)))
        .collect();

    for option in options {
        let mut destinations = std::collections::HashSet::new();
        let mut any_contents = false;
        for file in &mut option.files {
            let mut contents = Vec::new();
            for indexed_file in indexed.get(&file.file_id).into_iter().flatten() {
                any_contents = true;
                let destination = target
                    .as_ref()
                    .map(|target| target.join(&indexed_file.file_name));
                let destination_key = destination
                    .as_deref()
                    .map(path_key)
                    .unwrap_or_else(|| indexed_file.file_name.to_ascii_lowercase());
                let duplicate_in_group = !destinations.insert(destination_key.clone());
                let owner = installed.iter().find(|item| {
                    item.mod_id != mod_id
                        && item
                            .files
                            .iter()
                            .any(|owned| path_key(Path::new(owned)) == destination_key)
                });
                let current_owns = current.is_some_and(|item| {
                    item.files
                        .iter()
                        .any(|owned| path_key(Path::new(owned)) == destination_key)
                });
                let unmanaged_exists = destination.as_ref().is_some_and(|destination| {
                    (destination.exists() || disabled_path(destination).exists()) && !current_owns
                });
                let action = if duplicate_in_group || owner.is_some() || unmanaged_exists {
                    option.predicted_blocked_files += 1;
                    InstallPreviewAction::Blocked
                } else if current_owns {
                    option.predicted_replaces += 1;
                    InstallPreviewAction::Replace
                } else {
                    option.predicted_adds += 1;
                    InstallPreviewAction::Add
                };
                let owner_name = if duplicate_in_group {
                    Some("Another selected part".into())
                } else {
                    owner
                        .map(|item| item.name.clone())
                        .or_else(|| unmanaged_exists.then(|| "Unmanaged file".into()))
                };
                contents.push(ModInstallContentFile {
                    path: indexed_file.path.clone(),
                    file_name: indexed_file.file_name.clone(),
                    size_bytes: indexed_file.size_bytes,
                    action,
                    owner_mod_id: owner.map(|item| item.mod_id),
                    owner_name,
                });
            }
            contents
                .sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
            contents.dedup_by(|left, right| left.path.eq_ignore_ascii_case(&right.path));
            file.contents = contents;
        }
        option.content_preview_available = any_contents;
        if let (Some(current), Some(_)) = (current, target.as_ref()) {
            for existing in &current.files {
                let key = path_key(Path::new(existing));
                if !destinations.contains(&key) && !claimed_elsewhere.contains(&key) {
                    option.predicted_removes += 1;
                    option.predicted_removed_files.push(
                        Path::new(existing)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
            option
                .predicted_removed_files
                .sort_by_key(|name| name.to_lowercase());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_mod_install_options(
    app: AppHandle,
    state: State<'_, AuthState>,
    api_key: String,
    mod_id: u32,
) -> Result<Vec<ModInstallOption>, InstallError> {
    let api_key = resolve_api_key(&app, &state, &api_key)?;
    let files = get_mod_files(&api_key, mod_id).await?;
    let mut options = install_options(&files);
    if options.is_empty() {
        Err(InstallError::NoFiles)
    } else {
        let file_ids: Vec<u32> = options
            .iter()
            .flat_map(|option| option.files.iter().map(|file| file.file_id))
            .collect();
        if let Ok(indexed) = get_indexed_archive_contents(&api_key, &file_ids).await {
            enrich_install_options(&app, mod_id, &mut options, &indexed)?;
        }
        Ok(options)
    }
}

fn next_install_plan_id(mod_id: u32) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = INSTALL_PLAN_ID.fetch_add(1, Ordering::Relaxed);
    format!("{mod_id}-{timestamp}-{sequence}")
}

#[cfg(windows)]
fn available_disk_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0_u64;
    unsafe {
        GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None).ok()?;
    }
    Some(available)
}

#[cfg(not(windows))]
fn available_disk_space(_path: &Path) -> Option<u64> {
    None
}

struct InstallPreviewAnalysis<'a> {
    archive_verified: &'a [bool],
    asset_conflicts: asset_conflicts::PreviewAssetConflictReport,
}

fn build_install_preview(
    app: &AppHandle,
    plan_id: String,
    mod_id: u32,
    plan: &[(PathBuf, PathBuf)],
    identities: &[ArchiveIdentity],
    info: &ModInfo,
    analysis: InstallPreviewAnalysis<'_>,
) -> Result<(InstallPreview, String), InstallError> {
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    let installed = read_installed_list(&dir.join(INSTALLED_FILE))?;
    let inventory_fingerprint = installed_list_fingerprint(&installed)?;
    let current = installed.iter().find(|item| item.mod_id == mod_id);
    let destinations: std::collections::HashSet<String> = plan
        .iter()
        .map(|(_, destination)| path_key(destination))
        .collect();

    let mut files = Vec::new();
    let mut required_bytes = 0_u64;
    let mut adds = 0;
    let mut replaces = 0;
    let mut removes = 0;
    let mut blocked_files = 0;

    for (source, destination) in plan {
        let size_bytes = source
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        required_bytes = required_bytes.saturating_add(size_bytes);
        let key = path_key(destination);
        let owner = installed.iter().find(|item| {
            item.mod_id != mod_id
                && item
                    .files
                    .iter()
                    .any(|file| path_key(Path::new(file)) == key)
        });
        let current_owns = current.is_some_and(|item| {
            item.files
                .iter()
                .any(|file| path_key(Path::new(file)) == key)
        });
        let unmanaged_exists =
            (destination.exists() || disabled_path(destination).exists()) && !current_owns;

        let action = if owner.is_some() || unmanaged_exists {
            blocked_files += 1;
            InstallPreviewAction::Blocked
        } else if current_owns {
            replaces += 1;
            InstallPreviewAction::Replace
        } else {
            adds += 1;
            InstallPreviewAction::Add
        };

        files.push(InstallPreviewFile {
            name: destination
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            size_bytes,
            action,
            owner_mod_id: owner.map(|item| item.mod_id),
            owner_name: owner
                .map(|item| item.name.clone())
                .or_else(|| unmanaged_exists.then(|| "Unmanaged file".to_string())),
        });
    }

    if let Some(current) = current {
        let claimed_elsewhere: std::collections::HashSet<String> = installed
            .iter()
            .filter(|item| item.mod_id != mod_id)
            .flat_map(|item| item.files.iter())
            .map(|file| path_key(Path::new(file)))
            .collect();
        for stale in &current.files {
            let stale_path = Path::new(stale);
            let key = path_key(stale_path);
            if destinations.contains(&key) || claimed_elsewhere.contains(&key) {
                continue;
            }
            let physical = if stale_path.exists() {
                stale_path.to_path_buf()
            } else {
                disabled_path(stale_path)
            };
            let size_bytes = physical
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            removes += 1;
            files.push(InstallPreviewFile {
                name: stale_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                size_bytes,
                action: InstallPreviewAction::Remove,
                owner_mod_id: Some(mod_id),
                owner_name: Some(current.name.clone()),
            });
        }
    }

    files.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    let available_bytes = plan
        .first()
        .and_then(|(_, destination)| destination.parent())
        .and_then(available_disk_space);
    let enough_space = available_bytes
        .is_none_or(|available| available >= required_bytes.saturating_add(DISK_SPACE_MARGIN));
    Ok((
        InstallPreview {
            plan_id,
            mod_id,
            mod_name: info.name.clone(),
            version: info.version.clone(),
            archive_name: identities
                .first()
                .and_then(|identity| identity.file_name.clone())
                .unwrap_or_else(|| format!("Mod {mod_id} archive")),
            archive_md5: identities
                .first()
                .map(|identity| identity.md5.clone())
                .unwrap_or_default(),
            archive_verified: !analysis.archive_verified.is_empty()
                && analysis.archive_verified.iter().all(|verified| *verified),
            required_bytes,
            available_bytes,
            enough_space,
            adds,
            replaces,
            removes,
            blocked_files,
            asset_conflicts: analysis.asset_conflicts,
            can_install: blocked_files == 0 && enough_space,
            archives: identities
                .iter()
                .enumerate()
                .map(|(index, identity)| InstallPreviewArchive {
                    file_id: identity.file_id,
                    file_name: identity
                        .file_name
                        .clone()
                        .unwrap_or_else(|| format!("Archive {}", index + 1)),
                    md5: identity.md5.clone(),
                    verified: analysis
                        .archive_verified
                        .get(index)
                        .copied()
                        .unwrap_or(false),
                })
                .collect(),
            files,
        },
        inventory_fingerprint,
    ))
}

fn installed_list_fingerprint(installed: &[InstalledMod]) -> Result<String, InstallError> {
    let bytes = serde_json::to_vec(installed).map_err(|error| {
        InstallError::Storage(format!("Could not inspect installed mods: {error}"))
    })?;
    let mut hasher = Md5::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn capture_preview_paths(
    app: &AppHandle,
    mod_id: u32,
    plan: &[(PathBuf, PathBuf)],
) -> Result<Vec<PreviewPathState>, InstallError> {
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    let installed = read_installed_list(&dir.join(INSTALLED_FILE))?;
    let mut paths: Vec<PathBuf> = plan
        .iter()
        .map(|(_, destination)| destination.clone())
        .chain(
            installed
                .iter()
                .filter(|item| item.mod_id == mod_id)
                .flat_map(|item| item.files.iter())
                .map(PathBuf::from),
        )
        .collect();
    paths.sort_by_key(|path| path_key(path));
    paths.dedup_by(|left, right| path_key(left) == path_key(right));
    Ok(paths
        .into_iter()
        .map(|path| PreviewPathState {
            active: path.exists(),
            disabled: disabled_path(&path).exists(),
            path: path.to_string_lossy().into_owned(),
        })
        .collect())
}

fn modified_at_millis(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

async fn prepare_archive_install(
    app: &AppHandle,
    conflict_state: &AssetConflictState,
    api_key: &str,
    request: PrepareArchiveRequest,
) -> Result<PreparedInstall, InstallError> {
    if request.source_archives.is_empty() || request.source_archives.len() > 8 {
        return Err(InstallError::Install(
            "Select between one and eight archives for a grouped install.".into(),
        ));
    }
    emit(app, "verifying_archive");
    let identities = match request.verified_identities {
        Some(identities) if identities.len() == request.source_archives.len() => identities,
        Some(_) => {
            return Err(InstallError::Storage(
                "Grouped archive verification data is incomplete.".into(),
            ));
        }
        None => {
            let mut identities = Vec::with_capacity(request.source_archives.len());
            for archive in &request.source_archives {
                identities.push(
                    verify_archive_identity(
                        api_key,
                        request.mod_id,
                        archive,
                        request.expected_file_ids.as_deref(),
                    )
                    .await?,
                );
            }
            identities
        }
    };
    let mut seen_file_ids = std::collections::HashSet::new();
    for identity in &identities {
        if identity
            .file_id
            .is_some_and(|file_id| !seen_file_ids.insert(file_id))
        {
            return Err(InstallError::ArchiveMismatch(
                "The same Nexus file was selected more than once.".into(),
            ));
        }
    }
    if let Some(expected) = &request.expected_file_ids {
        let expected: std::collections::HashSet<u32> = expected.iter().copied().collect();
        let actual: std::collections::HashSet<u32> = identities
            .iter()
            .filter_map(|identity| identity.file_id)
            .collect();
        if expected != actual {
            return Err(InstallError::ArchiveMismatch(
                "The selected archives do not match the Nexus file group shown in the download plan."
                    .into(),
            ));
        }
    }

    emit(app, "extracting");
    let target = mods_dir(&request.game_path);
    let extraction_app = app.clone();
    let source_archives = request.source_archives.clone();
    let extraction_root = request.temp_dir.join("extracted");
    let mod_id = request.mod_id;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let mut plan = Vec::new();
        let mut destination_names = std::collections::HashSet::new();
        for (index, archive) in source_archives.iter().enumerate() {
            let extract_dir = extraction_root.join(index.to_string());
            std::fs::create_dir_all(&extract_dir).map_err(|error| {
                InstallError::Storage(format!("Could not create temp folder: {error}"))
            })?;
            utoc::extract_archive(archive, &extract_dir).map_err(map_utoc)?;
            emit(&extraction_app, "locating_paks");
            let mut paks = Vec::new();
            find_paks(&extract_dir, &mut paks)?;
            if paks.is_empty() {
                return Err(InstallError::NoPaks);
            }
            let partial = build_copy_plan(&paks, &extract_dir, &target, mod_id);
            for (_, destination) in &partial {
                let destination_name = path_key(destination);
                if !destination_names.insert(destination_name) {
                    return Err(InstallError::Install(format!(
                        "Multiple selected parts contain '{}'. Install the archives separately or choose a different file group.",
                        destination.file_name().unwrap_or_default().to_string_lossy()
                    )));
                }
            }
            plan.extend(partial);
        }
        Ok(plan)
    })
    .await
    .map_err(|error| {
        InstallError::Install(format!("Archive extraction worker stopped: {error}"))
    })??;
    let info = get_mod_info(api_key, request.mod_id)
        .await
        .unwrap_or_else(|_| ModInfo {
            name: format!("Mod {}", request.mod_id),
            version: String::new(),
            picture_url: None,
        });
    let plan_id = next_install_plan_id(request.mod_id);
    let staged_files: Vec<PathBuf> = plan.iter().map(|(source, _)| source.clone()).collect();
    emit(app, "scanning_assets");
    let scan_app = app.clone();
    let scan_state = conflict_state.clone();
    let scan_files = staged_files.clone();
    let candidate_mod_id = request.mod_id;
    let asset_conflicts = tauri::async_runtime::spawn_blocking(move || {
        asset_conflicts::preview_asset_conflicts(
            &scan_app,
            &scan_state,
            candidate_mod_id,
            &scan_files,
        )
    })
    .await
    .map_err(|error| InstallError::Install(format!("Asset scanner stopped: {error}")))?;
    let (preview, inventory_fingerprint) = build_install_preview(
        app,
        plan_id,
        request.mod_id,
        &plan,
        &identities,
        &info,
        InstallPreviewAnalysis {
            archive_verified: &request.archive_verified,
            asset_conflicts,
        },
    )?;
    let path_snapshot = capture_preview_paths(app, request.mod_id, &plan)?;

    Ok(PreparedInstall {
        preview,
        created_at: std::time::Instant::now(),
        inventory_fingerprint,
        path_snapshot,
        temp_dir: request.temp_dir,
        source_archives: request.source_archives,
        delete_source_archives: request.delete_source_archives,
        game_path: request.game_path,
        plan,
        identities,
        info,
    })
}

fn store_prepared_install(
    state: &State<'_, InstallState>,
    prepared: PreparedInstall,
) -> Result<InstallPreview, InstallError> {
    let preview = prepared.preview.clone();
    let mut plans = state
        .prepared
        .lock()
        .map_err(|_| InstallError::Storage("Install preview state is unavailable.".into()))?;
    plans.retain(|_, plan| plan.created_at.elapsed() < std::time::Duration::from_secs(30 * 60));
    if plans.len() >= 4 {
        if let Some(oldest) = plans
            .iter()
            .min_by_key(|(_, plan)| plan.created_at)
            .map(|(plan_id, _)| plan_id.clone())
        {
            plans.remove(&oldest);
        }
    }
    plans.insert(preview.plan_id.clone(), prepared);
    Ok(preview)
}

#[tauri::command]
pub async fn prepare_mod_install(
    app: AppHandle,
    auth_state: State<'_, AuthState>,
    install_state: State<'_, InstallState>,
    conflict_state: State<'_, AssetConflictState>,
    mod_id: u32,
    file_ids: Option<Vec<u32>>,
) -> Result<InstallPreview, InstallError> {
    let api_key = resolve_api_key(&app, &auth_state, "")?;
    let location = game::load_location(&app)
        .map_err(|error| InstallError::Storage(error.to_string()))?
        .ok_or(InstallError::GameNotFound)?;
    ensure_setup(&location.path)?;

    let temp_dir = storage::unique_temp_dir(&format!("mrmmr-prepare-{mod_id}"));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|error| InstallError::Storage(format!("Could not create temp folder: {error}")))?;
    let result = async {
        emit(&app, "fetching_files");
        let files = get_mod_files(&api_key, mod_id).await?;
        let selected_ids = file_ids.unwrap_or_else(|| {
            pick_file(&files)
                .map(|file| vec![file.file_id])
                .unwrap_or_default()
        });
        if selected_ids.is_empty() || selected_ids.len() > 8 {
            return Err(InstallError::NoFiles);
        }
        let unique_ids: std::collections::HashSet<u32> = selected_ids.iter().copied().collect();
        if unique_ids.len() != selected_ids.len() {
            return Err(InstallError::Install(
                "A Nexus file can only appear once in an install group.".into(),
            ));
        }
        let selected: Vec<&ModFile> = selected_ids
            .iter()
            .map(|file_id| {
                files
                    .iter()
                    .find(|file| file.file_id == *file_id)
                    .ok_or_else(|| {
                        InstallError::Install(format!(
                            "Nexus file {file_id} is no longer available. Refresh the install plan."
                        ))
                    })
            })
            .collect::<Result<_, _>>()?;
        let mut source_archives = Vec::with_capacity(selected.len());
        let mut identities = Vec::with_capacity(selected.len());
        let mut verified = Vec::with_capacity(selected.len());
        for (index, file) in selected.iter().enumerate() {
            let archive_path = temp_dir.join(format!(
                "download-{index}.{}",
                utoc::archive_extension_from_name(file.file_name.as_deref())
            ));
            emit(&app, "fetching_download_link");
            let uri = get_download_links(&api_key, mod_id, file.file_id)
                .await?
                .first()
                .map(|link| link.uri.clone())
                .ok_or(InstallError::NoDownloadLink)?;
            emit(&app, "downloading_for_preview");
            let downloaded_md5 = utoc::download_file(&uri, &archive_path)
                .await
                .map_err(map_utoc)?;
            if file
                .md5
                .as_deref()
                .is_some_and(|expected| !expected.eq_ignore_ascii_case(&downloaded_md5))
            {
                return Err(InstallError::ArchiveMismatch(
                    "A downloaded archive did not match Nexus Mods metadata.".into(),
                ));
            }
            verified.push(file.md5.is_some());
            identities.push(ArchiveIdentity {
                file_id: Some(file.file_id),
                file_name: file.file_name.clone(),
                md5: downloaded_md5,
            });
            source_archives.push(archive_path);
        }
        prepare_archive_install(
            &app,
            &conflict_state,
            &api_key,
            PrepareArchiveRequest {
                mod_id,
                game_path: location.path,
                temp_dir: temp_dir.clone(),
                source_archives,
                verified_identities: Some(identities),
                archive_verified: verified,
                expected_file_ids: Some(selected_ids),
                delete_source_archives: false,
            },
        )
        .await
    }
    .await;

    match result {
        Ok(prepared) => store_prepared_install(&install_state, prepared),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn prepare_mod_install_from_archive(
    app: AppHandle,
    auth_state: State<'_, AuthState>,
    install_state: State<'_, InstallState>,
    conflict_state: State<'_, AssetConflictState>,
    mod_id: u32,
    archive_paths: Vec<String>,
    file_ids: Option<Vec<u32>>,
) -> Result<InstallPreview, InstallError> {
    let api_key = resolve_api_key(&app, &auth_state, "")?;
    let location = game::load_location(&app)
        .map_err(|error| InstallError::Storage(error.to_string()))?
        .ok_or(InstallError::GameNotFound)?;
    ensure_setup(&location.path)?;
    let temp_dir = storage::unique_temp_dir(&format!("mrmmr-prepare-{mod_id}"));
    let archive_count = archive_paths.len();
    let result = prepare_archive_install(
        &app,
        &conflict_state,
        &api_key,
        PrepareArchiveRequest {
            mod_id,
            game_path: location.path,
            temp_dir: temp_dir.clone(),
            source_archives: archive_paths.into_iter().map(PathBuf::from).collect(),
            verified_identities: None,
            archive_verified: vec![true; archive_count],
            expected_file_ids: file_ids,
            delete_source_archives: true,
        },
    )
    .await;
    match result {
        Ok(prepared) => store_prepared_install(&install_state, prepared),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn discard_mod_install(
    state: State<'_, InstallState>,
    plan_id: String,
) -> Result<(), InstallError> {
    state
        .prepared
        .lock()
        .map_err(|_| InstallError::Storage("Install preview state is unavailable.".into()))?
        .remove(&plan_id);
    Ok(())
}

fn delete_installed_archive(path: &Path, enabled: bool) -> std::io::Result<bool> {
    if !enabled || !utoc::is_archive(path) {
        return Ok(false);
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

/// Remove a mod's previously-installed files and metadata so a reinstall/update
/// starts clean (no stale files with changed names).
fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', std::path::MAIN_SEPARATOR_STR)
        .to_lowercase()
}

fn is_managed_path(path: &Path, target: &Path) -> bool {
    path.file_name().is_some()
        && path
            .parent()
            .is_some_and(|parent| path_key(parent) == path_key(target))
}

fn remove_file_if_present_runtime(path: &Path) -> Result<(), InstallError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            return Err(InstallError::Install(format!(
                "Refusing to remove a non-regular mod file: '{}'.",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(InstallError::Install(format!(
                "Could not inspect '{}': {error}.",
                path.display()
            )));
        }
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InstallError::Install(format!(
            "Could not remove '{}': {error}. Close Marvel Rivals and try again.",
            path.display()
        ))),
    }
}

#[cfg(test)]
fn remove_file_if_present(path: &Path) -> Result<(), InstallError> {
    remove_file_if_present_runtime(path)
}

#[cfg(test)]
fn remove_owned_files(
    files: &[String],
    remaining: &[InstalledMod],
    target: &Path,
) -> Result<(), InstallError> {
    let claimed: std::collections::HashSet<String> = remaining
        .iter()
        .flat_map(|installed| installed.files.iter())
        .map(|file| path_key(Path::new(file)))
        .collect();

    for file in files {
        let file_path = Path::new(file);
        if !is_managed_path(file_path, target) || claimed.contains(&path_key(file_path)) {
            continue;
        }
        remove_file_if_present(file_path)?;
        remove_file_if_present(&disabled_path(file_path))?;
    }
    Ok(())
}

fn plan_previous_backups(
    app: &AppHandle,
    mod_id: u32,
    target: &Path,
    backup_dir: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, InstallError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| InstallError::Storage(format!("Could not resolve config directory: {e}")))?;
    let mut list = read_installed_list(&dir.join(INSTALLED_FILE))?;
    let Some(index) = list.iter().position(|installed| installed.mod_id == mod_id) else {
        return Ok(Vec::new());
    };
    let previous = list.remove(index);
    let claimed: std::collections::HashSet<String> = list
        .iter()
        .flat_map(|installed| installed.files.iter())
        .map(|file| path_key(Path::new(file)))
        .collect();

    let mut backups = Vec::new();
    for file in previous.files {
        let active = PathBuf::from(file);
        if !is_managed_path(&active, target) || claimed.contains(&path_key(&active)) {
            continue;
        }
        for candidate in [active.clone(), disabled_path(&active)] {
            if !candidate.exists() {
                continue;
            }
            let backup = backup_dir.join(backups.len().to_string());
            backups.push((candidate, backup));
        }
    }
    Ok(backups)
}

fn apply_backup_plan(
    backups: &[(PathBuf, PathBuf)],
    backup_dir: &Path,
) -> Result<(), InstallError> {
    if !backups.is_empty() {
        std::fs::create_dir_all(backup_dir).map_err(|error| {
            InstallError::Storage(format!("Could not create rollback folder: {error}"))
        })?;
    }
    let mut completed = Vec::new();
    for (original, backup) in backups {
        if let Err(error) = std::fs::rename(original, backup) {
            let rollback = restore_backups(&completed);
            let detail = rollback
                .err()
                .map(|rollback| {
                    format!(
                        " Rollback also failed: {rollback}. Recovery files were kept at '{}'.",
                        backup_dir.display()
                    )
                })
                .unwrap_or_default();
            return Err(InstallError::Install(format!(
                "Could not prepare existing mod files for a safe change: {error}.{detail}"
            )));
        }
        completed.push((original.clone(), backup.clone()));
    }
    Ok(())
}

fn restore_backups(backups: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let mut failures = Vec::new();
    for (original, backup) in backups.iter().rev() {
        if !backup.exists() && original.exists() {
            continue;
        }
        if let Err(error) = std::fs::rename(backup, original) {
            failures.push(format!("'{}': {error}", original.display()));
        }
    }
    if failures.is_empty() && backups.iter().all(|(original, _)| original.exists()) {
        Ok(())
    } else {
        if failures.is_empty() {
            failures.push("one or more restored files could not be verified".into());
        }
        Err(failures.join(", "))
    }
}

fn finish_rollback(
    backups: &[(PathBuf, PathBuf)],
    backup_dir: &Path,
    context: &str,
) -> Result<(), InstallError> {
    match restore_backups(backups) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(backup_dir);
            Ok(())
        }
        Err(error) => Err(InstallError::Install(format!(
            "{context} Rollback failed: {error}. Recovery files were kept at '{}'.",
            backup_dir.display()
        ))),
    }
}

fn copy_file_new(source: &Path, destination: &Path) -> std::io::Result<u64> {
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    match std::io::copy(&mut input, &mut output) {
        Ok(bytes) => {
            output.sync_all()?;
            Ok(bytes)
        }
        Err(error) => {
            drop(output);
            let _ = std::fs::remove_file(destination);
            Err(error)
        }
    }
}

fn build_copy_plan(
    paks: &[PathBuf],
    extract_dir: &Path,
    target: &Path,
    mod_id: u32,
) -> Vec<(PathBuf, PathBuf)> {
    let mut plan = Vec::new();
    let mut copied_names = std::collections::HashSet::new();
    let mut seen_sources = std::collections::HashSet::new();
    let mut directories: Vec<&Path> = paks
        .iter()
        .map(|pak| pak.parent().unwrap_or(extract_dir))
        .collect();
    directories.sort_unstable();
    directories.dedup();
    for dir in directories {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut sources: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && matches!(
                        path.extension()
                            .and_then(|extension| extension.to_str())
                            .map(str::to_ascii_lowercase)
                            .as_deref(),
                        Some("pak" | "utoc" | "ucas" | "sig")
                    )
            })
            .collect();
        sources.sort();
        for path in sources {
            if !seen_sources.insert(path.clone()) {
                continue;
            }
            let Some(original) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let mut name = original.clone();
            if !copied_names.insert(name.clone()) {
                let mut suffix = 1_u32;
                loop {
                    name = if suffix == 1 {
                        format!("{mod_id}_{original}")
                    } else {
                        format!("{mod_id}_{suffix}_{original}")
                    };
                    if copied_names.insert(name.clone()) {
                        break;
                    }
                    suffix = suffix.saturating_add(1);
                }
            }
            plan.push((path, target.join(name)));
        }
    }
    plan
}

fn ensure_copy_plan_safe(
    app: &AppHandle,
    mod_id: u32,
    target: &Path,
    plan: &[(PathBuf, PathBuf)],
) -> Result<(), InstallError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| InstallError::Storage(format!("Could not resolve config directory: {e}")))?;
    let list = read_installed_list(&dir.join(INSTALLED_FILE))?;
    let current: std::collections::HashSet<String> = list
        .iter()
        .filter(|installed| installed.mod_id == mod_id)
        .flat_map(|installed| installed.files.iter())
        .map(|file| path_key(Path::new(file)))
        .collect();
    let other: std::collections::HashSet<String> = list
        .iter()
        .filter(|installed| installed.mod_id != mod_id)
        .flat_map(|installed| installed.files.iter())
        .map(|file| path_key(Path::new(file)))
        .collect();

    for (_, destination) in plan {
        if !is_managed_path(destination, target) {
            return Err(InstallError::Install(
                "Refusing to write outside the ~mods folder.".into(),
            ));
        }
        let key = path_key(destination);
        if other.contains(&key) {
            return Err(InstallError::Install(format!(
                "{} is already owned by another installed mod.",
                destination
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            )));
        }
        if (destination.exists() || disabled_path(destination).exists()) && !current.contains(&key)
        {
            return Err(InstallError::Install(format!(
                "{} already exists in the ~mods folder. Move it out before installing.",
                destination
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            )));
        }
    }
    Ok(())
}

fn execute_copy_plan(plan: &[(PathBuf, PathBuf)]) -> Result<Vec<String>, InstallError> {
    let mut copied = Vec::new();
    for (source, destination) in plan {
        if let Err(error) = copy_file_new(source, destination) {
            for copied_file in copied.iter().rev() {
                let _ = std::fs::remove_file(copied_file);
            }
            return Err(InstallError::Install(format!(
                "Could not copy {}: {error}",
                destination.display()
            )));
        }
        copied.push(destination.to_string_lossy().into_owned());
    }
    Ok(copied)
}

fn find_paks(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), InstallError> {
    let entries = std::fs::read_dir(dir).map_err(|error| {
        InstallError::Extract(format!("Could not scan '{}': {error}", dir.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            InstallError::Extract(format!("Could not read '{}': {error}", dir.display()))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            InstallError::Extract(format!("Could not inspect '{}': {error}", path.display()))
        })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            find_paks(&path, out)?;
        } else if path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase() == "pak")
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}

fn save_metadata(
    app: &AppHandle,
    mod_id: u32,
    name: &str,
    version: &str,
    picture_url: Option<&str>,
    files: Vec<String>,
    identities: &[ArchiveIdentity],
) -> Result<InstalledMod, InstallError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| InstallError::Storage(format!("Could not resolve config directory: {e}")))?;
    let path = dir.join(INSTALLED_FILE);

    let mut list = read_installed_list(&path)?;

    list.retain(|m| m.mod_id != mod_id);
    let installed = InstalledMod {
        mod_id,
        name: name.to_string(),
        version: version.to_string(),
        files,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        nexus_file_id: identities.first().and_then(|identity| identity.file_id),
        archive_name: identities
            .first()
            .and_then(|identity| identity.file_name.clone()),
        archive_md5: identities.first().map(|identity| identity.md5.clone()),
        parts: identities
            .iter()
            .enumerate()
            .map(|(index, identity)| InstalledPart {
                nexus_file_id: identity.file_id,
                archive_name: identity
                    .file_name
                    .clone()
                    .unwrap_or_else(|| format!("Archive {}", index + 1)),
                archive_md5: identity.md5.clone(),
            })
            .collect(),
        picture_url: picture_url.map(str::to_owned),
        enabled: true,
        missing: false,
    };
    list.push(installed.clone());

    std::fs::create_dir_all(&dir)
        .map_err(|e| InstallError::Storage(format!("Could not create config directory: {e}")))?;
    storage::write_json_atomic(&path, &list)
        .map_err(|e| InstallError::Storage(format!("Could not write installed mods: {e}")))?;
    Ok(installed)
}

fn read_undo_record(app: &AppHandle) -> Result<Option<InstallUndoRecord>, InstallError> {
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    match std::fs::read_to_string(dir.join(UNDO_FILE)) {
        Ok(contents) => serde_json::from_str(&contents)
            .map(Some)
            .map_err(|error| InstallError::Storage(format!("Could not read undo data: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(InstallError::Storage(format!(
            "Could not read undo data: {error}"
        ))),
    }
}

fn backup_records(backups: &[(PathBuf, PathBuf)]) -> Vec<UndoBackup> {
    backups
        .iter()
        .map(|(original, backup)| UndoBackup {
            original: original.to_string_lossy().into_owned(),
            backup: backup.to_string_lossy().into_owned(),
        })
        .collect()
}

fn backup_pairs(backups: &[UndoBackup]) -> Vec<(PathBuf, PathBuf)> {
    backups
        .iter()
        .map(|backup| {
            (
                PathBuf::from(&backup.original),
                PathBuf::from(&backup.backup),
            )
        })
        .collect()
}

fn write_pending_change(app: &AppHandle, pending: &PendingChange) -> Result<(), InstallError> {
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    storage::write_json_atomic(&dir.join(PENDING_CHANGE_FILE), pending)
        .map_err(|error| InstallError::Storage(format!("Could not save recovery data: {error}")))
}

fn read_pending_change(app: &AppHandle) -> Result<Option<PendingChange>, InstallError> {
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    match std::fs::read_to_string(dir.join(PENDING_CHANGE_FILE)) {
        Ok(contents) => serde_json::from_str(&contents).map(Some).map_err(|error| {
            InstallError::Storage(format!("Could not read recovery data: {error}"))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(InstallError::Storage(format!(
            "Could not read recovery data: {error}"
        ))),
    }
}

fn remove_pending_change(app: &AppHandle) -> Result<(), InstallError> {
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    match std::fs::remove_file(dir.join(PENDING_CHANGE_FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InstallError::Storage(format!(
            "Could not clear recovery data: {error}"
        ))),
    }
}

pub fn recover_pending(app: &AppHandle) -> Result<(), InstallError> {
    let Some(pending) = read_pending_change(app)? else {
        return Ok(());
    };
    if pending.transaction_id.is_empty()
        || !pending
            .transaction_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(InstallError::Storage(
            "Recovery data contains an invalid transaction identifier.".into(),
        ));
    }

    let target = mods_dir(&pending.game_path);
    let backups = backup_pairs(&pending.backups);
    for (original, backup) in &backups {
        let backup_parent_valid = backup.parent().is_some_and(|parent| {
            parent
                .parent()
                .is_some_and(|root| path_key(root) == path_key(&target))
                && parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name == format!(".mrmmr-undo-{}", pending.transaction_id)
                            || name == format!(".mrmmr-undo-rollback-{}", pending.transaction_id)
                            || name == format!(".mrmmr-uninstall-{}", pending.transaction_id)
                    })
        });
        if !is_managed_path(original, &target) || !backup_parent_valid {
            return Err(InstallError::Storage(
                "Recovery data contains an unsafe file path.".into(),
            ));
        }
    }
    for file in &pending.new_files {
        if !is_managed_path(Path::new(file), &target) {
            return Err(InstallError::Storage(
                "Recovery data references a file outside the configured ~mods folder.".into(),
            ));
        }
    }

    let config_dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    if pending.state == PendingChangeState::Committed {
        if pending.clear_undo_on_commit {
            clear_undo_record(app, &target)?;
        }
        if !pending.retain_backups_on_commit {
            cleanup_backup_directories(&pending.backups, &target);
        }
        return remove_pending_change(app);
    }

    if pending.state == PendingChangeState::BackedUp {
        for file in &pending.new_files {
            remove_file_if_present_runtime(Path::new(file))?;
            remove_file_if_present_runtime(&disabled_path(Path::new(file)))?;
        }
    }
    let backup_dir = backups
        .first()
        .and_then(|(_, backup)| backup.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| target.join(format!(".mrmmr-undo-{}", pending.transaction_id)));
    finish_rollback(
        &backups,
        &backup_dir,
        "MRMMR found an interrupted mod change.",
    )?;

    storage::write_json_atomic(&config_dir.join(INSTALLED_FILE), &pending.before_inventory)
        .map_err(|error| {
            InstallError::Storage(format!("Could not restore installed-mod metadata: {error}"))
        })?;
    if read_undo_record(app)?.is_some_and(|record| record.transaction_id == pending.transaction_id)
    {
        std::fs::remove_file(config_dir.join(UNDO_FILE)).map_err(|error| {
            InstallError::Storage(format!("Could not clear interrupted undo data: {error}"))
        })?;
    }
    remove_pending_change(app)
}

fn cleanup_backup_directories(backups: &[UndoBackup], target: &Path) {
    let mut directories = std::collections::HashSet::new();
    for backup in backups {
        let path = Path::new(&backup.backup);
        if let Some(parent) = path.parent() {
            let owned = parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(".mrmmr-undo-") || name.starts_with(".mrmmr-uninstall-")
                })
                && parent
                    .parent()
                    .is_some_and(|root| path_key(root) == path_key(target));
            if owned {
                directories.insert(parent.to_path_buf());
            }
        }
    }
    for directory in directories {
        let _ = std::fs::remove_dir_all(directory);
    }
}

fn cleanup_undo_backups(record: &InstallUndoRecord, target: &Path) {
    cleanup_backup_directories(&record.backups, target);
}

fn validate_undo_record(record: &InstallUndoRecord, target: &Path) -> Result<(), InstallError> {
    if record.transaction_id.is_empty()
        || !record
            .transaction_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(InstallError::Storage(
            "Undo data contains an invalid transaction identifier.".into(),
        ));
    }
    let expected_backup_dir = target.join(format!(".mrmmr-undo-{}", record.transaction_id));
    for file in &record.installed.files {
        if !is_managed_path(Path::new(file), target) {
            return Err(InstallError::Storage(
                "Undo data references a file outside the configured ~mods folder.".into(),
            ));
        }
    }
    for fingerprint in &record.after_files {
        if !is_managed_path(Path::new(&fingerprint.path), target) {
            return Err(InstallError::Storage(
                "Undo data contains an unsafe installed-file path.".into(),
            ));
        }
    }
    for backup in &record.backups {
        let original = Path::new(&backup.original);
        let backup_path = Path::new(&backup.backup);
        if !is_managed_path(original, target)
            || backup_path
                .parent()
                .is_none_or(|parent| path_key(parent) != path_key(&expected_backup_dir))
        {
            return Err(InstallError::Storage(
                "Undo data contains an unsafe recovery path.".into(),
            ));
        }
        let metadata = std::fs::symlink_metadata(backup_path).map_err(|_| {
            InstallError::Install(
                "The previous-version backup is incomplete, so undo was stopped.".into(),
            )
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(InstallError::Storage(
                "Undo recovery data is not a regular file.".into(),
            ));
        }
    }
    Ok(())
}

fn clear_undo_record(app: &AppHandle, target: &Path) -> Result<(), InstallError> {
    let record = read_undo_record(app)?;
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    let removed = match std::fs::remove_file(dir.join(UNDO_FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(InstallError::Storage(format!(
            "Could not clear undo data: {error}"
        ))),
    };
    removed?;
    if let Some(record) = record {
        cleanup_undo_backups(&record, target);
    }
    Ok(())
}

fn commit_prepared_install(
    app: &AppHandle,
    prepared: &PreparedInstall,
) -> Result<InstalledMod, InstallError> {
    if !prepared.preview.can_install {
        return Err(InstallError::Install(
            "Resolve the blocked container files in the installation preview before installing."
                .into(),
        ));
    }

    let current_location = game::load_location(app)
        .map_err(|error| InstallError::Storage(error.to_string()))?
        .ok_or(InstallError::GameNotFound)?;
    if path_key(Path::new(&current_location.path)) != path_key(Path::new(&prepared.game_path)) {
        return Err(InstallError::Install(
            "The Marvel Rivals location changed after this preview. Prepare the mod again.".into(),
        ));
    }
    let target = mods_dir(&prepared.game_path);
    std::fs::create_dir_all(&target).map_err(|error| {
        InstallError::Install(format!("Could not create the ~mods folder: {error}"))
    })?;
    let config_dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    let installed_path = config_dir.join(INSTALLED_FILE);
    let before_list = read_installed_list(&installed_path)?;
    if installed_list_fingerprint(&before_list)? != prepared.inventory_fingerprint {
        return Err(InstallError::Install(
            "Installed mods changed after this preview. Prepare the mod again.".into(),
        ));
    }
    if capture_preview_paths(app, prepared.preview.mod_id, &prepared.plan)?
        != prepared.path_snapshot
    {
        return Err(InstallError::Install(
            "Mod files changed after this preview. Prepare the mod again.".into(),
        ));
    }
    if available_disk_space(&target).is_some_and(|available| {
        available
            < prepared
                .preview
                .required_bytes
                .saturating_add(DISK_SPACE_MARGIN)
    }) {
        return Err(InstallError::Install(
            "The game drive no longer has enough free space. Free some space and prepare the mod again."
                .into(),
        ));
    }
    ensure_copy_plan_safe(app, prepared.preview.mod_id, &target, &prepared.plan)?;
    let previous = before_list
        .iter()
        .find(|item| item.mod_id == prepared.preview.mod_id)
        .cloned();
    let previous_undo = read_undo_record(app)?;
    let backup_dir = target.join(format!(".mrmmr-undo-{}", prepared.preview.plan_id));
    let backups = plan_previous_backups(app, prepared.preview.mod_id, &target, &backup_dir)?;
    let mut pending = PendingChange {
        transaction_id: prepared.preview.plan_id.clone(),
        game_path: prepared.game_path.clone(),
        state: PendingChangeState::Prepared,
        before_inventory: before_list.clone(),
        backups: backup_records(&backups),
        new_files: prepared
            .plan
            .iter()
            .map(|(_, destination)| destination.to_string_lossy().into_owned())
            .collect(),
        retain_backups_on_commit: true,
        clear_undo_on_commit: false,
    };
    write_pending_change(app, &pending)?;
    if let Err(error) = apply_backup_plan(&backups, &backup_dir) {
        let _ = remove_pending_change(app);
        return Err(error);
    }
    pending.state = PendingChangeState::BackedUp;
    if let Err(error) = write_pending_change(app, &pending) {
        finish_rollback(
            &backups,
            &backup_dir,
            "Could not advance install recovery data.",
        )?;
        remove_pending_change(app)?;
        return Err(error);
    }

    emit(app, "copying");
    let copied = match execute_copy_plan(&prepared.plan) {
        Ok(copied) => copied,
        Err(error) => {
            finish_rollback(&backups, &backup_dir, "Installation failed.")?;
            remove_pending_change(app)?;
            return Err(error);
        }
    };

    emit(app, "saving");
    let installed = match save_metadata(
        app,
        prepared.preview.mod_id,
        &prepared.info.name,
        &prepared.info.version,
        prepared.info.picture_url.as_deref(),
        copied.clone(),
        &prepared.identities,
    ) {
        Ok(installed) => installed,
        Err(error) => {
            for file in &copied {
                let _ = std::fs::remove_file(file);
            }
            finish_rollback(&backups, &backup_dir, "Metadata save failed.")?;
            remove_pending_change(app)?;
            return Err(error);
        }
    };

    let after_files: Vec<UndoFingerprint> = copied
        .iter()
        .filter_map(|file| {
            std::fs::metadata(file)
                .ok()
                .map(|metadata| UndoFingerprint {
                    path: file.clone(),
                    size_bytes: metadata.len(),
                    modified_at_millis: modified_at_millis(&metadata),
                })
        })
        .collect();
    if after_files.len() != copied.len() {
        for file in &copied {
            let _ = std::fs::remove_file(file);
        }
        let metadata_result = storage::write_json_atomic(&installed_path, &before_list);
        finish_rollback(&backups, &backup_dir, "Undo-data preparation failed.")?;
        remove_pending_change(app)?;
        if let Err(error) = metadata_result {
            return Err(InstallError::Storage(format!(
                "Could not restore installed-mod metadata: {error}"
            )));
        }
        return Err(InstallError::Storage(
            "Could not record installed files for undo.".into(),
        ));
    }

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let operation = if previous.is_some() {
        "Update"
    } else {
        "Install"
    };
    let record = InstallUndoRecord {
        transaction_id: prepared.preview.plan_id.clone(),
        created_at,
        label: format!("{operation} {}", installed.name),
        mod_id: prepared.preview.mod_id,
        game_path: prepared.game_path.clone(),
        previous,
        installed: installed.clone(),
        backups: backup_records(&backups),
        after_files,
    };
    if let Err(error) = storage::write_json_atomic(&config_dir.join(UNDO_FILE), &record) {
        for file in &copied {
            let _ = std::fs::remove_file(file);
        }
        let metadata_result = storage::write_json_atomic(&installed_path, &before_list);
        finish_rollback(&backups, &backup_dir, "Undo-data save failed.")?;
        remove_pending_change(app)?;
        let detail = metadata_result
            .err()
            .map(|rollback| format!(" Metadata rollback also failed: {rollback}"))
            .unwrap_or_default();
        return Err(InstallError::Storage(format!(
            "Could not save undo data: {error}.{detail}"
        )));
    }
    pending.state = PendingChangeState::Committed;
    write_pending_change(app, &pending)?;
    remove_pending_change(app)?;
    if let Some(old) = previous_undo {
        cleanup_undo_backups(&old, &target);
    }
    emit(app, "done");
    Ok(installed)
}

#[tauri::command]
pub fn commit_mod_install(
    app: AppHandle,
    state: State<'_, InstallState>,
    conflict_state: State<'_, AssetConflictState>,
    plan_id: String,
) -> Result<InstalledMod, InstallError> {
    ensure_game_files_mutable()?;
    let prepared = state
        .prepared
        .lock()
        .map_err(|_| InstallError::Storage("Install preview state is unavailable.".into()))?
        .remove(&plan_id)
        .ok_or_else(|| {
            InstallError::Install(
                "This installation preview expired. Prepare the mod again.".into(),
            )
        })?;
    if prepared.created_at.elapsed() >= std::time::Duration::from_secs(30 * 60) {
        return Err(InstallError::Install(
            "This installation preview expired. Prepare the mod again.".into(),
        ));
    }
    let _mutation = state
        .mutation
        .lock()
        .map_err(|_| InstallError::Storage("Mod changes are temporarily unavailable.".into()))?;
    recover_pending(&app)?;
    let installed = commit_prepared_install(&app, &prepared)?;
    if prepared.delete_source_archives {
        let cleanup_enabled = preferences::load(&app)
            .map(|preferences| preferences.auto_delete_mod_archives)
            .unwrap_or(false);
        for archive in &prepared.source_archives {
            if let Err(error) = delete_installed_archive(archive, cleanup_enabled) {
                eprintln!(
                    "[install] mod installed, but archive cleanup failed for '{}': {error}",
                    archive.display()
                );
            }
        }
    }
    asset_conflicts::refresh_mod_best_effort(&app, &conflict_state, &installed);
    Ok(installed)
}

#[tauri::command]
pub fn get_last_install_change(app: AppHandle) -> Result<UndoStatus, InstallError> {
    let record = read_undo_record(&app)?;
    Ok(UndoStatus {
        available: record.is_some(),
        label: record.as_ref().map(|record| record.label.clone()),
        created_at: record.as_ref().map(|record| record.created_at),
    })
}

#[tauri::command]
pub fn undo_last_install_change(
    app: AppHandle,
    state: State<'_, InstallState>,
    conflict_state: State<'_, AssetConflictState>,
) -> Result<Vec<InstalledMod>, InstallError> {
    ensure_game_files_mutable()?;
    let _mutation = state
        .mutation
        .lock()
        .map_err(|_| InstallError::Storage("Mod changes are temporarily unavailable.".into()))?;
    recover_pending(&app)?;
    let record = read_undo_record(&app)?
        .ok_or_else(|| InstallError::Install("There is no install or update to undo.".into()))?;
    let location = game::load_location(&app)
        .map_err(|error| InstallError::Storage(error.to_string()))?
        .ok_or(InstallError::GameNotFound)?;
    if path_key(Path::new(&location.path)) != path_key(Path::new(&record.game_path)) {
        return Err(InstallError::Install(
            "The game location changed after this operation, so undo was stopped.".into(),
        ));
    }
    let target = mods_dir(&location.path);
    validate_undo_record(&record, &target)?;
    let config_dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    let installed_path = config_dir.join(INSTALLED_FILE);
    let current = read_installed_list(&installed_path)?;
    let current_entry = current
        .iter()
        .find(|item| item.mod_id == record.mod_id)
        .ok_or_else(|| {
            InstallError::Install("The installed mod changed after this operation.".into())
        })?;
    if current_entry.files != record.installed.files
        || current_entry.archive_md5 != record.installed.archive_md5
    {
        return Err(InstallError::Install(
            "The installed mod changed after this operation, so undo was stopped.".into(),
        ));
    }
    for fingerprint in &record.after_files {
        let path = Path::new(&fingerprint.path);
        let metadata = path.metadata().map_err(|_| {
            InstallError::Install(format!(
                "{} is missing or disabled. Restore it before undoing.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ))
        })?;
        if metadata.len() != fingerprint.size_bytes
            || modified_at_millis(&metadata) != fingerprint.modified_at_millis
        {
            return Err(InstallError::Install(format!(
                "{} was modified after installation, so undo was stopped.",
                path.file_name().unwrap_or_default().to_string_lossy()
            )));
        }
    }
    for backup in &record.backups {
        if !Path::new(&backup.backup).is_file() {
            return Err(InstallError::Install(
                "The previous-version backup is incomplete, so undo was stopped.".into(),
            ));
        }
    }

    let transaction_id = next_install_plan_id(record.mod_id);
    let rollback_dir = target.join(format!(".mrmmr-undo-rollback-{transaction_id}"));
    let current_backups = plan_previous_backups(&app, record.mod_id, &target, &rollback_dir)?;
    let mut pending = PendingChange {
        transaction_id,
        game_path: location.path.clone(),
        state: PendingChangeState::Prepared,
        before_inventory: current.clone(),
        backups: backup_records(&current_backups),
        new_files: record
            .backups
            .iter()
            .map(|backup| backup.original.clone())
            .collect(),
        retain_backups_on_commit: false,
        clear_undo_on_commit: true,
    };
    write_pending_change(&app, &pending)?;
    if let Err(error) = apply_backup_plan(&current_backups, &rollback_dir) {
        let _ = remove_pending_change(&app);
        return Err(error);
    }
    pending.state = PendingChangeState::BackedUp;
    if let Err(error) = write_pending_change(&app, &pending) {
        finish_rollback(
            &current_backups,
            &rollback_dir,
            "Could not advance undo recovery data.",
        )?;
        remove_pending_change(&app)?;
        return Err(error);
    }
    let mut restored = Vec::new();
    for backup in &record.backups {
        let source = Path::new(&backup.backup);
        let destination = PathBuf::from(&backup.original);
        if let Err(error) = copy_file_new(source, &destination) {
            for path in &restored {
                let _ = std::fs::remove_file(path);
            }
            finish_rollback(
                &current_backups,
                &rollback_dir,
                "Previous-version restore failed.",
            )?;
            remove_pending_change(&app)?;
            return Err(InstallError::Install(format!(
                "Could not restore the previous version: {error}"
            )));
        }
        restored.push(destination);
    }

    let mut before = current.clone();
    before.retain(|item| item.mod_id != record.mod_id);
    if let Some(previous) = record.previous.clone() {
        before.push(previous);
    }
    if let Err(error) = storage::write_json_atomic(&installed_path, &before) {
        for path in &restored {
            let _ = std::fs::remove_file(path);
        }
        finish_rollback(
            &current_backups,
            &rollback_dir,
            "Undo metadata save failed.",
        )?;
        remove_pending_change(&app)?;
        return Err(InstallError::Storage(format!(
            "Could not restore installed-mod metadata: {error}"
        )));
    }

    pending.state = PendingChangeState::Committed;
    if let Err(error) = write_pending_change(&app, &pending) {
        for path in &restored {
            let _ = remove_file_if_present_runtime(path);
        }
        let metadata_result = storage::write_json_atomic(&installed_path, &current);
        finish_rollback(
            &current_backups,
            &rollback_dir,
            "Could not commit undo recovery data.",
        )?;
        remove_pending_change(&app)?;
        if let Err(rollback) = metadata_result {
            return Err(InstallError::Storage(format!(
                "Could not commit undo recovery data: {error:?}. Metadata rollback also failed: {rollback}"
            )));
        }
        return Err(error);
    }
    recover_pending(&app)?;
    let installed = installed_snapshot(&app)?;
    if let Some(restored) = installed.iter().find(|entry| entry.mod_id == record.mod_id) {
        asset_conflicts::refresh_mod_best_effort(&app, &conflict_state, restored);
    } else {
        asset_conflicts::remove_mod_best_effort(&app, &conflict_state, record.mod_id);
    }
    Ok(installed)
}

pub(crate) fn installed_snapshot(app: &AppHandle) -> Result<Vec<InstalledMod>, InstallError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| InstallError::Storage(format!("Could not resolve config directory: {e}")))?;
    let mut list = read_installed_list(&dir.join(INSTALLED_FILE))?;
    for installed in &mut list {
        let (enabled, missing) = compute_state(installed);
        installed.enabled = enabled;
        installed.missing = missing;
    }
    Ok(list)
}

#[tauri::command]
pub fn get_installed_mods(app: AppHandle) -> Result<Vec<InstalledMod>, InstallError> {
    installed_snapshot(&app)
}

#[tauri::command]
pub fn get_installed_stats(app: AppHandle) -> Result<InstalledStats, InstallError> {
    let installed = installed_snapshot(&app)?;
    let mut seen = HashSet::new();
    let mut total_size_bytes = 0u64;
    for item in &installed {
        for file in &item.files {
            let active = PathBuf::from(file);
            let physical = if active.is_file() {
                active
            } else {
                disabled_path(&active)
            };
            let key = path_key(&physical);
            if seen.insert(key) {
                total_size_bytes = total_size_bytes.saturating_add(
                    physical
                        .metadata()
                        .map(|metadata| metadata.len())
                        .unwrap_or(0),
                );
            }
        }
    }
    Ok(InstalledStats {
        mod_count: installed.len(),
        enabled_count: installed.iter().filter(|item| item.enabled).count(),
        disabled_count: installed
            .iter()
            .filter(|item| !item.enabled && !item.missing)
            .count(),
        missing_count: installed.iter().filter(|item| item.missing).count(),
        total_size_bytes,
    })
}

/// Enable or disable a mod by renaming its files between `Mod.pak` and
/// `Mod.pak.disabled`.
#[tauri::command]
pub fn set_mod_enabled(
    app: AppHandle,
    state: State<'_, InstallState>,
    conflict_state: State<'_, AssetConflictState>,
    mod_id: u32,
    enabled: bool,
) -> Result<InstalledMod, InstallError> {
    ensure_game_files_mutable()?;
    let _mutation = state
        .mutation
        .lock()
        .map_err(|_| InstallError::Storage("Mod changes are temporarily unavailable.".into()))?;
    recover_pending(&app)?;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| InstallError::Storage(format!("Could not resolve config directory: {e}")))?;
    let path = dir.join(INSTALLED_FILE);
    let mut list = read_installed_list(&path)?;

    let Some(index) = list.iter().position(|m| m.mod_id == mod_id) else {
        return Err(InstallError::Install("Mod is not installed.".to_string()));
    };

    let location = game::load_location(&app)
        .map_err(|e| InstallError::Storage(e.to_string()))?
        .ok_or(InstallError::GameNotFound)?;
    let target = mods_dir(&location.path);
    for file in &list[index].files {
        if !is_managed_path(Path::new(file), &target) {
            return Err(InstallError::Install(format!(
                "Refusing to toggle a file outside the configured ~mods folder: {file}"
            )));
        }
    }
    rename_mod_files(&list[index].files, enabled)?;

    let (enabled_state, missing) = compute_state(&list[index]);
    if missing || enabled_state != enabled {
        return Err(InstallError::Install(format!(
            "Could not {} the mod.",
            if enabled { "enable" } else { "disable" }
        )));
    }
    list[index].enabled = enabled_state;
    list[index].missing = false;
    if let Err(error) = clear_undo_record(&app, &target) {
        eprintln!("[install] mod toggled, but undo cleanup failed: {error:?}");
    }
    let updated = list[index].clone();
    if enabled {
        asset_conflicts::refresh_mod_best_effort(&app, &conflict_state, &updated);
    }
    Ok(updated)
}

/// Uninstall a mod by deleting only files exclusively owned by it inside the
/// configured `~mods` folder, then removing its metadata entry.
#[tauri::command]
pub fn uninstall_mod(
    app: AppHandle,
    state: State<'_, InstallState>,
    conflict_state: State<'_, AssetConflictState>,
    mod_id: u32,
) -> Result<(), InstallError> {
    ensure_game_files_mutable()?;
    let _mutation = state
        .mutation
        .lock()
        .map_err(|_| InstallError::Storage("Mod changes are temporarily unavailable.".into()))?;
    recover_pending(&app)?;
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| InstallError::Storage(format!("Could not resolve config directory: {e}")))?;
    let path = dir.join(INSTALLED_FILE);

    let mut list = read_installed_list(&path)?;
    let before_list = list.clone();

    let Some(index) = list.iter().position(|m| m.mod_id == mod_id) else {
        asset_conflicts::remove_mod_best_effort(&app, &conflict_state, mod_id);
        return Ok(());
    };
    let _installed = list.remove(index);

    let location = game::load_location(&app)
        .map_err(|error| InstallError::Storage(error.to_string()))?
        .ok_or(InstallError::GameNotFound)?;
    let target = mods_dir(&location.path);
    let transaction_id = next_install_plan_id(mod_id);
    let rollback_dir = target.join(format!(".mrmmr-uninstall-{transaction_id}"));
    let backups = plan_previous_backups(&app, mod_id, &target, &rollback_dir)?;
    let mut pending = PendingChange {
        transaction_id,
        game_path: location.path.clone(),
        state: PendingChangeState::Prepared,
        before_inventory: before_list.clone(),
        backups: backup_records(&backups),
        new_files: Vec::new(),
        retain_backups_on_commit: false,
        clear_undo_on_commit: true,
    };
    write_pending_change(&app, &pending)?;
    if let Err(error) = apply_backup_plan(&backups, &rollback_dir) {
        let _ = remove_pending_change(&app);
        return Err(error);
    }
    pending.state = PendingChangeState::BackedUp;
    if let Err(error) = write_pending_change(&app, &pending) {
        finish_rollback(
            &backups,
            &rollback_dir,
            "Could not advance uninstall recovery data.",
        )?;
        remove_pending_change(&app)?;
        return Err(error);
    }
    if let Err(error) = storage::write_json_atomic(&path, &list) {
        finish_rollback(&backups, &rollback_dir, "Uninstall metadata save failed.")?;
        remove_pending_change(&app)?;
        return Err(InstallError::Storage(format!(
            "Could not write installed mods: {error}"
        )));
    }
    pending.state = PendingChangeState::Committed;
    if let Err(error) = write_pending_change(&app, &pending) {
        let metadata_result = storage::write_json_atomic(&path, &before_list);
        finish_rollback(
            &backups,
            &rollback_dir,
            "Could not commit uninstall recovery data.",
        )?;
        remove_pending_change(&app)?;
        if let Err(rollback) = metadata_result {
            return Err(InstallError::Storage(format!(
                "Could not commit uninstall recovery data: {error:?}. Metadata rollback also failed: {rollback}"
            )));
        }
        return Err(error);
    }
    recover_pending(&app)?;
    asset_conflicts::remove_mod_best_effort(&app, &conflict_state, mod_id);
    Ok(())
}

/// Remove the entire `~mods` folder and the installed-mods metadata. Used by
/// the factory reset. Metadata is retained if game-file cleanup fails.
pub fn remove_mods(app: &AppHandle) -> Result<(), InstallError> {
    if let Ok(Some(location)) = game::load_location(app) {
        let mods = mods_dir(&location.path);
        if mods.exists() {
            std::fs::remove_dir_all(&mods).map_err(|error| {
                InstallError::Storage(format!("Could not remove '{}': {error}", mods.display()))
            })?;
        }
    }
    let dir = app.path().app_config_dir().map_err(|error| {
        InstallError::Storage(format!("Could not resolve config directory: {error}"))
    })?;
    for file in [INSTALLED_FILE, UNDO_FILE, PENDING_CHANGE_FILE] {
        let path = dir.join(file);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| {
                InstallError::Storage(format!("Could not remove '{}': {error}", path.display()))
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ModUpdate {
    pub mod_id: u32,
    pub name: String,
    pub installed_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub picture_url: Option<String>,
}

/// Compare each installed mod's version against the latest published on Nexus.
/// Best-effort per mod; a mod whose version can't be fetched is reported as
/// having no update.
#[tauri::command]
pub async fn check_mod_updates(
    app: AppHandle,
    state: State<'_, AuthState>,
) -> Result<Vec<ModUpdate>, InstallError> {
    let api_key = resolve_api_key(&app, &state, "")?;
    let mods = get_installed_mods(app.clone())?;

    let mut updates = Vec::new();
    for installed in mods {
        let info = get_mod_info(&api_key, installed.mod_id).await.ok();
        let latest = info
            .as_ref()
            .map(|info| info.version.clone())
            .unwrap_or_default();
        updates.push(ModUpdate {
            mod_id: installed.mod_id,
            name: installed.name,
            installed_version: installed.version.clone(),
            latest_version: latest.clone(),
            has_update: !latest.is_empty() && latest != installed.version,
            picture_url: info.and_then(|info| info.picture_url),
        });
    }
    Ok(updates)
}

#[tauri::command]
pub fn mod_files_url(mod_id: u32) -> String {
    format!("https://www.nexusmods.com/{GAME_DOMAIN}/mods/{mod_id}?tab=files")
}

#[tauri::command]
pub async fn detect_mod_download(
    app: AppHandle,
    state: State<'_, AuthState>,
    mod_id: u32,
    file_ids: Vec<u32>,
) -> Result<Option<String>, InstallError> {
    const RECENT: std::time::Duration = std::time::Duration::from_secs(600);
    let api_key = resolve_api_key(&app, &state, "")?;
    let expected_hashes: HashSet<String> = get_mod_files(&api_key, mod_id)
        .await?
        .into_iter()
        .filter(|file| file_ids.is_empty() || file_ids.contains(&file.file_id))
        .filter_map(|file| file.md5.map(|md5| md5.to_ascii_lowercase()))
        .collect();
    if expected_hashes.is_empty() {
        return Ok(None);
    }
    let mut candidates = Vec::new();

    for dir in utoc::download_candidate_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !utoc::is_archive(&path) {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if modified.elapsed().map(|age| age > RECENT).unwrap_or(true) {
                continue;
            }
            candidates.push((modified, path));
        }
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, path) in candidates {
        if expected_hashes.contains(&archive_md5(&path)?.to_ascii_lowercase())
            && archive_contains_pak(&path)
        {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

fn archive_contains_pak(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let has_pak = |name: &str| -> bool {
        let name = name.replace('\\', "/").to_ascii_lowercase();
        name.ends_with(".pak")
    };

    match ext.as_str() {
        "zip" => {
            let file = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(_) => return false,
            };
            let mut archive = match zip::ZipArchive::new(file) {
                Ok(a) => a,
                Err(_) => return false,
            };
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    if has_pak(entry.name()) {
                        return true;
                    }
                }
            }
            false
        }
        "7z" => {
            let archive = match sevenz_rust::Archive::open(path) {
                Ok(a) => a,
                Err(_) => return false,
            };
            archive.files.iter().any(|e| has_pak(e.name()))
        }
        "rar" => {
            let open = match unrar::Archive::new(path).open_for_listing() {
                Ok(o) => o,
                Err(_) => return false,
            };
            open.into_iter()
                .flatten()
                .any(|e| has_pak(&e.filename.to_string_lossy()))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nexus_download_link_uri_casing() {
        let uppercase: Vec<DownloadLink> =
            serde_json::from_str(r#"[{"URI":"https://premium-files.nexusmods.com/mod.7z"}]"#)
                .unwrap();
        let lowercase: Vec<DownloadLink> =
            serde_json::from_str(r#"[{"uri":"https://premium-files.nexusmods.com/mod.7z"}]"#)
                .unwrap();
        assert_eq!(uppercase[0].uri, lowercase[0].uri);
    }

    fn installed(mod_id: u32, files: Vec<String>) -> InstalledMod {
        InstalledMod {
            mod_id,
            name: format!("Mod {mod_id}"),
            version: String::new(),
            files,
            installed_at: 0,
            nexus_file_id: None,
            archive_name: None,
            archive_md5: None,
            parts: Vec::new(),
            picture_url: None,
            enabled: true,
            missing: false,
        }
    }

    #[test]
    fn finds_pak_files_recursively() {
        let root = std::env::temp_dir().join("mrmmr_install_test");
        let nested = root.join("sub").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("Mod.pak"), b"").unwrap();
        std::fs::write(root.join("skip.txt"), b"").unwrap();
        std::fs::write(root.join("Other.PAK"), b"").unwrap();

        let mut paks = Vec::new();
        find_paks(&root, &mut paks).unwrap();
        assert_eq!(paks.len(), 2);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copy_plan_keeps_every_duplicate_named_payload() {
        let root = std::env::temp_dir().join("mrmmr_duplicate_plan_test");
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("target");
        let mut paks = Vec::new();
        for part in ["part-a", "part-b", "part-c"] {
            let directory = root.join(part);
            std::fs::create_dir_all(&directory).unwrap();
            let pak = directory.join("SharedName.pak");
            std::fs::write(&pak, part.as_bytes()).unwrap();
            paks.push(pak);
        }

        let plan = build_copy_plan(&paks, &root, &target, 42);
        let destinations: std::collections::HashSet<String> = plan
            .iter()
            .map(|(_, destination)| {
                destination
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(plan.len(), 3);
        assert_eq!(destinations.len(), 3);
        assert!(destinations.contains("SharedName.pak"));
        assert!(destinations.contains("42_SharedName.pak"));
        assert!(destinations.contains("42_2_SharedName.pak"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn preview_actions_have_stable_ipc_names() {
        assert_eq!(
            serde_json::to_string(&InstallPreviewAction::Add).unwrap(),
            r#""add""#
        );
        assert_eq!(
            serde_json::to_string(&InstallPreviewAction::Blocked).unwrap(),
            r#""blocked""#
        );
    }

    fn nexus_file(file_id: u32, name: &str) -> ModFile {
        ModFile {
            file_id,
            category_name: Some("MAIN".into()),
            file_name: Some(format!("{name}.zip")),
            md5: Some(format!("md5-{file_id}")),
            name: Some(name.into()),
            version: Some("1.0".into()),
            size_kb: Some(100),
        }
    }

    #[test]
    fn detects_explicit_multi_part_file_names() {
        assert_eq!(
            explicit_part(&nexus_file(1, "Costume Pack Part 1 of 3")),
            Some(("costume pack".into(), 1))
        );
        assert_eq!(
            explicit_part(&nexus_file(2, "Costume-Pack-pt2")),
            Some(("costume pack".into(), 2))
        );
        assert_eq!(explicit_part(&nexus_file(3, "Particle Effects")), None);
    }

    #[test]
    fn groups_only_continuous_numbered_nexus_parts() {
        let grouped = install_options(&[
            nexus_file(10, "Hero Audio Part 1"),
            nexus_file(11, "Hero Audio Part 2"),
        ]);
        assert_eq!(grouped.len(), 1);
        assert!(grouped[0].multipart);
        assert_eq!(
            grouped[0]
                .files
                .iter()
                .map(|file| file.file_id)
                .collect::<Vec<_>>(),
            vec![10, 11]
        );

        let incomplete = install_options(&[
            nexus_file(20, "Hero Audio Part 1"),
            nexus_file(22, "Hero Audio Part 3"),
        ]);
        assert_eq!(incomplete.len(), 2);
        assert!(incomplete.iter().all(|option| !option.multipart));
    }

    #[test]
    fn pending_change_commit_policy_survives_serialization() {
        let pending = PendingChange {
            transaction_id: "42-7".into(),
            game_path: r"C:\Games\MarvelRivals".into(),
            state: PendingChangeState::Committed,
            before_inventory: Vec::new(),
            backups: vec![UndoBackup {
                original: r"C:\Games\MarvelRivals\~mods\Mod.pak".into(),
                backup: r"C:\Games\MarvelRivals\~mods\.mrmmr-undo-42-7\0".into(),
            }],
            new_files: Vec::new(),
            retain_backups_on_commit: false,
            clear_undo_on_commit: true,
        };

        let encoded = serde_json::to_string(&pending).unwrap();
        let decoded: PendingChange = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.state, PendingChangeState::Committed);
        assert!(decoded.clear_undo_on_commit);
        assert!(!decoded.retain_backups_on_commit);
    }

    #[test]
    fn rollback_restores_every_moved_file() {
        let root = std::env::temp_dir().join(format!(
            "mrmmr_rollback_test_{}",
            next_install_plan_id(9000)
        ));
        let backup_dir = root.join("backup");
        std::fs::create_dir_all(&backup_dir).unwrap();
        let first = root.join("First.pak");
        let second = root.join("Second.utoc");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let backups = vec![
            (first.clone(), backup_dir.join("0")),
            (second.clone(), backup_dir.join("1")),
        ];
        apply_backup_plan(&backups, &backup_dir).unwrap();
        assert!(!first.exists());
        assert!(!second.exists());

        finish_rollback(&backups, &backup_dir, "test rollback").unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
        assert!(!backup_dir.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recovery_removal_rejects_directories() {
        let root = std::env::temp_dir().join(format!(
            "mrmmr_recovery_remove_test_{}",
            next_install_plan_id(9001)
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert!(matches!(
            remove_file_if_present_runtime(&root),
            Err(InstallError::Install(_))
        ));
        assert!(root.is_dir());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn archive_contains_pak_in_zip() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("mrmmr_install_zip");
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("mod.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file("paks/MyMod.pak", options).unwrap();
            writer.write_all(b"pak").unwrap();
            writer.finish().unwrap();
        }
        assert!(archive_contains_pak(&zip_path));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn migrates_old_paks_metadata_schema() {
        let json = r#"[{"mod_id": 1, "name": "Old", "version": "1.0", "paks": ["C:\\x\\a.pak"], "installed_at": 1}]"#;
        let list: Vec<InstalledMod> = serde_json::from_str(json).expect("old schema should parse");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].files, vec!["C:\\x\\a.pak"]);
        assert_eq!(list[0].picture_url, None);
        assert!(list[0].parts.is_empty());
    }

    #[test]
    fn installed_state_is_serialized_for_tauri_ipc() {
        let installed = installed(1, vec![r"C:\mods\Example.pak".to_string()]);
        let value = serde_json::to_value(installed).unwrap();
        assert_eq!(value["enabled"], true);
        assert_eq!(value["missing"], false);
    }

    #[test]
    fn archive_cleanup_is_opt_in_and_archive_only() {
        let dir = std::env::temp_dir().join("mrmmr_archive_cleanup_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("mod.7z");
        let unrelated = dir.join("notes.txt");
        std::fs::write(&archive, b"archive").unwrap();
        std::fs::write(&unrelated, b"notes").unwrap();

        assert!(!delete_installed_archive(&archive, false).unwrap());
        assert!(archive.exists());
        assert!(!delete_installed_archive(&unrelated, true).unwrap());
        assert!(unrelated.exists());
        assert!(delete_installed_archive(&archive, true).unwrap());
        assert!(!archive.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nexus_md5_result_must_match_requested_mod() {
        let value = serde_json::json!([{
            "mod": { "mod_id": 41 },
            "file_details": { "file_id": 99, "file_name": "mod-a.zip" }
        }]);

        let identity = extract_archive_identity(&value, 41, "abc123").unwrap();
        assert_eq!(identity.file_id, Some(99));
        assert_eq!(identity.file_name.as_deref(), Some("mod-a.zip"));
        assert_eq!(identity.md5, "abc123");
        assert!(extract_archive_identity(&value, 42, "abc123").is_none());
    }

    #[test]
    fn owned_file_removal_preserves_shared_and_outside_files() {
        let root = std::env::temp_dir().join("mrmmr_ownership_test");
        let target = root.join("~mods");
        let outside = root.join("outside.pak");
        let shared = target.join("shared.pak");
        let unique = target.join("unique.pak");
        let unique_disabled = disabled_path(&unique);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(&outside, b"outside").unwrap();
        std::fs::write(&shared, b"shared").unwrap();
        std::fs::write(&unique_disabled, b"disabled").unwrap();

        let files = vec![
            shared.to_string_lossy().into_owned(),
            unique.to_string_lossy().into_owned(),
            outside.to_string_lossy().into_owned(),
        ];
        let remaining = vec![installed(1, vec![shared.to_string_lossy().into_owned()])];
        remove_owned_files(&files, &remaining, &target).unwrap();

        assert!(shared.exists(), "another mod still owns the shared path");
        assert!(
            !unique_disabled.exists(),
            "disabled owned file should be removed"
        );
        assert!(
            outside.exists(),
            "paths outside ~mods must never be removed"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn disable_enable_round_trip() {
        let dir = std::env::temp_dir().join("mrmmr_toggle_test");
        let target = dir.join("~mods");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&target).unwrap();
        let pak = target.join("Mod.pak");
        let utoc = target.join("Mod.utoc");
        std::fs::write(&pak, b"pak").unwrap();
        std::fs::write(&utoc, b"utoc").unwrap();

        let installed = InstalledMod {
            mod_id: 1,
            name: "Mod".into(),
            version: "1.0".into(),
            files: vec![
                pak.to_string_lossy().into_owned(),
                utoc.to_string_lossy().into_owned(),
            ],
            installed_at: 0,
            nexus_file_id: None,
            archive_name: None,
            archive_md5: None,
            parts: Vec::new(),
            picture_url: None,
            enabled: true,
            missing: false,
        };

        // Initially enabled.
        assert_eq!(compute_state(&installed), (true, false));

        // Disable.
        rename_mod_files(&installed.files, false).unwrap();
        assert_eq!(compute_state(&installed), (false, false));
        assert!(disabled_path(&pak).exists());
        assert!(disabled_path(&utoc).exists());

        // Re-enable.
        rename_mod_files(&installed.files, true).unwrap();
        assert_eq!(compute_state(&installed), (true, false));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn toggle_preflight_rejects_collision_without_mutating_other_files() {
        let dir = std::env::temp_dir().join("mrmmr_toggle_collision_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("First.pak");
        let second = dir.join("Second.pak");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        std::fs::write(disabled_path(&second), b"collision").unwrap();
        let files = vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];

        let error = rename_mod_files(&files, false).unwrap_err();
        assert!(matches!(error, InstallError::Install(_)));
        assert!(first.exists(), "preflight must run before any rename");
        assert!(!disabled_path(&first).exists());
        assert!(second.exists());
        assert!(disabled_path(&second).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mixed_toggle_state_can_converge_to_enabled() {
        let dir = std::env::temp_dir().join("mrmmr_toggle_mixed_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pak = dir.join("Mixed.pak");
        let utoc = dir.join("Mixed.utoc");
        std::fs::write(&pak, b"pak").unwrap();
        std::fs::write(disabled_path(&utoc), b"utoc").unwrap();
        let installed = installed(
            1,
            vec![
                pak.to_string_lossy().into_owned(),
                utoc.to_string_lossy().into_owned(),
            ],
        );

        assert_eq!(compute_state(&installed), (false, false));
        rename_mod_files(&installed.files, true).unwrap();
        assert_eq!(compute_state(&installed), (true, false));
        assert!(pak.exists());
        assert!(utoc.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
