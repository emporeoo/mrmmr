use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, State};

use md5::{Digest, Md5};

use crate::auth::{self, AuthState};
use crate::game;
use crate::nexus;
use crate::storage;

const UTOC_MOD_ID: u32 = 2940;
const GAME_DOMAIN: &str = "marvelrivals";
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtocStatus {
    pub installed: bool,
    pub win64_dir: String,
    pub missing: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum UtocError {
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
    #[serde(rename = "missing_files")]
    MissingFiles(Vec<String>),
    #[serde(rename = "install")]
    Install(String),
    #[serde(rename = "game_files_locked")]
    GameFilesLocked,
}

fn ensure_game_files_mutable() -> Result<(), UtocError> {
    game::ensure_game_files_mutable().map_err(|error| match error {
        game::GameError::GameFilesLocked => UtocError::GameFilesLocked,
        other => UtocError::Storage(other.to_string()),
    })
}

#[derive(Debug, Deserialize)]
struct ModFile {
    file_id: u32,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
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

fn win64_dir(game_path: &str) -> PathBuf {
    PathBuf::from(game_path)
        .join("MarvelGame")
        .join("Marvel")
        .join("Binaries")
        .join("Win64")
}

pub fn check_installed(game_path: &str) -> UtocStatus {
    let dir = win64_dir(game_path);
    let mut missing = Vec::new();
    if !dir.join("dsound.dll").is_file() {
        missing.push("dsound.dll".to_string());
    }
    if !dir.join("plugins").is_dir() {
        missing.push("plugins".to_string());
    }
    UtocStatus {
        installed: missing.is_empty(),
        win64_dir: dir.to_string_lossy().into_owned(),
        missing,
    }
}

#[tauri::command]
pub fn utoc_status(app: AppHandle) -> Result<UtocStatus, UtocError> {
    let location = game::load_location(&app)
        .map_err(|e| UtocError::Storage(e.to_string()))?
        .ok_or(UtocError::GameNotFound)?;
    Ok(check_installed(&location.path))
}

/// Remove the UTOC Signature Bypass files from the game folder, if a game
/// location is known. Missing files are already considered uninstalled.
pub fn uninstall_utoc(app: &AppHandle) -> Result<(), UtocError> {
    if let Ok(Some(location)) = game::load_location(app) {
        let win64 = win64_dir(&location.path);
        let dll = win64.join("dsound.dll");
        let plugins = win64.join("plugins");
        if dll.exists() {
            std::fs::remove_file(&dll).map_err(|error| {
                UtocError::Storage(format!("Could not remove '{}': {error}", dll.display()))
            })?;
        }
        if plugins.exists() {
            std::fs::remove_dir_all(&plugins).map_err(|error| {
                UtocError::Storage(format!("Could not remove '{}': {error}", plugins.display()))
            })?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn install_utoc(
    app: AppHandle,
    state: State<'_, AuthState>,
) -> Result<UtocStatus, UtocError> {
    ensure_game_files_mutable()?;
    let credential = auth::resolve_credential(&app, &state).map_err(|e| {
        UtocError::NotAuthenticated(format!(
            "The saved Nexus authorization could not be used: {e:?}"
        ))
    })?;
    let location = game::load_location(&app)
        .map_err(|e| UtocError::Storage(e.to_string()))?
        .ok_or(UtocError::GameNotFound)?;
    let game_path = location.path;

    let temp_dir = storage::unique_temp_dir("mrmmr-utoc");
    let extract_dir = temp_dir.join("extracted");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| UtocError::Storage(format!("Could not create temp folder: {e}")))?;

    let result = install_inner(&app, &credential, &game_path, &temp_dir, &extract_dir).await;

    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

async fn install_inner(
    app: &AppHandle,
    credential: &str,
    game_path: &str,
    temp_dir: &Path,
    extract_dir: &Path,
) -> Result<UtocStatus, UtocError> {
    emit(app, "fetching_files");
    let files = get_mod_files(credential, UTOC_MOD_ID).await?;
    let file = pick_file(&files).ok_or(UtocError::NoFiles)?;
    let archive_path = temp_dir.join(format!(
        "utoc-bypass.{}",
        archive_extension_from_name(file.file_name.as_deref())
    ));

    emit(app, "fetching_download_link");
    let links = get_download_links(credential, UTOC_MOD_ID, file.file_id).await?;
    let uri = links
        .first()
        .map(|link| link.uri.clone())
        .ok_or(UtocError::NoDownloadLink)?;

    emit(app, "downloading");
    download_file(&uri, &archive_path).await?;

    let extraction_app = app.clone();
    let game_path = game_path.to_string();
    let extract_dir = extract_dir.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        extract_and_install(&extraction_app, &game_path, &archive_path, &extract_dir)
    })
    .await
    .map_err(|error| UtocError::Extract(format!("Extraction worker stopped: {error}")))?
}

fn extract_and_install(
    app: &AppHandle,
    game_path: &str,
    zip_path: &Path,
    extract_dir: &Path,
) -> Result<UtocStatus, UtocError> {
    emit(app, "extracting");
    std::fs::create_dir_all(extract_dir)
        .map_err(|e| UtocError::Storage(format!("Could not create temp folder: {e}")))?;
    extract_archive(zip_path, extract_dir)?;

    emit(app, "installing");
    let win64 = win64_dir(game_path);
    std::fs::create_dir_all(&win64).map_err(|e| {
        UtocError::Install(format!("Could not create the game's Win64 folder: {e}"))
    })?;

    let mut dsound = None;
    let mut plugins = None;
    find_required(extract_dir, &mut dsound, &mut plugins);

    let dsound = dsound.ok_or_else(|| UtocError::MissingFiles(vec!["dsound.dll".to_string()]))?;
    let plugins = plugins.ok_or_else(|| UtocError::MissingFiles(vec!["plugins".to_string()]))?;

    std::fs::copy(&dsound, win64.join("dsound.dll"))
        .map_err(|e| UtocError::Install(format!("Could not copy dsound.dll: {e}")))?;
    copy_dir_recursive(&plugins, &win64.join("plugins"))?;

    emit(app, "verifying");
    let status = check_installed(game_path);
    if !status.installed {
        return Err(UtocError::Install(
            "Installation could not be verified.".to_string(),
        ));
    }
    Ok(status)
}

#[tauri::command]
pub fn utoc_files_url() -> String {
    format!("https://www.nexusmods.com/{GAME_DOMAIN}/mods/{UTOC_MOD_ID}?tab=files")
}

#[tauri::command]
pub async fn utoc_install_from_archive(
    app: AppHandle,
    archive_path: String,
) -> Result<UtocStatus, UtocError> {
    ensure_game_files_mutable()?;
    let location = game::load_location(&app)
        .map_err(|e| UtocError::Storage(e.to_string()))?
        .ok_or(UtocError::GameNotFound)?;
    let game_path = location.path;

    let temp_dir = storage::unique_temp_dir("mrmmr-utoc");
    let extract_dir = temp_dir.join("extracted");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let extraction_app = app.clone();
    let archive_path = PathBuf::from(archive_path);
    let worker_extract_dir = extract_dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        extract_and_install(
            &extraction_app,
            &game_path,
            &archive_path,
            &worker_extract_dir,
        )
    })
    .await
    .map_err(|error| UtocError::Extract(format!("Extraction worker stopped: {error}")))?;
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

#[tauri::command]
pub fn utoc_detect_download() -> Result<Option<String>, UtocError> {
    const RECENT: std::time::Duration = std::time::Duration::from_secs(600);

    for dir in download_candidate_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;

        for entry in entries.flatten() {
            let path = entry.path();
            if !is_archive(&path) {
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
            if !archive_contains_mod(&path) {
                continue;
            }

            if newest
                .as_ref()
                .map(|(time, _)| modified > *time)
                .unwrap_or(true)
            {
                newest = Some((modified, path));
            }
        }

        if let Some((_, path)) = newest {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }

    Ok(None)
}

pub(crate) fn is_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "zip" | "7z" | "rar" | "tar" | "gz" | "tgz"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn archive_extension_from_name(file_name: Option<&str>) -> &'static str {
    match file_name
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("7z") => "7z",
        Some("rar") => "rar",
        Some("tar") => "tar",
        Some("gz") => "gz",
        Some("tgz") => "tgz",
        _ => "zip",
    }
}

fn archive_contains_mod(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "zip" => zip_contains_mod(path),
        "7z" => sevenz_contains_mod(path),
        "rar" => rar_contains_mod(path),
        _ => false,
    }
}

