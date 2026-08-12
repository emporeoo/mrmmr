use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

use crate::install::{self, InstallError, InstalledMod};
use crate::storage;

const INDEX_FILE: &str = "asset-index-v1.json";
const INDEX_VERSION: u32 = 1;
const SCANNER_VERSION: u32 = 1;
const MAX_ASSETS_PER_MOD: usize = 250_000;
const MAX_ASSET_PATH_BYTES: usize = 2_048;
const MAX_CONFLICT_DETAILS_PER_MOD: usize = 1_000;
const MARVEL_AES_KEY: &str = "0C263D8C22DCB085894899C3A3796383E9BF9DE0CBFB08C9BF2DEF2E84F29D74";

#[derive(Clone, Default)]
pub struct AssetConflictState {
    cache: Arc<Mutex<Option<AssetIndexStore>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssetIndexStore {
    version: u32,
    #[serde(default)]
    mods: BTreeMap<u32, ModAssetIndex>,
}

impl Default for AssetIndexStore {
    fn default() -> Self {
        Self {
            version: INDEX_VERSION,
            mods: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModAssetIndex {
    scanner_version: u32,
    fingerprint: String,
    status: AssetScanStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    scanned_at: i64,
    #[serde(default)]
    assets: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetScanStatus {
    Complete,
    Partial,
    Failed,
    Pending,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetConflictSummary {
    pub conflicting_asset_count: usize,
    pub affected_mod_count: usize,
    pub scan_incomplete_mod_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetConflictReport {
    pub summary: AssetConflictSummary,
    pub mods: Vec<ModAssetConflictReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModAssetConflictReport {
    pub mod_id: u32,
    pub mod_name: String,
    pub enabled: bool,
    pub scan_status: AssetScanStatus,
    pub scan_error: Option<String>,
    pub conflicting_asset_count: usize,
    pub conflicts: Vec<AssetConflictDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetConflictDetail {
    pub asset_path: String,
    pub other_mods: Vec<AssetConflictPeer>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewAssetConflictReport {
    pub scan_status: AssetScanStatus,
    pub scan_error: Option<String>,
    pub scanned_asset_count: usize,
    pub conflicting_asset_count: usize,
    pub affected_mod_count: usize,
    pub conflicts: Vec<AssetConflictDetail>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetConflictPeer {
    pub mod_id: u32,
    pub mod_name: String,
    pub enabled: bool,
}

fn index_path(app: &AppHandle) -> Result<PathBuf, InstallError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(INDEX_FILE))
        .map_err(|error| {
            InstallError::Storage(format!(
                "Could not resolve the conflict index location: {error}"
            ))
        })
}

fn read_store(app: &AppHandle) -> Result<AssetIndexStore, InstallError> {
    let path = index_path(app)?;
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str::<AssetIndexStore>(&contents) {
            Ok(store) if store.version == INDEX_VERSION => Ok(store),
            Ok(_) => Ok(AssetIndexStore::default()),
            Err(error) => {
                eprintln!(
                    "[conflicts] rebuilding unreadable asset index '{}': {error}",
                    path.display()
                );
                Ok(AssetIndexStore::default())
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(AssetIndexStore::default())
        }
        Err(error) => Err(InstallError::Storage(format!(
            "Could not read the conflict index: {error}"
        ))),
    }
}

fn write_store(app: &AppHandle, store: &AssetIndexStore) -> Result<(), InstallError> {
    storage::write_json_atomic(&index_path(app)?, store).map_err(|error| {
        InstallError::Storage(format!("Could not save the conflict index: {error}"))
    })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn disabled_path(path: &Path) -> PathBuf {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) if !name.ends_with(".disabled") => {
            path.with_file_name(format!("{name}.disabled"))
        }
        _ => path.to_path_buf(),
    }
}

fn physical_path(logical: &Path) -> Option<PathBuf> {
    if logical.is_file() {
        Some(logical.to_path_buf())
    } else {
        let disabled = disabled_path(logical);
        disabled.is_file().then_some(disabled)
    }
}

fn files_fingerprint(files: &[String]) -> String {
    let mut logical_files: Vec<&String> = files.iter().collect();
    logical_files.sort_by_key(|path| path.to_ascii_lowercase());
    let mut digest = Md5::new();
    digest.update(SCANNER_VERSION.to_le_bytes());
    for logical in logical_files {
        digest.update(logical.to_ascii_lowercase().as_bytes());
        let path = Path::new(logical);
        if let Some(physical) = physical_path(path) {
            if let Ok(metadata) = physical.metadata() {
                digest.update(metadata.len().to_le_bytes());
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis())
                    .unwrap_or_default();
                digest.update(modified.to_le_bytes());
                continue;
            }
        }
        digest.update(b"missing");
    }
    format!("{:x}", digest.finalize())
}

fn normalize_asset_path(raw: &str) -> Option<String> {
    let mut path = raw.trim().replace('\\', "/");
    while path.starts_with("../") || path.starts_with("./") {
        path = path
            .strip_prefix("../")
            .or_else(|| path.strip_prefix("./"))
            .unwrap_or(&path)
            .to_string();
    }
    path = path.trim_start_matches('/').to_string();
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if path.is_empty() || path.len() > MAX_ASSET_PATH_BYTES {
        return None;
    }

    let lower = path.to_ascii_lowercase();
    let mut normalized = if lower == "game" {
        "marvel/content".to_string()
    } else if let Some(rest) = lower.strip_prefix("game/") {
        format!("marvel/content/{rest}")
    } else {
        lower
    };

    if let Some((stem, extension)) = normalized.rsplit_once('.') {
        if matches!(extension, "uexp" | "ubulk" | "uptnl") {
            normalized = format!("{stem}.uasset");
        }
    }
    if normalized == "patched_files" || normalized.starts_with("patched_files/") {
        return None;
    }
    Some(format!("/{}", normalized.trim_start_matches('/')))
}

fn push_asset(assets: &mut BTreeSet<String>, raw: &str) -> Result<(), String> {
    if let Some(path) = normalize_asset_path(raw) {
        assets.insert(path);
        if assets.len() > MAX_ASSETS_PER_MOD {
            return Err(format!(
                "The mod contains more than {MAX_ASSETS_PER_MOD} indexed assets."
            ));
        }
    }
    Ok(())
}

fn scan_pak(path: &Path, assets: &mut BTreeSet<String>) -> Result<(), String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(file);
    let key = repak::utils::AesKey::from_str(MARVEL_AES_KEY)
        .map_err(|error| format!("invalid Marvel Rivals key: {error}"))?;
    let pak = repak::PakBuilder::new()
        .key(key.0)
        .reader(&mut reader)
        .map_err(|error| error.to_string())?;
    let mount = pak.mount_point().replace('\\', "/");
    for file in pak.files() {
        let path = if file.starts_with('/') || file.starts_with("../") {
            file
        } else {
            format!("{mount}{file}")
        };
        push_asset(assets, &path)?;
    }
    Ok(())
}

fn scan_utoc(path: &Path, assets: &mut BTreeSet<String>) -> Result<(), String> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".disabled"))
    {
        return Err("Enable this mod once to index its disabled IoStore files.".into());
    }
    let mut config = retoc::Config::default();
    let key = retoc::AesKey::from_str(MARVEL_AES_KEY).map_err(|error| error.to_string())?;
    config.aes_keys.insert(retoc::FGuid::default(), key);
    let store = retoc::open_iostore(path, Arc::new(config)).map_err(|error| error.to_string())?;
    for chunk in store.chunks() {
        if let Some(path) = chunk.path() {
            push_asset(assets, &path)?;
        }
    }
    Ok(())
}

fn logical_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
}

fn scan_files(files: &[String], fingerprint: String) -> ModAssetIndex {
    let mut assets = BTreeSet::new();
    let mut errors = Vec::new();
    let mut successful_containers = 0_usize;
    let mut supported_containers = 0_usize;
    let utoc_stems: HashSet<String> = files
        .iter()
        .map(Path::new)
        .filter(|path| logical_extension(path).as_deref() == Some("utoc"))
        .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()))
        .map(str::to_ascii_lowercase)
        .collect();

