use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};
use winreg::RegKey;

const GAME_FOLDER_NAME: &str = "Marvel Rivals";
const STEAM_APP_ID: &str = "2767030";
const STEAM_FALLBACKS: [&str; 2] = [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"];
const SETTINGS_FILE: &str = "settings.json";
const PLATFORM_PROCESSES: [&str; 2] = ["steam.exe", "epicgameslauncher.exe"];
const GAME_PROCESSES: [&str; 3] = [
    "marvel.exe",
    "marvelrivals_launcher.exe",
    "marvel-win64-shipping.exe",
];
static LAUNCH_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameLocation {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameProcessStatus {
    pub game_running: bool,
    pub shipping_running: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum GameError {
    #[serde(rename = "not_a_game")]
    NotAGame,
    #[serde(rename = "platform_not_running")]
    PlatformNotRunning,
    #[serde(rename = "game_already_running")]
    GameAlreadyRunning,
    #[serde(rename = "game_files_locked")]
    GameFilesLocked,
    #[serde(rename = "storage")]
    Storage(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameError::NotAGame => write!(f, "Not a Marvel Rivals installation"),
            GameError::PlatformNotRunning => {
                write!(f, "Steam or Epic Games Launcher is not running")
            }
            GameError::GameAlreadyRunning => write!(f, "Marvel Rivals is already running"),
            GameError::GameFilesLocked => {
                write!(f, "Close Marvel Rivals before changing game files")
            }
            GameError::Storage(message) => write!(f, "{message}"),
        }
    }
}

pub(crate) fn process_status() -> Result<GameProcessStatus, GameError> {
    let processes = running_process_names()?;
    Ok(GameProcessStatus {
        game_running: GAME_PROCESSES.iter().any(|name| processes.contains(*name)),
        shipping_running: processes.contains("marvel-win64-shipping.exe"),
    })
}

pub(crate) fn ensure_game_files_mutable() -> Result<(), GameError> {
    if process_status()?.shipping_running {
        Err(GameError::GameFilesLocked)
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn get_game_process_status() -> Result<GameProcessStatus, GameError> {
    process_status()
}

#[tauri::command]
pub fn get_game_location(app: AppHandle) -> Result<Option<GameLocation>, GameError> {
    let location = load_location(&app)?;
    if location
        .as_ref()
        .is_some_and(|saved| is_game_installation(Path::new(&saved.path)))
    {
        return Ok(location);
    }
    if location.is_some() {
        clear_location(&app);
    }
    Ok(None)
}

#[tauri::command]
pub fn ensure_mods_folder(app: AppHandle) -> Result<String, GameError> {
    let location = load_location(&app)?
        .ok_or_else(|| GameError::Storage("Marvel Rivals hasn't been located yet.".to_string()))?;
    let game_root = PathBuf::from(&location.path);
    if !is_game_installation(&game_root) {
        return Err(GameError::Storage(
            "The saved Marvel Rivals location is no longer available.".to_string(),
        ));
    }
    let mods_dir = game_root
        .join("MarvelGame")
        .join("Marvel")
        .join("Content")
        .join("Paks")
        .join("~mods");
    std::fs::create_dir_all(&mods_dir)
        .map_err(|e| GameError::Storage(format!("Could not create the ~mods folder: {e}")))?;
    Ok(mods_dir.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn open_mods_folder(app: AppHandle) -> Result<String, GameError> {
    let path = ensure_mods_folder(app.clone())?;
    app.opener()
        .open_path(path.clone(), None::<String>)
        .map_err(|e| GameError::Storage(format!("Could not open the ~mods folder: {e}")))?;
    Ok(path)
}

pub fn clear_location(app: &AppHandle) {
    if let Ok(dir) = app.path().app_config_dir() {
        let _ = std::fs::remove_file(dir.join(SETTINGS_FILE));
    }
}

#[tauri::command]
pub fn close_game() -> Result<(), GameError> {
    if !terminate_process("Marvel-Win64-Shipping.exe")? {
        return Err(GameError::Storage(
            "Marvel Rivals is not running.".to_string(),
        ));
    }
    Ok(())
}

/// Terminate every process with the exact executable name. Returns whether a
/// matching process was found and reports access/termination failures.
fn terminate_process(name: &str) -> Result<bool, GameError> {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let wanted = name.to_ascii_lowercase();
    let snapshot = Snapshot(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|error| {
            GameError::Storage(format!("Could not inspect running applications: {error}"))
        })?,
    );
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe { Process32FirstW(snapshot.0, &mut entry) }.map_err(|error| {
        GameError::Storage(format!("Could not inspect running applications: {error}"))
    })?;

    let mut found = false;
    loop {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable = String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase();
        if executable == wanted {
            found = true;
            let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, entry.th32ProcessID) }
                .map_err(|error| {
                    GameError::Storage(format!(
                        "Could not access Marvel Rivals to close it: {error}"
                    ))
                })?;
            let result = unsafe { TerminateProcess(handle, 1) };
            unsafe {
                let _ = CloseHandle(handle);
            }
            result.map_err(|error| {
                GameError::Storage(format!("Could not close Marvel Rivals: {error}"))
            })?;
        }

        match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => break,
            Err(error) => {
                return Err(GameError::Storage(format!(
                    "Could not finish inspecting running applications: {error}"
                )));
            }
        }
    }
    Ok(found)
}

#[tauri::command]
pub fn launch_game(app: AppHandle) -> Result<String, GameError> {
    let _launch_guard = LAUNCH_LOCK
        .lock()
        .map_err(|_| GameError::Storage("Could not lock the game launcher.".to_string()))?;
    let location = load_location(&app)?
        .ok_or_else(|| GameError::Storage("Marvel Rivals hasn't been located yet.".to_string()))?;
    let game_dir = PathBuf::from(&location.path);
    let launcher = game_dir.join("MarvelRivals_Launcher.exe");
    if !launcher.is_file() {
        return Err(GameError::Storage(
            "MarvelRivals_Launcher.exe couldn't be found in the game folder.".to_string(),
        ));
    }
    validate_launch_processes(&running_process_names()?)?;
    let launcher_str = launcher.to_string_lossy().into_owned();

    match std::process::Command::new(&launcher)
        .current_dir(&game_dir)
        .spawn()
    {
        Ok(_) => Ok(launcher_str),
        // ERROR_ELEVATION_REQUIRED — the launcher needs admin rights.
        Err(e) if e.raw_os_error() == Some(740) => {
            launch_elevated(&launcher, &game_dir)?;
            Ok(launcher_str)
        }
        Err(e) => Err(GameError::Storage(format!(
            "Could not launch the game: {e}"
        ))),
    }
}

fn validate_launch_processes(processes: &HashSet<String>) -> Result<(), GameError> {
    if !PLATFORM_PROCESSES
        .iter()
        .any(|name| processes.contains(*name))
    {
        return Err(GameError::PlatformNotRunning);
    }
    if GAME_PROCESSES.iter().any(|name| processes.contains(*name)) {
        return Err(GameError::GameAlreadyRunning);
    }
    Ok(())
}

fn running_process_names() -> Result<HashSet<String>, GameError> {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let snapshot = Snapshot(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|error| {
            GameError::Storage(format!("Could not inspect running applications: {error}"))
        })?,
    );
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe { Process32FirstW(snapshot.0, &mut entry) }.map_err(|error| {
        GameError::Storage(format!("Could not inspect running applications: {error}"))
    })?;

    let mut names = HashSet::new();
    loop {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        names.insert(String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase());

        match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => {
                break;
            }
            Err(error) => {
                return Err(GameError::Storage(format!(
                    "Could not finish inspecting running applications: {error}"
                )));
            }
        }
    }
    Ok(names)
}

