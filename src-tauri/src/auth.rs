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
const CREDENTIAL_FILE: &str = "auth.token";
const LEGACY_CREDENTIAL_FILE: &str = "auth.key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    #[serde(skip_serializing)]
    pub credential: String,
    pub user: NexusUser,
}

/// Non-sensitive account data stored alongside the encrypted SSO credential.
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

fn credential_path(dir: &Path) -> PathBuf {
    dir.join(CREDENTIAL_FILE)
}

fn legacy_credential_path(dir: &Path) -> PathBuf {
    dir.join(LEGACY_CREDENTIAL_FILE)
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
    serde_json::from_str(&contents)
        .map(Some)
        .map_err(|e| AuthError::Storage(format!("Could not parse auth file: {e}")))
}

fn save_profile(dir: &Path, profile: &StoredProfile) -> Result<(), AuthError> {
    storage::write_json_atomic(&profile_path(dir), profile)
        .map_err(|e| AuthError::Storage(format!("Could not write auth file: {e}")))
}

fn save_credential(dir: &Path, credential: &str) -> Result<(), AuthError> {
    let ciphertext = secure::encrypt(credential)?;
    storage::write_bytes_atomic(&credential_path(dir), &ciphertext)
        .map_err(|e| AuthError::Storage(format!("Could not write authorization file: {e}")))
}

fn load_credential(dir: &Path) -> Result<Option<String>, AuthError> {
    let ciphertext = match std::fs::read(credential_path(dir)) {
        Ok(ciphertext) => ciphertext,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(AuthError::Storage(format!(
                "Could not read authorization file: {e}"
            )))
        }
    };
    secure::decrypt(&ciphertext).map(Some)
}

fn delete_file(path: PathBuf) {
    let _ = std::fs::remove_file(path);
}

fn remove_persisted(app: &AppHandle) {
    if let Ok(dir) = config_dir(app) {
        delete_file(credential_path(&dir));
        delete_file(legacy_credential_path(&dir));
        delete_file(profile_path(&dir));
    }
}

fn persist_session(app: &AppHandle, session: &AuthSession) -> Result<(), AuthError> {
    let dir = config_dir(app)?;
    ensure_dir(&dir)?;
    save_credential(&dir, &session.credential)?;
    delete_file(legacy_credential_path(&dir));
    save_profile(
        &dir,
        &StoredProfile {
            user: session.user.clone(),
        },
    )
}

fn set_session(state: &State<'_, AuthState>, session: Option<AuthSession>) {
    *state.0.lock().expect("auth state mutex poisoned") = session;
}

pub fn current_credential(state: &State<'_, AuthState>) -> Option<String> {
    state
        .0
        .lock()
        .expect("auth state mutex poisoned")
        .as_ref()
        .map(|session| session.credential.clone())
}

/// Resolve the application-scoped SSO credential from memory or encrypted storage.
pub fn resolve_credential(
    app: &AppHandle,
    state: &State<'_, AuthState>,
) -> Result<String, AuthError> {
    if let Some(credential) = current_credential(state) {
        return Ok(credential);
    }
    let dir = config_dir(app)?;
    load_credential(&dir)?.ok_or(AuthError::MissingCredential)
}

fn session_from_storage(
    app: &AppHandle,
    state: &State<'_, AuthState>,
) -> Result<Option<AuthSession>, AuthError> {
    if let Some(session) = state.0.lock().expect("auth state mutex poisoned").clone() {
        return Ok(Some(session));
    }

    let dir = config_dir(app)?;
    // Never carry the credential file used by pre-SSO releases into this flow.
    delete_file(legacy_credential_path(&dir));
    let profile = match load_profile(&dir)? {
        Some(profile) => profile,
        None => return Ok(None),
    };

    let credential = match load_credential(&dir)? {
        Some(credential) => credential,
        None => {
            // Versions before SSO stored a manually supplied credential. Public
            // builds must not reuse it, so remove that legacy state.
            delete_file(legacy_credential_path(&dir));
            delete_file(profile_path(&dir));
            return Ok(None);
        }
    };

    let session = AuthSession {
        credential,
        user: profile.user,
    };
    set_session(state, Some(session.clone()));
    Ok(Some(session))
}

/// Complete the Nexus SSO flow after the official service returns the
/// application-scoped credential through its WebSocket connection.
#[tauri::command]
pub async fn complete_sso(
    app: AppHandle,
    state: State<'_, AuthState>,
    credential: String,
) -> Result<AuthSession, AuthError> {
    let credential = credential.trim();
    if credential.is_empty() {
        return Err(AuthError::MissingCredential);
    }

    let user = nexus::validate_credential(credential).await?;
    let session = AuthSession {
        credential: credential.to_string(),
        user,
    };
    persist_session(&app, &session)?;
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

    let user = match nexus::validate_credential(&session.credential).await {
        Ok(user) => user,
        Err(AuthError::InvalidCredential) => {
            remove_persisted(&app);
            set_session(&state, None);
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    let updated = AuthSession { user, ..session };
    persist_session(&app, &updated)?;
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
/// game location, and remove the saved Nexus authorization.
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
        let credential = "application-scoped-sso-credential";
        let ciphertext = secure::encrypt(credential).unwrap();
        assert_ne!(ciphertext, credential.as_bytes());
        assert_eq!(secure::decrypt(&ciphertext).unwrap(), credential);
    }

    #[test]
    fn encrypted_credential_file_round_trip() {
        let dir = test_dir();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let credential = "application-scoped-sso-credential";
        save_credential(&dir, credential).unwrap();
        assert_eq!(load_credential(&dir).unwrap(), Some(credential.to_string()));

        let on_disk = std::fs::read(credential_path(&dir)).unwrap();
        assert_ne!(on_disk, credential.as_bytes());

        delete_file(credential_path(&dir));
        assert_eq!(load_credential(&dir).unwrap(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auth_session_never_serializes_the_credential() {
        let session = AuthSession {
            credential: "secret".to_string(),
            user: NexusUser {
                user_id: 7,
                name: "Modder".to_string(),
                profile_url: None,
                is_premium: true,
                is_supporter: false,
                is_admin: false,
            },
        };
        let value = serde_json::to_value(session).unwrap();
        assert!(value.get("credential").is_none());
        assert_eq!(value["user"]["is_premium"], true);
    }
}