fn contains_required(name: &str, has_dsound: &mut bool, has_plugins: &mut bool) -> bool {
    let name = name.replace('\\', "/").to_ascii_lowercase();
    if name.ends_with("dsound.dll") {
        *has_dsound = true;
    }
    if name == "plugins"
        || name.starts_with("plugins/")
        || name.contains("/plugins/")
        || name.ends_with("/plugins")
    {
        *has_plugins = true;
    }
    *has_dsound && *has_plugins
}

fn zip_contains_mod(path: &Path) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) => return false,
    };

    let mut has_dsound = false;
    let mut has_plugins = false;
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index(index) else {
            continue;
        };
        if contains_required(entry.name(), &mut has_dsound, &mut has_plugins) {
            return true;
        }
    }
    false
}

fn sevenz_contains_mod(path: &Path) -> bool {
    let Ok(archive) = sevenz_rust::Archive::open(path) else {
        return false;
    };

    let mut has_dsound = false;
    let mut has_plugins = false;
    for entry in &archive.files {
        if contains_required(entry.name(), &mut has_dsound, &mut has_plugins) {
            return true;
        }
    }
    false
}

fn rar_contains_mod(path: &Path) -> bool {
    let Ok(open) = unrar::Archive::new(path).open_for_listing() else {
        return false;
    };

    let mut has_dsound = false;
    let mut has_plugins = false;
    for entry in open {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.filename.to_string_lossy();
        if contains_required(&name, &mut has_dsound, &mut has_plugins) {
            return true;
        }
    }
    false
}