fn launch_elevated(launcher: &Path, game_dir: &Path) -> Result<(), GameError> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn to_wstr(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let file = to_wstr(&launcher.to_string_lossy());
    let dir = to_wstr(&game_dir.to_string_lossy());
    let verb = to_wstr("runas");

    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR(dir.as_ptr()),
            SW_SHOWNORMAL,
        );
        if result.0 as isize <= 32 {
            return Err(GameError::Storage(format!(
                "Could not launch the game with elevation (error {}).",
                result.0 as isize
            )));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn detect_game(app: AppHandle) -> Result<Option<GameLocation>, GameError> {
    let location = tauri::async_runtime::spawn_blocking(|| detect_steam().or_else(detect_epic))
        .await
        .map_err(|error| GameError::Storage(format!("Game detection stopped: {error}")))?;
    if let Some(location) = location {
        save_location(&app, &location)?;
        return Ok(Some(location));
    }

    Ok(None)
}

#[tauri::command]
pub fn save_game_location(app: AppHandle, path: String) -> Result<GameLocation, GameError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(GameError::NotAGame);
    }

    let dir = PathBuf::from(path);
    let location = game_location(&dir, "manual").ok_or(GameError::NotAGame)?;
    save_location(&app, &location)?;
    Ok(location)
}

fn steam_library_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(steam) = hkcu
        .open_subkey(r"Software\Valve\Steam")
        .and_then(|key| key.get_value::<String, _>("SteamPath"))
    {
        push_unique_path(&mut roots, PathBuf::from(steam));
    }
    for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
        if let Ok(path) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ | view)
            .and_then(|key| key.get_value::<String, _>("InstallPath"))
        {
            push_unique_path(&mut roots, PathBuf::from(path));
        }
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = std::env::var_os(variable) {
            push_unique_path(&mut roots, PathBuf::from(path).join("Steam"));
        }
    }
    for fallback in STEAM_FALLBACKS {
        push_unique_path(&mut roots, PathBuf::from(fallback));
    }
    roots
}