    for logical in files {
        let logical_path = Path::new(logical);
        let Some(extension) = logical_extension(logical_path) else {
            continue;
        };
        if extension != "pak" && extension != "utoc" {
            continue;
        }
        if extension == "pak"
            && logical_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| utoc_stems.contains(&stem.to_ascii_lowercase()))
        {
            continue;
        }
        supported_containers += 1;
        let Some(physical) = physical_path(logical_path) else {
            errors.push(format!(
                "{} is missing.",
                logical_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
            continue;
        };
        let file_name = logical_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if extension == "pak" {
                scan_pak(&physical, &mut assets)
            } else {
                scan_utoc(&physical, &mut assets)
            }
        }))
        .unwrap_or_else(|_| Err("the parser stopped unexpectedly".into()));
        match result {
            Ok(()) => successful_containers += 1,
            Err(error) => errors.push(format!("{file_name}: {error}")),
        }
    }

    if supported_containers == 0 {
        errors.push("No PAK or UTOC containers were available to scan.".into());
    }
    if errors.len() > 4 {
        let remaining = errors.len() - 4;
        errors.truncate(4);
        errors.push(format!("{remaining} more container errors."));
    }
    let status = if errors.is_empty() {
        AssetScanStatus::Complete
    } else if successful_containers > 0 {
        AssetScanStatus::Partial
    } else {
        AssetScanStatus::Failed
    };
    ModAssetIndex {
        scanner_version: SCANNER_VERSION,
        fingerprint,
        status,
        error: (!errors.is_empty()).then(|| errors.join(" ")),
        scanned_at: now_unix(),
        assets: assets.into_iter().collect(),
    }
}