pub(crate) fn download_candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(downloads) = downloads_dir() {
        dirs.push(downloads);
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        dirs.push(PathBuf::from(profile).join("Desktop"));
    }
    dirs
}

pub(crate) fn downloads_dir() -> Option<PathBuf> {
    const DOWNLOADS_GUID: &str = "{374DE290-123F-4565-9164-39C4925E467B}";
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(user_shell) =
        hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
    {
        if let Ok(value) = user_shell.get_value::<String, _>(DOWNLOADS_GUID) {
            candidates.push(expand_env_vars(&value));
        }
    }
    if let Ok(shell) =
        hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders")
    {
        if let Ok(value) = shell.get_value::<String, _>("Downloads") {
            candidates.push(expand_env_vars(&value));
        }
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        candidates.push(PathBuf::from(profile).join("Downloads"));
    }

    candidates
        .into_iter()
        .find(|path| !path.as_os_str().is_empty() && path.is_dir())
}

pub(crate) fn expand_env_vars(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix('%') {
        if let Some(end) = rest.find('%') {
            if let Some(value) = std::env::var_os(&rest[..end]) {
                let mut expanded = PathBuf::from(value);
                expanded.push(&rest[end + 1..]);
                return expanded;
            }
        }
    }
    PathBuf::from(path)
}

fn emit(app: &AppHandle, phase: &str) {
    let _ = app.emit("utoc-progress", phase);
}

fn pick_file(files: &[ModFile]) -> Option<&ModFile> {
    files
        .iter()
        .find(|file| {
            file.category_name
                .as_deref()
                .map(|name| name.eq_ignore_ascii_case("main"))
                .unwrap_or(false)
        })
        .or_else(|| files.first())
}

