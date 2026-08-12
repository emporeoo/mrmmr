use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

use crate::game;
use crate::install;
use crate::nexus::{self, AuthError, NexusUser};
use crate::preferences;
use crate::secure;
use crate::storage;
use crate::utoc;

const PROFILE_FILE: &str = "auth.json";
const KEY_FILE: &str = "auth.key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    #[serde(skip_serializing)]
    pub api_key: String,
    pub user: NexusUser,
    pub remembered: bool,
}

/// Non-sensitive data persisted to disk. The API key itself is stored as a
/// DPAPI-encrypted blob (see [`crate::secure`]), never as plaintext.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredProfile {
    user: NexusUser,
}

#[derive(Default)]
pub struct AuthState(pub Mutex<Option<AuthSession>>);

fn config_dir(app: &AppHandle) -> Result<PathBuf, AuthError> {
    app.path()
        .app_config_dir()
        .map_err(|e| AuthError::Storage(format!("Could not resolve config directory: {e}")))
}

fn profile_path(dir: &Path) -> PathBuf {
    dir.join(PROFILE_FILE)
}

fn key_path(dir: &Path) -> PathBuf {
    dir.join(KEY_FILE)
}

fn ensure_dir(dir: &Path) -> Result<(), AuthError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| AuthError::Storage(format!("Could not create config directory: {e}")))
}

fn load_profile(dir: &Path) -> Result<Option<StoredProfile>, AuthError> {
    let path = profile_path(dir);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AuthError::Storage(format!("Could not read auth file: {e}"))),
    };
    let profile: StoredProfile = serde_json::from_str(&contents)
        .map_err(|e| AuthError::Storage(format!("Could not parse auth file: {e}")))?;
    // Scrub the legacy plaintext key once, without rewriting the profile on
    // every cold start.
    if contents.contains("\"key\"") {
        save_profile(dir, &profile)?;
    }
    Ok(Some(profile))
}

fn save_profile(dir: &Path, profile: &StoredProfile) -> Result<(), AuthError> {
    storage::write_json_atomic(&profile_path(dir), profile)
        .map_err(|e| AuthError::Storage(format!("Could not write auth file: {e}")))
}

fn delete_profile(dir: &Path) {
    let _ = std::fs::remove_file(profile_path(dir));
}

fn save_key(dir: &Path, api_key: &str) -> Result<(), AuthError> {
    let ciphertext = secure::encrypt(api_key)?;
    storage::write_bytes_atomic(&key_path(dir), &ciphertext)
        .map_err(|e| AuthError::Storage(format!("Could not write key file: {e}")))
}

fn load_key(dir: &Path) -> Result<Option<String>, AuthError> {
    let ciphertext = match std::fs::read(key_path(dir)) {
        Ok(ciphertext) => ciphertext,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AuthError::Storage(format!("Could not read key file: {e}"))),
    };
    secure::decrypt(&ciphertext).map(Some)
}

fn delete_key(dir: &Path) {
    let _ = std::fs::remove_file(key_path(dir));
}

fn persist_session(app: &AppHandle, session: &AuthSession) -> Result<(), AuthError> {
    let dir = config_dir(app)?;
    ensure_dir(&dir)?;
    save_key(&dir, &session.api_key)?;
    save_profile(
        &dir,
        &StoredProfile {
            user: session.user.clone(),
        },
    )
}

fn remove_persisted(app: &AppHandle) {
    if let Ok(dir) = config_dir(app) {
        delete_key(&dir);
        delete_profile(&dir);
    }
}

fn set_session(state: &State<'_, AuthState>, session: Option<AuthSession>) {
    *state.0.lock().expect("auth state mutex poisoned") = session;
}

pub fn current_api_key(state: &State<'_, AuthState>) -> Option<String> {
    state
        .0
        .lock()
        .expect("auth state mutex poisoned")
        .as_ref()
        .map(|session| session.api_key.clone())
}

/// Resolve the API key from the in-memory session, falling back to the
/// persisted credential so that commands work even before the session has
/// been fully restored into memory.
pub fn resolve_api_key(app: &AppHandle, state: &State<'_, AuthState>) -> Result<String, AuthError> {
    if let Some(api_key) = current_api_key(state) {
        return Ok(api_key);
    }
    let dir = config_dir(app)?;
    match load_key(&dir)? {
        Some(api_key) => Ok(api_key),
        None => Err(AuthError::EmptyApiKey),
    }
}