fn scan_mod(mod_entry: &InstalledMod, fingerprint: String) -> ModAssetIndex {
    scan_files(&mod_entry.files, fingerprint)
}

fn ensure_loaded<'a>(
    app: &AppHandle,
    state: &'a AssetConflictState,
) -> Result<std::sync::MutexGuard<'a, Option<AssetIndexStore>>, InstallError> {
    let mut cache = state.cache.lock().map_err(|_| {
        InstallError::Storage("The conflict index is temporarily unavailable.".into())
    })?;
    if cache.is_none() {
        *cache = Some(read_store(app)?);
    }
    Ok(cache)
}

fn reconcile_store<'a>(
    app: &AppHandle,
    state: &'a AssetConflictState,
    installed: &[InstalledMod],
) -> Result<std::sync::MutexGuard<'a, Option<AssetIndexStore>>, InstallError> {
    let mut cache = ensure_loaded(app, state)?;
    let store = cache.as_mut().expect("asset index loaded");
    let installed_ids: HashSet<u32> = installed.iter().map(|entry| entry.mod_id).collect();
    let before_count = store.mods.len();
    store
        .mods
        .retain(|mod_id, _| installed_ids.contains(mod_id));
    let mut dirty = store.mods.len() != before_count;

    for mod_entry in installed {
        let fingerprint = files_fingerprint(&mod_entry.files);
        let current = store.mods.get(&mod_entry.mod_id);
        let stale = current.is_none_or(|index| {
            index.scanner_version != SCANNER_VERSION || index.fingerprint != fingerprint
        });
        if stale {
            store
                .mods
                .insert(mod_entry.mod_id, scan_mod(mod_entry, fingerprint));
            dirty = true;
        }
    }
    if dirty {
        write_store(app, store)?;
    }
    Ok(cache)
}

fn build_summary(installed: &[InstalledMod], store: &AssetIndexStore) -> AssetConflictSummary {
    let active: Vec<&InstalledMod> = installed
        .iter()
        .filter(|entry| entry.enabled && !entry.missing)
        .collect();
    let mut owner_counts: HashMap<&str, usize> = HashMap::new();
    for entry in &active {
        let Some(index) = store.mods.get(&entry.mod_id) else {
            continue;
        };
        if index.status == AssetScanStatus::Failed {
            continue;
        }
        for asset in &index.assets {
            *owner_counts.entry(asset).or_default() += 1;
        }
    }
    let conflicting_assets: HashSet<&str> = owner_counts
        .into_iter()
        .filter_map(|(asset, count)| (count > 1).then_some(asset))
        .collect();
    let affected_mod_count = active
        .iter()
        .filter(|entry| {
            store.mods.get(&entry.mod_id).is_some_and(|index| {
                index
                    .assets
                    .iter()
                    .any(|asset| conflicting_assets.contains(asset.as_str()))
            })
        })
        .count();
    let scan_incomplete_mod_count = installed
        .iter()
        .filter(|entry| {
            store
                .mods
                .get(&entry.mod_id)
                .is_none_or(|index| index.status != AssetScanStatus::Complete)
        })
        .count();
    AssetConflictSummary {
        conflicting_asset_count: conflicting_assets.len(),
        affected_mod_count,
        scan_incomplete_mod_count,
    }
}