async fn nexus_get(credential: &str, url: &str) -> Result<reqwest::Response, UtocError> {
    if let Some(message) = nexus::rate_limit_cooldown() {
        return Err(UtocError::Api(message));
    }
    let response = nexus::http_client()
        .get(url)
        .header("apikey", credential)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| UtocError::Network(e.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let rate_limit =
        (status.as_u16() == 429).then(|| nexus::rate_limit_message(response.headers()));
    let body = response.text().await.unwrap_or_default();
    eprintln!("[install_utoc] Nexus API error: HTTP {status} for {url}: {body}");
    match status.as_u16() {
        401 | 403 => Err(UtocError::NotAuthenticated(format!(
            "Nexus Mods rejected the request (HTTP {status}): {body}"
        ))),
        404 => Err(UtocError::Api(format!(
            "Not found on Nexus Mods (HTTP 404): {body}"
        ))),
        429 => Err(UtocError::Api(rate_limit.unwrap_or_else(|| {
            "Nexus Mods rate limit reached. Please try again later.".to_string()
        }))),
        s => Err(UtocError::Api(format!(
            "Nexus Mods returned HTTP {s}: {body}"
        ))),
    }
}

async fn get_mod_files(credential: &str, mod_id: u32) -> Result<Vec<ModFile>, UtocError> {
    let url = format!("https://api.nexusmods.com/v1/games/{GAME_DOMAIN}/mods/{mod_id}/files.json");
    let response = nexus_get(credential, &url).await?;
    let parsed: FilesResponse = response
        .json()
        .await
        .map_err(|e| UtocError::Api(format!("Invalid files response: {e}")))?;
    Ok(parsed.files)
}

async fn get_download_links(
    credential: &str,
    mod_id: u32,
    file_id: u32,
) -> Result<Vec<DownloadLink>, UtocError> {
    let url = format!(
        "https://api.nexusmods.com/v1/games/{GAME_DOMAIN}/mods/{mod_id}/files/{file_id}/download_link.json"
    );
    let response = nexus_get(credential, &url).await?;
    response
        .json()
        .await
        .map_err(|e| UtocError::Api(format!("Invalid download link response: {e}")))
}

pub(crate) async fn download_file(uri: &str, dest: &Path) -> Result<String, UtocError> {
    let mut response = nexus::http_client()
        .get(uri)
        .timeout(std::time::Duration::from_secs(15 * 60))
        .send()
        .await
        .map_err(|e| UtocError::Download(format!("Could not download: {e}")))?;

    if !response.status().is_success() {
        return Err(UtocError::Download(format!(
            "Download failed (HTTP {})",
            response.status()
        )));
    }

    let file_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    let partial = dest.with_file_name(format!("{file_name}.part"));
    let mut file = std::fs::File::create(&partial)
        .map_err(|e| UtocError::Storage(format!("Could not create download: {e}")))?;
    let mut hasher = Md5::new();

    while let Some(chunk) = response.chunk().await.map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        UtocError::Download(format!("Could not read download: {e}"))
    })? {
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| {
            let _ = std::fs::remove_file(&partial);
            UtocError::Storage(format!("Could not write download: {e}"))
        })?;
    }
    file.flush().map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        UtocError::Storage(format!("Could not flush download: {e}"))
    })?;
    drop(file);
    std::fs::rename(&partial, dest).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        UtocError::Storage(format!("Could not finish download: {e}"))
    })?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn extract_archive(path: &Path, dest: &Path) -> Result<(), UtocError> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "zip" => extract_zip(path, dest),
        "7z" => extract_7z(path, dest),
        "rar" => extract_rar(path, dest),
        "tar" => extract_tar(path, dest),
        "gz" | "tgz" => extract_targz(path, dest),
        _ => Err(UtocError::Extract(format!(
            "Unsupported archive type: .{ext}"
        ))),
    }
}