fn session_from_storage(
    app: &AppHandle,
    state: &State<'_, AuthState>,
) -> Result<Option<AuthSession>, AuthError> {
    let in_memory = state.0.lock().expect("auth state mutex poisoned").clone();
    if let Some(session) = in_memory {
        return Ok(Some(session));
    }

    let dir = config_dir(app)?;
    let profile = match load_profile(&dir)? {
        Some(profile) => profile,
        None => return Ok(None),
    };

    let api_key = match load_key(&dir)? {
        Some(key) => key,
        None => {
            // Stale profile without a stored key — clear it.
            delete_profile(&dir);
            return Ok(None);
        }
    };

    let session = AuthSession {
        api_key,
        user: profile.user,
        remembered: true,
    };
    set_session(state, Some(session.clone()));
    Ok(Some(session))
}

#[tauri::command]
pub async fn authenticate(
    app: AppHandle,
    state: State<'_, AuthState>,
    api_key: String,
    remember: bool,
) -> Result<AuthSession, AuthError> {
    let user = nexus::validate_api_key(&api_key).await?;
    let session = AuthSession {
        api_key: api_key.trim().to_string(),
        user,
        remembered: remember,
    };

    if remember {
        persist_session(&app, &session)?;
    } else {
        remove_persisted(&app);
    }

    set_session(&state, Some(session.clone()));
    Ok(session)
}

#[tauri::command]
pub async fn get_auth_session(
    app: AppHandle,
    state: State<'_, AuthState>,
) -> Result<Option<AuthSession>, AuthError> {
    session_from_storage(&app, &state)
}

#[tauri::command]
pub async fn refresh_auth_session(
    app: AppHandle,
    state: State<'_, AuthState>,
) -> Result<Option<AuthSession>, AuthError> {
    let session = match session_from_storage(&app, &state)? {
        Some(session) => session,
        None => return Ok(None),
    };

    let user = match nexus::validate_api_key(&session.api_key).await {
        Ok(user) => user,
        Err(AuthError::InvalidApiKey) => {
            remove_persisted(&app);
            set_session(&state, None);
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    let updated = AuthSession { user, ..session };
    if updated.remembered {
        persist_session(&app, &updated)?;
    }
    set_session(&state, Some(updated.clone()));
    Ok(Some(updated))
}

#[tauri::command]
pub fn clear_auth(app: AppHandle, state: State<'_, AuthState>) -> Result<(), AuthError> {
    remove_persisted(&app);
    set_session(&state, None);
    Ok(())
}

/// Factory reset: uninstall the UTOC bypass, remove downloaded mods, forget the
/// game location, and remove the saved credentials.
#[tauri::command]
pub fn reset_all_data(app: AppHandle, state: State<'_, AuthState>) -> Result<(), AuthError> {
    game::ensure_game_files_mutable().map_err(|error| match error {
        game::GameError::GameFilesLocked => AuthError::GameFilesLocked,
        other => AuthError::Storage(other.to_string()),
    })?;
    utoc::uninstall_utoc(&app).map_err(|error| AuthError::Storage(format!("{error:?}")))?;
    install::remove_mods(&app).map_err(|error| AuthError::Storage(format!("{error:?}")))?;
    game::clear_location(&app);
    preferences::clear(&app);
    remove_persisted(&app);
    set_session(&state, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        std::env::temp_dir().join("mrmmr_auth_test_dir")
    }

    #[test]
    fn dpapi_round_trip() {
        let key = "some-personal-api-key-123";
        let ciphertext = secure::encrypt(key).unwrap();
        assert_ne!(ciphertext, key.as_bytes());
        assert_eq!(secure::decrypt(&ciphertext).unwrap(), key);
    }

    #[test]
    fn dpapi_file_round_trip() {
        let dir = test_dir();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let key = "some-personal-api-key-123";
        save_key(&dir, key).unwrap();
        assert_eq!(load_key(&dir).unwrap(), Some(key.to_string()));

        // The key file must not contain the plaintext.
        let on_disk = std::fs::read(key_path(&dir)).unwrap();
        assert_ne!(on_disk, key.as_bytes());

        delete_key(&dir);
        assert_eq!(load_key(&dir).unwrap(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auth_session_never_serializes_the_api_key() {
        let session = AuthSession {
            api_key: "secret".to_string(),
            user: NexusUser {
                user_id: 7,
                name: "Modder".to_string(),
                profile_url: None,
                is_premium: true,
                is_supporter: false,
                is_admin: false,
            },
            remembered: true,
        };
        let value = serde_json::to_value(session).unwrap();
        assert!(value.get("api_key").is_none());
        assert_eq!(value["user"]["is_premium"], true);
    }
}
