use crate::storage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const PREFERENCES_FILE: &str = "preferences.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub auto_delete_mod_archives: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum PreferencesError {
    #[serde(rename = "storage")]
    Storage(String),
}

fn preferences_path(app: &AppHandle) -> Result<PathBuf, PreferencesError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(PREFERENCES_FILE))
        .map_err(|e| PreferencesError::Storage(format!("Could not resolve config directory: {e}")))
}

pub(crate) fn load(app: &AppHandle) -> Result<Preferences, PreferencesError> {
    let path = preferences_path(app)?;
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|e| PreferencesError::Storage(format!("Could not parse preferences: {e}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(error) => Err(PreferencesError::Storage(format!(
            "Could not read preferences: {error}"
        ))),
    }
}

fn save(app: &AppHandle, preferences: &Preferences) -> Result<(), PreferencesError> {
    let path = preferences_path(app)?;
    let directory = path.parent().ok_or_else(|| {
        PreferencesError::Storage("Could not resolve preferences directory.".to_string())
    })?;
    std::fs::create_dir_all(directory).map_err(|e| {
        PreferencesError::Storage(format!("Could not create preferences directory: {e}"))
    })?;
    storage::write_json_atomic(&path, preferences)
        .map_err(|e| PreferencesError::Storage(format!("Could not save preferences: {e}")))
}

#[tauri::command]
pub fn get_preferences(app: AppHandle) -> Result<Preferences, PreferencesError> {
    load(&app)
}

#[tauri::command]
pub fn set_auto_delete_mod_archives(
    app: AppHandle,
    enabled: bool,
) -> Result<Preferences, PreferencesError> {
    let mut preferences = load(&app)?;
    preferences.auto_delete_mod_archives = enabled;
    save(&app, &preferences)?;
    Ok(preferences)
}

pub(crate) fn clear(app: &AppHandle) {
    if let Ok(path) = preferences_path(app) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_or_empty_preferences_default_to_safe_cleanup_behavior() {
        let preferences: Preferences = serde_json::from_str("{}").unwrap();
        assert!(!preferences.auto_delete_mod_archives);
        assert!(!Preferences::default().auto_delete_mod_archives);
    }
}