fn detect_steam() -> Option<GameLocation> {
    detect_steam_in_roots(&steam_library_roots())
}

fn detect_steam_in_roots(roots: &[PathBuf]) -> Option<GameLocation> {
    let mut steamapps_dirs = Vec::new();
    for root in roots {
        let steamapps = if root
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
        {
            root.clone()
        } else {
            root.join("steamapps")
        };
        push_unique_path(&mut steamapps_dirs, steamapps.clone());
        let Some(contents) =
            read_small_text(&steamapps.join("libraryfolders.vdf"), 4 * 1024 * 1024)
        else {
            continue;
        };
        for library in parse_libraryfolders(&contents) {
            let library = PathBuf::from(library);
            let library_apps = if library
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
            {
                library
            } else {
                library.join("steamapps")
            };
            push_unique_path(&mut steamapps_dirs, library_apps);
        }
    }

    for steamapps in steamapps_dirs {
        let manifest = steamapps.join(format!("appmanifest_{STEAM_APP_ID}.acf"));
        if let Some(contents) = read_small_text(&manifest, 1024 * 1024) {
            if let Some(install_dir) = parse_vdf_string(&contents, "installdir") {
                if let Some(location) =
                    game_location(&steamapps.join("common").join(install_dir), "steam")
                {
                    return Some(location);
                }
            }
        }
        if let Some(location) =
            game_location(&steamapps.join("common").join(GAME_FOLDER_NAME), "steam")
        {
            return Some(location);
        }
    }
    None
}

fn detect_epic() -> Option<GameLocation> {
    let mut data_roots = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        push_unique_path(&mut data_roots, PathBuf::from(program_data).join("Epic"));
    }
    push_unique_path(&mut data_roots, PathBuf::from(r"C:\ProgramData\Epic"));

    if let Some(location) = detect_epic_in_data_roots(&data_roots) {
        return Some(location);
    }

    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Some(root) = std::env::var_os(variable).map(PathBuf::from) else {
            continue;
        };
        for candidate in [
            root.join("Epic Games").join("MarvelRivals"),
            root.join("Epic Games").join(GAME_FOLDER_NAME),
            root.join("MarvelRivals"),
            root.join(GAME_FOLDER_NAME),
        ] {
            if let Some(location) = game_location(&candidate, "epic") {
                return Some(location);
            }
        }
    }
    None
}

fn detect_epic_in_data_roots(data_roots: &[PathBuf]) -> Option<GameLocation> {
    for data_root in data_roots {
        let manifests = data_root
            .join("EpicGamesLauncher")
            .join("Data")
            .join("Manifests");
        if let Ok(entries) = std::fs::read_dir(manifests) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
                {
                    continue;
                }
                if let Some(location) = epic_location_from_json(&path) {
                    return Some(location);
                }
            }
        }
        if let Some(location) = epic_location_from_json(
            &data_root
                .join("UnrealEngineLauncher")
                .join("LauncherInstalled.dat"),
        ) {
            return Some(location);
        }
    }
    None
}

fn epic_location_from_json(path: &Path) -> Option<GameLocation> {
    let contents = read_small_text(path, 4 * 1024 * 1024)?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let mut locations = Vec::new();
    collect_install_locations(&value, &mut locations);
    locations
        .into_iter()
        .find_map(|location| game_location(Path::new(location), "epic"))
}