fn build_preview_report(
    candidate_mod_id: u32,
    candidate: ModAssetIndex,
    installed: &[InstalledMod],
    store: &AssetIndexStore,
) -> PreviewAssetConflictReport {
    let mut owners: HashMap<&str, Vec<AssetConflictPeer>> = HashMap::new();
    let mut incomplete_peers = Vec::new();
    for entry in installed
        .iter()
        .filter(|entry| entry.mod_id != candidate_mod_id && entry.enabled && !entry.missing)
    {
        let Some(index) = store.mods.get(&entry.mod_id) else {
            incomplete_peers.push(entry.name.clone());
            continue;
        };
        if index.status != AssetScanStatus::Complete {
            incomplete_peers.push(entry.name.clone());
        }
        if index.status == AssetScanStatus::Failed {
            continue;
        }
        for asset in &index.assets {
            owners.entry(asset).or_default().push(AssetConflictPeer {
                mod_id: entry.mod_id,
                mod_name: entry.name.clone(),
                enabled: true,
            });
        }
    }

    let mut affected_mods = BTreeSet::new();
    let mut conflicts = Vec::new();
    if candidate.status != AssetScanStatus::Failed {
        for asset in &candidate.assets {
            let Some(peers) = owners.get(asset.as_str()) else {
                continue;
            };
            let mut peers = peers.clone();
            peers.sort();
            peers.dedup_by_key(|peer| peer.mod_id);
            affected_mods.extend(peers.iter().map(|peer| peer.mod_id));
            conflicts.push(AssetConflictDetail {
                asset_path: asset.clone(),
                other_mods: peers,
            });
        }
    }
    conflicts.sort_by(|left, right| left.asset_path.cmp(&right.asset_path));
    let conflicting_asset_count = conflicts.len();
    conflicts.truncate(MAX_CONFLICT_DETAILS_PER_MOD);

    incomplete_peers.sort_by_key(|name| name.to_ascii_lowercase());
    incomplete_peers.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    let mut errors = candidate.error.into_iter().collect::<Vec<_>>();
    if !incomplete_peers.is_empty() {
        let shown = incomplete_peers.iter().take(3).cloned().collect::<Vec<_>>();
        let remaining = incomplete_peers.len().saturating_sub(shown.len());
        errors.push(format!(
            "Installed asset indexes are incomplete for {}{}.",
            shown.join(", "),
            if remaining == 0 {
                String::new()
            } else {
                format!(" and {remaining} more")
            }
        ));
    }
    let scan_status = if candidate.status == AssetScanStatus::Failed {
        AssetScanStatus::Failed
    } else if candidate.status == AssetScanStatus::Partial || !incomplete_peers.is_empty() {
        AssetScanStatus::Partial
    } else {
        AssetScanStatus::Complete
    };

    PreviewAssetConflictReport {
        scan_status,
        scan_error: (!errors.is_empty()).then(|| errors.join(" ")),
        scanned_asset_count: candidate.assets.len(),
        conflicting_asset_count,
        affected_mod_count: affected_mods.len(),
        conflicts,
    }
}

pub(crate) fn preview_asset_conflicts(
    app: &AppHandle,
    state: &AssetConflictState,
    candidate_mod_id: u32,
    staged_files: &[PathBuf],
) -> PreviewAssetConflictReport {
    let staged_files: Vec<String> = staged_files
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let candidate = scan_files(&staged_files, files_fingerprint(&staged_files));
    let fallback = |error: InstallError| PreviewAssetConflictReport {
        scan_status: AssetScanStatus::Failed,
        scan_error: Some(format!(
            "Could not compare installed asset indexes: {error:?}"
        )),
        scanned_asset_count: candidate.assets.len(),
        conflicting_asset_count: 0,
        affected_mod_count: 0,
        conflicts: Vec::new(),
    };
    let installed = match install::installed_snapshot(app) {
        Ok(installed) => installed,
        Err(error) => return fallback(error),
    };
    let cache = match reconcile_store(app, state, &installed) {
        Ok(cache) => cache,
        Err(error) => return fallback(error),
    };
    build_preview_report(
        candidate_mod_id,
        candidate,
        &installed,
        cache.as_ref().expect("asset index loaded"),
    )
}

