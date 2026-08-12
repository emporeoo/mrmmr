use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

const NEXUS_BASE_URL: &str = "https://api.nexusmods.com";
const VALIDATE_PATH: &str = "/v1/users/validate.json";
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static RATE_LIMIT_UNTIL: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();

pub(crate) fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::HeaderName::from_static("protocol-version"),
            reqwest::header::HeaderValue::from_static("1"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("application-name"),
            reqwest::header::HeaderValue::from_static("MRMMR"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("application-version"),
            reqwest::header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );

        reqwest::Client::builder()
            .default_headers(headers)
            .user_agent(format!("MRMMR/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::limited(10))
            .https_only(true)
            .build()
            .expect("static Nexus HTTP client configuration must be valid")
    })
}

pub(crate) fn rate_limit_message(headers: &reqwest::header::HeaderMap) -> String {
    let retry_after = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty());
    let seconds = retry_after
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(1, 60 * 60);
    *RATE_LIMIT_UNTIL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("rate-limit mutex poisoned") =
        Some(std::time::Instant::now() + std::time::Duration::from_secs(seconds));
    match retry_after {
        Some(value) => format!("Nexus Mods rate limit reached. Try again after {value} seconds."),
        None => "Nexus Mods rate limit reached. Please try again later.".to_string(),
    }
}

/// Avoid repeatedly spending requests after Nexus has told this process to
/// back off. A later successful request is not needed to clear the deadline.
pub(crate) fn rate_limit_cooldown() -> Option<String> {
    let deadline = *RATE_LIMIT_UNTIL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("rate-limit mutex poisoned");
    let remaining = deadline?.checked_duration_since(std::time::Instant::now())?;
    Some(format!(
        "Nexus Mods rate limit reached. Try again in {} seconds.",
        remaining.as_secs().max(1)
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexusUser {
    pub user_id: u32,
    pub name: String,
    #[serde(default)]
    pub profile_url: Option<String>,
    #[serde(default)]
    pub is_premium: bool,
    #[serde(default)]
    pub is_supporter: bool,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AuthError {
    #[serde(rename = "empty_api_key")]
    EmptyApiKey,
    #[serde(rename = "invalid_api_key")]
    InvalidApiKey,
    #[serde(rename = "network")]
    Network(String),
    #[serde(rename = "storage")]
    Storage(String),
    #[serde(rename = "game_files_locked")]
    GameFilesLocked,
}

pub async fn validate_api_key(api_key: &str) -> Result<NexusUser, AuthError> {
    if api_key.trim().is_empty() {
        return Err(AuthError::EmptyApiKey);
    }
    if let Some(message) = rate_limit_cooldown() {
        return Err(AuthError::Network(message));
    }

    let response = http_client()
        .get(format!("{NEXUS_BASE_URL}{VALIDATE_PATH}"))
        .header("apikey", api_key.trim())
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| AuthError::Network(e.to_string()))?;

    match response.status().as_u16() {
        200 => response
            .json::<NexusUser>()
            .await
            .map_err(|e| AuthError::Network(format!("Unexpected response from Nexus: {e}"))),
        401 | 403 => Err(AuthError::InvalidApiKey),
        429 => Err(AuthError::Network(rate_limit_message(response.headers()))),
        status => Err(AuthError::Network(format!("Nexus returned HTTP {status}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_response_keeps_authoritative_profile_url() {
        let user: NexusUser = serde_json::from_value(serde_json::json!({
            "user_id": 7,
            "key": "redacted",
            "name": "Modder",
            "profile_url": "https://example.nexusmods.com/avatar.png",
            "is_premium": true,
            "is_supporter": true
        }))
        .unwrap();

        assert_eq!(
            user.profile_url.as_deref(),
            Some("https://example.nexusmods.com/avatar.png")
        );
        assert!(user.is_premium);
        assert!(user.is_supporter);
    }

    #[test]
    fn stored_users_without_profile_url_still_deserialize() {
        let user: NexusUser = serde_json::from_value(serde_json::json!({
            "user_id": 7,
            "key": "redacted",
            "name": "Modder"
        }))
        .unwrap();

        assert_eq!(user.profile_url, None);
        assert!(!user.is_premium);
        assert!(!user.is_supporter);

        let serialized = serde_json::to_value(user).unwrap();
        assert!(serialized.get("key").is_none());
    }
}