fn collect_install_locations<'a>(value: &'a serde_json::Value, locations: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::Object(values) => {
            if let Some(location) = values
                .get("InstallLocation")
                .and_then(|value| value.as_str())
            {
                locations.push(location);
            }
            for value in values.values() {
                collect_install_locations(value, locations);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_install_locations(value, locations);
            }
        }
        _ => {}
    }
}

fn is_game_installation(dir: &Path) -> bool {
    dir.is_dir()
        && dir.join("MarvelRivals_Launcher.exe").is_file()
        && dir
            .join("MarvelGame")
            .join("Marvel")
            .join("Content")
            .join("Paks")
            .is_dir()
}

fn game_location(dir: &Path, source: &str) -> Option<GameLocation> {
    if !is_game_installation(dir) {
        return None;
    }
    let normalized = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let normalized = normalized.to_string_lossy();
    let normalized = normalized
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .or_else(|| normalized.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| normalized.into_owned());
    Some(GameLocation {
        path: normalized,
        source: source.to_string(),
    })
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    let key = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    if !paths.iter().any(|existing| {
        existing
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
            == key
    }) {
        paths.push(path);
    }
}

fn read_small_text(path: &Path, limit: u64) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn parse_vdf_string(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let mut quoted = line.split('"');
        let _ = quoted.next();
        let candidate_key = quoted.next()?;
        let _ = quoted.next();
        let value = quoted.next()?;
        candidate_key
            .eq_ignore_ascii_case(key)
            .then(|| value.replace("\\\\", "\\"))
    })
}

pub fn load_location(app: &AppHandle) -> Result<Option<GameLocation>, GameError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| GameError::Storage(format!("Could not resolve config directory: {e}")))?;
    let path = dir.join(SETTINGS_FILE);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(GameError::Storage(format!(
                "Could not read settings file: {e}"
            )))
        }
    };
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|e| GameError::Storage(format!("Could not parse settings file: {e}")))
}

pub fn save_location(app: &AppHandle, location: &GameLocation) -> Result<(), GameError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| GameError::Storage(format!("Could not resolve config directory: {e}")))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| GameError::Storage(format!("Could not create config directory: {e}")))?;
    crate::storage::write_json_atomic(&dir.join(SETTINGS_FILE), location)
        .map_err(|e| GameError::Storage(format!("Could not write settings file: {e}")))
}

fn find_subslice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| start + position)
}