fn extract_zip(path: &Path, dest: &Path) -> Result<(), UtocError> {
    let file = std::fs::File::open(path)
        .map_err(|e| UtocError::Extract(format!("Could not open download: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| UtocError::Extract(format!("Not a valid ZIP file: {e}")))?;
    archive
        .extract(dest)
        .map_err(|e| UtocError::Extract(format!("Could not extract ZIP: {e}")))?;
    Ok(())
}

fn extract_7z(path: &Path, dest: &Path) -> Result<(), UtocError> {
    std::fs::create_dir_all(dest)
        .map_err(|e| UtocError::Extract(format!("Could not create extract folder: {e}")))?;
    sevenz_rust::decompress_file(path, dest)
        .map_err(|e| UtocError::Extract(format!("Could not extract 7z archive: {e}")))?;
    Ok(())
}

fn extract_rar(path: &Path, dest: &Path) -> Result<(), UtocError> {
    std::fs::create_dir_all(dest)
        .map_err(|e| UtocError::Extract(format!("Could not create extract folder: {e}")))?;

    let mut open = unrar::Archive::new(path)
        .open_for_processing()
        .map_err(|e| UtocError::Extract(format!("Could not open rar archive: {e}")))?;

    while let Some(header) = open
        .read_header()
        .map_err(|e| UtocError::Extract(format!("Could not read rar archive: {e}")))?
    {
        open = if header.entry().is_file() {
            header
                .extract_with_base(dest)
                .map_err(|e| UtocError::Extract(format!("Could not extract rar archive: {e}")))?
        } else {
            header
                .skip()
                .map_err(|e| UtocError::Extract(format!("Could not skip rar entry: {e}")))?
        };
    }
    Ok(())
}

fn extract_tar(path: &Path, dest: &Path) -> Result<(), UtocError> {
    let file = std::fs::File::open(path)
        .map_err(|e| UtocError::Extract(format!("Could not open download: {e}")))?;
    let mut archive = tar::Archive::new(file);
    archive
        .unpack(dest)
        .map_err(|e| UtocError::Extract(format!("Could not extract tar archive: {e}")))?;
    Ok(())
}

fn extract_targz(path: &Path, dest: &Path) -> Result<(), UtocError> {
    let file = std::fs::File::open(path)
        .map_err(|e| UtocError::Extract(format!("Could not open download: {e}")))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest)
        .map_err(|e| UtocError::Extract(format!("Could not extract archive: {e}")))?;
    Ok(())
}

fn find_required(dir: &Path, dsound: &mut Option<PathBuf>, plugins: &mut Option<PathBuf>) {
    if dsound.is_some() && plugins.is_some() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase());

        if path.is_dir() {
            if plugins.is_none() && name.as_deref() == Some("plugins") {
                *plugins = Some(path.clone());
            }
            if dsound.is_none() || plugins.is_none() {
                find_required(&path, dsound, plugins);
            }
        } else if dsound.is_none() && name.as_deref() == Some("dsound.dll") {
            *dsound = Some(path.clone());
        }

        if dsound.is_some() && plugins.is_some() {
            return;
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), UtocError> {
    std::fs::create_dir_all(dst)
        .map_err(|e| UtocError::Install(format!("Could not create folder: {e}")))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| UtocError::Install(format!("Could not read folder: {e}")))?
        .flatten()
    {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| UtocError::Install(format!("Could not copy file: {e}")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nexus_download_link_uri_casing() {
        let uppercase: Vec<DownloadLink> =
            serde_json::from_str(r#"[{"URI":"https://premium-files.nexusmods.com/utoc.7z"}]"#)
                .unwrap();
        let lowercase: Vec<DownloadLink> =
            serde_json::from_str(r#"[{"uri":"https://premium-files.nexusmods.com/utoc.7z"}]"#)
                .unwrap();
        assert_eq!(uppercase[0].uri, lowercase[0].uri);
    }

    #[test]
    fn detects_missing_files() {
        let dir = std::env::temp_dir().join("mrmmr_utoc_missing");
        let status = check_installed(dir.to_string_lossy().as_ref());
        assert!(!status.installed);
        assert_eq!(status.missing, vec!["dsound.dll", "plugins"]);
    }

    #[test]
    fn detects_installed_files() {
        let root = std::env::temp_dir().join("mrmmr_utoc_installed");
        let win64 = root
            .join("MarvelGame")
            .join("Marvel")
            .join("Binaries")
            .join("Win64");
        std::fs::create_dir_all(&win64).unwrap();
        std::fs::write(win64.join("dsound.dll"), b"").unwrap();
        std::fs::create_dir_all(win64.join("plugins")).unwrap();

        let status = check_installed(root.to_string_lossy().as_ref());
        assert!(status.installed);
        assert!(status.missing.is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn finds_required_files_in_nested_zip() {
        let root = std::env::temp_dir().join("mrmmr_utoc_extract");
        let nested = root.join("sub").join("files");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("dsound.dll"), b"").unwrap();
        std::fs::create_dir_all(nested.join("plugins")).unwrap();

        let mut dsound = None;
        let mut plugins = None;
        find_required(&root, &mut dsound, &mut plugins);
        assert!(dsound.is_some());
        assert!(plugins.is_some());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prefers_main_file() {
        let files = vec![
            ModFile {
                file_id: 1,
                category_name: Some("OPTIONAL".into()),
                file_name: None,
            },
            ModFile {
                file_id: 2,
                category_name: Some("MAIN".into()),
                file_name: Some("bypass.7z".into()),
            },
        ];
        let picked = pick_file(&files).unwrap();
        assert_eq!(picked.file_id, 2);
    }

    #[test]
    fn extracts_deflate_zip() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("mrmmr_utoc_zip_test");
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("test.zip");

        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("root/dsound.dll", options).unwrap();
            writer.write_all(b"dummy").unwrap();
            writer.start_file("root/plugins/x.dll", options).unwrap();
            writer.write_all(b"dummy2").unwrap();
            writer.finish().unwrap();
        }

        let extract_dir = dir.join("out");
        extract_archive(&zip_path, &extract_dir).unwrap();
        assert!(extract_dir.join("root").join("dsound.dll").is_file());
        assert!(extract_dir
            .join("root")
            .join("plugins")
            .join("x.dll")
            .is_file());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolves_downloads_dir() {
        let dir = downloads_dir();
        assert!(dir.is_some(), "Downloads folder should resolve");
        assert!(
            dir.unwrap().is_dir(),
            "resolved Downloads path should exist"
        );
    }

    #[test]
    fn preserves_supported_nexus_archive_extensions() {
        assert_eq!(archive_extension_from_name(Some("mod.7z")), "7z");
        assert_eq!(archive_extension_from_name(Some("mod.RAR")), "rar");
        assert_eq!(archive_extension_from_name(Some("mod.tar.gz")), "gz");
        assert_eq!(archive_extension_from_name(Some("mod.exe")), "zip");
        assert_eq!(archive_extension_from_name(None), "zip");
    }
}