fn build_report(installed: &[InstalledMod], store: &AssetIndexStore) -> AssetConflictReport {
    let active: HashMap<u32, &InstalledMod> = installed
        .iter()
        .filter(|entry| entry.enabled && !entry.missing)
        .map(|entry| (entry.mod_id, entry))
        .collect();
    let mut owners: HashMap<&str, Vec<u32>> = HashMap::new();
    for (mod_id, mod_entry) in &active {
        let Some(index) = store.mods.get(mod_id) else {
            continue;
        };
        if index.status == AssetScanStatus::Failed {
            continue;
        }
        for asset in &index.assets {
            owners.entry(asset).or_default().push(mod_entry.mod_id);
        }
    }

    let mut conflicts_by_mod: HashMap<u32, Vec<AssetConflictDetail>> = HashMap::new();
    let mut affected = BTreeSet::new();
    let mut conflicting_assets = 0_usize;
    for (asset_path, owner_ids) in owners {
        let unique: BTreeSet<u32> = owner_ids.into_iter().collect();
        if unique.len() < 2 {
            continue;
        }
        conflicting_assets += 1;
        affected.extend(unique.iter().copied());
        for owner_id in &unique {
            let mut peers: Vec<AssetConflictPeer> = unique
                .iter()
                .filter(|peer_id| *peer_id != owner_id)
                .filter_map(|peer_id| active.get(peer_id))
                .map(|peer| AssetConflictPeer {
                    mod_id: peer.mod_id,
                    mod_name: peer.name.clone(),
                    enabled: peer.enabled,
                })
                .collect();
            peers.sort();
            conflicts_by_mod
                .entry(*owner_id)
                .or_default()
                .push(AssetConflictDetail {
                    asset_path: asset_path.to_string(),
                    other_mods: peers,
                });
        }
    }
    for details in conflicts_by_mod.values_mut() {
        details.sort_by(|left, right| left.asset_path.cmp(&right.asset_path));
    }

    let mut mods: Vec<ModAssetConflictReport> = installed
        .iter()
        .map(|entry| {
            let index = store.mods.get(&entry.mod_id);
            let mut conflicts = conflicts_by_mod.remove(&entry.mod_id).unwrap_or_default();
            let conflicting_asset_count = conflicts.len();
            conflicts.truncate(MAX_CONFLICT_DETAILS_PER_MOD);
            ModAssetConflictReport {
                mod_id: entry.mod_id,
                mod_name: entry.name.clone(),
                enabled: entry.enabled,
                scan_status: index
                    .map(|index| index.status)
                    .unwrap_or(AssetScanStatus::Pending),
                scan_error: index.and_then(|index| index.error.clone()),
                conflicting_asset_count,
                conflicts,
            }
        })
        .collect();
    mods.sort_by(|left, right| {
        left.mod_name
            .to_lowercase()
            .cmp(&right.mod_name.to_lowercase())
    });
    let scan_incomplete_mod_count = mods
        .iter()
        .filter(|entry| entry.scan_status != AssetScanStatus::Complete)
        .count();
    AssetConflictReport {
        summary: AssetConflictSummary {
            conflicting_asset_count: conflicting_assets,
            affected_mod_count: affected.len(),
            scan_incomplete_mod_count,
        },
        mods,
    }
}

pub(crate) fn refresh_mod_best_effort(
    app: &AppHandle,
    state: &AssetConflictState,
    mod_entry: &InstalledMod,
) {
    let result = (|| -> Result<(), InstallError> {
        let mut cache = ensure_loaded(app, state)?;
        let store = cache.as_mut().expect("asset index loaded");
        let fingerprint = files_fingerprint(&mod_entry.files);
        store
            .mods
            .insert(mod_entry.mod_id, scan_mod(mod_entry, fingerprint));
        write_store(app, store)
    })();
    if let Err(error) = result {
        eprintln!("[conflicts] mod installed, but its asset index was not saved: {error:?}");
    }
}