fn parse_libraryfolders(contents: &str) -> Vec<String> {
    const KEY: &[u8] = b"\"path\"";
    let bytes = contents.as_bytes();
    let mut index = 0;
    let mut paths = Vec::new();

    while index < bytes.len() {
        let Some(key_start) = find_subslice(bytes, KEY, index) else {
            break;
        };
        let after_key = key_start + KEY.len();
        let Some(quote) = find_subslice(bytes, b"\"", after_key) else {
            break;
        };
        let value_start = quote + 1;

        let mut cursor = value_start;
        let mut value = Vec::new();
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' => break,
                b'\\' if cursor + 1 < bytes.len() => {
                    value.push(bytes[cursor + 1]);
                    cursor += 2;
                }
                byte => {
                    value.push(byte);
                    cursor += 1;
                }
            }
        }

        let value = String::from_utf8_lossy(&value).into_owned();
        if !value.is_empty() {
            paths.push(value);
        }
        index = cursor;
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(prefix: &str) -> PathBuf {
        crate::storage::unique_temp_dir(prefix)
    }

    fn create_game(dir: &Path) {
        std::fs::create_dir_all(
            dir.join("MarvelGame")
                .join("Marvel")
                .join("Content")
                .join("Paks"),
        )
        .unwrap();
        std::fs::write(dir.join("MarvelRivals_Launcher.exe"), b"").unwrap();
    }

    #[test]
    fn parses_libraryfolders_paths() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam\\steamapps"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"label"		""
	}
}
"#;
        let paths = parse_libraryfolders(vdf);
        assert_eq!(
            paths,
            vec![
                r"C:\Program Files (x86)\Steam\steamapps",
                r"D:\SteamLibrary",
            ]
        );
    }

    #[test]
    fn rejects_empty_folder_even_when_name_matches() {
        let dir = test_dir("Marvel Rivals");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_game_installation(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recognises_game_folder_by_executable() {
        let dir = test_dir("SomeGame");
        create_game(&dir);
        assert!(is_game_installation(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detects_game_in_standard_steam_location() {
        let root = test_dir("mrmmr-test-std-steam");
        let game = root.join("steamapps").join("common").join("Marvel Rivals");
        create_game(&game);

        let found = detect_steam_in_roots(std::slice::from_ref(&root));
        let location = found.expect("game should be found in the standard location");
        assert_eq!(location.source, "steam");
        assert_eq!(location.path, game.to_string_lossy());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn detects_game_via_libraryfolders_vdf() {
        let root = test_dir("mrmmr-test-vdf-steam");
        let library = test_dir("mrmmr-test-vdf-library");
        let game = library
            .join("steamapps")
            .join("common")
            .join("Marvel Rivals");
        create_game(&game);

        let vdf_dir = root.join("steamapps");
        std::fs::create_dir_all(&vdf_dir).unwrap();
        let escaped = library.display().to_string().replace('\\', "\\\\");
        std::fs::write(
            vdf_dir.join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{escaped}\"\n\t}}\n}}\n"
            ),
        )
        .unwrap();

        let found = detect_steam_in_roots(std::slice::from_ref(&root));
        let location = found.expect("game should be found via libraryfolders.vdf");
        assert_eq!(location.source, "steam");
        assert_eq!(location.path, game.to_string_lossy());

        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&library).unwrap();
    }

    #[test]
    fn detects_nonstandard_steam_install_dir_from_app_manifest() {
        let root = test_dir("mrmmr-test-steam-manifest");
        let game = root.join("steamapps").join("common").join("RivalsCustom");
        create_game(&game);
        let steamapps = root.join("steamapps");
        std::fs::write(
            steamapps.join(format!("appmanifest_{STEAM_APP_ID}.acf")),
            r#""AppState"
{
    "appid" "2767030"
    "installdir" "RivalsCustom"
}"#,
        )
        .unwrap();

        let found = detect_steam_in_roots(std::slice::from_ref(&root)).unwrap();
        assert_eq!(PathBuf::from(found.path), game);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_epic_install_from_item_manifest() {
        let data_root = test_dir("mrmmr-test-epic-item");
        let game = test_dir("mrmmr-test-epic-game");
        create_game(&game);
        let manifests = data_root
            .join("EpicGamesLauncher")
            .join("Data")
            .join("Manifests");
        std::fs::create_dir_all(&manifests).unwrap();
        std::fs::write(
            manifests.join("rivals.item"),
            serde_json::json!({ "InstallLocation": game }).to_string(),
        )
        .unwrap();

        let found = detect_epic_in_data_roots(std::slice::from_ref(&data_root)).unwrap();
        assert_eq!(PathBuf::from(found.path), game);
        std::fs::remove_dir_all(data_root).unwrap();
        std::fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn detects_epic_install_from_launcher_inventory() {
        let data_root = test_dir("mrmmr-test-epic-inventory");
        let game = test_dir("mrmmr-test-epic-inventory-game");
        create_game(&game);
        let inventory = data_root.join("UnrealEngineLauncher");
        std::fs::create_dir_all(&inventory).unwrap();
        std::fs::write(
            inventory.join("LauncherInstalled.dat"),
            serde_json::json!({
                "InstallationList": [{ "InstallLocation": game }]
            })
            .to_string(),
        )
        .unwrap();

        let found = detect_epic_in_data_roots(std::slice::from_ref(&data_root)).unwrap();
        assert_eq!(PathBuf::from(found.path), game);
        std::fs::remove_dir_all(data_root).unwrap();
        std::fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn launch_process_classification_is_exact_and_case_insensitive() {
        let processes = ["STEAM.EXE"]
            .into_iter()
            .map(str::to_ascii_lowercase)
            .collect();
        assert!(validate_launch_processes(&processes).is_ok());

        for game in GAME_PROCESSES {
            let processes = ["epicgameslauncher.exe", game]
                .into_iter()
                .map(str::to_ascii_lowercase)
                .collect();
            assert!(matches!(
                validate_launch_processes(&processes),
                Err(GameError::GameAlreadyRunning)
            ));
        }

        let unrelated = ["steamservice.exe", "marvel-helper.exe"]
            .into_iter()
            .map(str::to_ascii_lowercase)
            .collect();
        assert!(matches!(
            validate_launch_processes(&unrelated),
            Err(GameError::PlatformNotRunning)
        ));
    }
}