pub(crate) fn remove_mod_best_effort(app: &AppHandle, state: &AssetConflictState, mod_id: u32) {
    let result = (|| -> Result<(), InstallError> {
        let mut cache = ensure_loaded(app, state)?;
        let store = cache.as_mut().expect("asset index loaded");
        if store.mods.remove(&mod_id).is_some() {
            write_store(app, store)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("[conflicts] mod removed, but its asset index was not updated: {error:?}");
    }
}

#[tauri::command]
pub fn get_asset_conflict_summary(
    app: AppHandle,
    state: State<'_, AssetConflictState>,
) -> Result<AssetConflictSummary, InstallError> {
    let installed = install::installed_snapshot(&app)?;
    let cache = reconcile_store(&app, &state, &installed)?;
    Ok(build_summary(
        &installed,
        cache.as_ref().expect("asset index loaded"),
    ))
}

#[tauri::command]
pub fn get_asset_conflicts(
    app: AppHandle,
    state: State<'_, AssetConflictState>,
) -> Result<AssetConflictReport, InstallError> {
    let installed = install::installed_snapshot(&app)?;
    let cache = reconcile_store(&app, &state, &installed)?;
    Ok(build_report(
        &installed,
        cache.as_ref().expect("asset index loaded"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(mod_id: u32, name: &str, enabled: bool) -> InstalledMod {
        InstalledMod {
            mod_id,
            name: name.into(),
            version: "1".into(),
            files: Vec::new(),
            installed_at: 0,
            nexus_file_id: None,
            archive_name: None,
            archive_md5: None,
            parts: Vec::new(),
            picture_url: None,
            enabled,
            missing: false,
        }
    }

    fn index(assets: &[&str]) -> ModAssetIndex {
        ModAssetIndex {
            scanner_version: SCANNER_VERSION,
            fingerprint: String::new(),
            status: AssetScanStatus::Complete,
            error: None,
            scanned_at: 0,
            assets: assets.iter().map(|asset| (*asset).into()).collect(),
        }
    }

    #[test]
    fn normalizes_unreal_mounts_and_sidecars() {
        assert_eq!(
            normalize_asset_path("../../../Marvel/Content/Characters/1024/Hero.uasset"),
            Some("/marvel/content/characters/1024/hero.uasset".into())
        );
        assert_eq!(
            normalize_asset_path("/Game/Characters/1024/Hero.uexp"),
            Some("/marvel/content/characters/1024/hero.uasset".into())
        );
    }

    #[test]
    fn reports_shared_assets_for_each_enabled_provider() {
        let mods = vec![installed(1, "One", true), installed(2, "Two", true)];
        let store = AssetIndexStore {
            version: INDEX_VERSION,
            mods: BTreeMap::from([
                (1, index(&["/marvel/content/shared.uasset", "/one.uasset"])),
                (2, index(&["/marvel/content/shared.uasset", "/two.uasset"])),
            ]),
        };
        let report = build_report(&mods, &store);
        assert_eq!(report.summary.conflicting_asset_count, 1);
        assert_eq!(report.summary.affected_mod_count, 2);
        assert_eq!(report.mods[0].conflicts[0].other_mods[0].mod_name, "Two");
    }

    #[test]
    fn disabled_mods_do_not_create_active_conflicts() {
        let mods = vec![installed(1, "One", true), installed(2, "Two", false)];
        let store = AssetIndexStore {
            version: INDEX_VERSION,
            mods: BTreeMap::from([
                (1, index(&["/marvel/content/shared.uasset"])),
                (2, index(&["/marvel/content/shared.uasset"])),
            ]),
        };
        let report = build_report(&mods, &store);
        assert_eq!(report.summary.conflicting_asset_count, 0);
        assert_eq!(report.summary.affected_mod_count, 0);
    }

    #[test]
    fn preview_reports_exact_assets_owned_by_enabled_mods() {
        let mods = vec![
            installed(1, "Existing", true),
            installed(2, "Disabled", false),
        ];
        let store = AssetIndexStore {
            version: INDEX_VERSION,
            mods: BTreeMap::from([
                (1, index(&["/marvel/content/characters/shared.uasset"])),
                (2, index(&["/marvel/content/characters/shared.uasset"])),
            ]),
        };
        let report = build_preview_report(
            3,
            index(&[
                "/marvel/content/characters/shared.uasset",
                "/marvel/content/characters/new.uasset",
            ]),
            &mods,
            &store,
        );
        assert_eq!(report.scanned_asset_count, 2);
        assert_eq!(report.conflicting_asset_count, 1);
        assert_eq!(report.affected_mod_count, 1);
        assert_eq!(
            report.conflicts[0].asset_path,
            "/marvel/content/characters/shared.uasset"
        );
        assert_eq!(report.conflicts[0].other_mods[0].mod_name, "Existing");
    }

    #[test]
    fn preview_update_does_not_conflict_with_its_previous_version() {
        let mods = vec![installed(1, "Updating", true)];
        let store = AssetIndexStore {
            version: INDEX_VERSION,
            mods: BTreeMap::from([(1, index(&["/marvel/content/shared.uasset"]))]),
        };
        let report =
            build_preview_report(1, index(&["/marvel/content/shared.uasset"]), &mods, &store);
        assert_eq!(report.conflicting_asset_count, 0);
        assert_eq!(report.affected_mod_count, 0);
    }
}
