use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, State};

use crate::auth::{self, AuthState};
use crate::nexus;

const GAME_DOMAIN: &str = "marvelrivals";
const V1_BASE_URL: &str = "https://api.nexusmods.com/v1/games";
const GRAPHQL_URL: &str = "https://api.nexusmods.com/v2/graphql";
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_PAGE_SIZE: u32 = 50;
const CATEGORY_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

type CategoryCache = Option<(std::time::Instant, Vec<ModCategory>)>;
static CATEGORY_CACHE: OnceLock<Mutex<CategoryCache>> = OnceLock::new();

fn category_cache() -> &'static Mutex<CategoryCache> {
    CATEGORY_CACHE.get_or_init(|| Mutex::new(None))
}

const BROWSE_QUERY: &str = r#"
query BrowseMods($filter: ModsFilter, $sort: [ModsSort!], $offset: Int!, $count: Int!) {
  mods(
    filter: $filter
    sort: $sort
    offset: $offset
    count: $count
    viewUploaderHidden: false
    viewUserBlockedContent: false
  ) {
    totalCount
    nodes {
      modId
      name
      summary
      pictureUrl
      downloads
      endorsements
      category
      modCategory { categoryId name }
      author
      updatedAt
    }
  }
}
"#;

#[derive(Debug, Clone, Serialize)]
pub struct ModSummary {
    pub id: u32,
    pub name: String,
    pub summary: String,
    pub picture_url: String,
    pub downloads: u64,
    pub endorsements: u64,
    pub category_id: u32,
    pub category_name: String,
    pub author: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModCategory {
    pub id: u32,
    pub name: String,
    pub parent: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseRequest {
    pub sort: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub category_names: Vec<String>,
    pub offset: u32,
    pub count: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
    pub mods: Vec<ModSummary>,
    pub nodes_count: u32,
    pub total_count: u32,
    pub next_offset: u32,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum WorkshopError {
    #[serde(rename = "not_authenticated")]
    NotAuthenticated(String),
    #[serde(rename = "network")]
    Network(String),
    #[serde(rename = "api")]
    Api(String),
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlData {
    mods: GraphQlMods,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlMods {
    nodes: Vec<GraphQlMod>,
    total_count: u32,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlMod {
    mod_id: u32,
    name: String,
    summary: Option<String>,
    picture_url: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    endorsements: u64,
    category: Option<String>,
    mod_category: Option<GraphQlCategory>,
    author: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphQlCategory {
    category_id: u32,
    #[serde(default)]
    name: String,
}

pub(crate) fn resolve_credential(
    app: &AppHandle,
    state: &State<'_, AuthState>,
) -> Result<String, WorkshopError> {
    auth::resolve_credential(app, state)
        .map_err(|e| WorkshopError::NotAuthenticated(format!("{e:?}")))
}

pub(crate) async fn nexus_get_json(
    credential: &str,
    url: &reqwest::Url,
) -> Result<serde_json::Value, WorkshopError> {
    if let Some(message) = nexus::rate_limit_cooldown() {
        return Err(WorkshopError::Api(message));
    }
    let response = nexus::http_client()
        .get(url.clone())
        .header("apikey", credential)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| WorkshopError::Network(e.to_string()))?;

    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .await
            .map_err(|e| WorkshopError::Api(format!("Invalid response: {e}")));
    }

    let rate_limit =
        (status.as_u16() == 429).then(|| nexus::rate_limit_message(response.headers()));
    let body = response.text().await.unwrap_or_default();
    match status.as_u16() {
        401 | 403 => Err(WorkshopError::NotAuthenticated(format!(
            "HTTP {status}: {body}"
        ))),
        429 => Err(WorkshopError::Api(rate_limit.unwrap_or_else(|| {
            "Nexus Mods rate limit reached. Please try again later.".to_string()
        }))),
        code => Err(WorkshopError::Api(format!(
            "Nexus Mods returned HTTP {code}: {body}"
        ))),
    }
}

fn browse_variables(request: &BrowseRequest) -> serde_json::Value {
    let query = request.query.trim();
    let mut filter = serde_json::json!({
        "op": "AND",
        "gameDomainName": [{ "value": GAME_DOMAIN, "op": "EQUALS" }]
    });

    if !query.is_empty() {
        filter["nameStemmed"] = serde_json::json!([{
            "value": query,
            "op": "MATCHES"
        }]);
    }

    if let Some(category_name) = request
        .category_names
        .iter()
        .map(|name| name.trim())
        .find(|name| !name.is_empty())
    {
        filter["categoryName"] = serde_json::json!([{
            "value": category_name,
            "op": "EQUALS"
        }]);
    }

    let primary_sort = if !query.is_empty() {
        serde_json::json!({ "relevance": { "direction": "DESC" } })
    } else if request.sort == "popular" {
        serde_json::json!({ "downloads": { "direction": "DESC" } })
    } else {
        serde_json::json!({ "createdAt": { "direction": "DESC" } })
    };

    serde_json::json!({
        "filter": filter,
        "sort": [primary_sort],
        "offset": request.offset,
        "count": request.count.clamp(1, MAX_PAGE_SIZE)
    })
}

#[tauri::command]
pub async fn browse_mods(
    app: AppHandle,
    state: State<'_, AuthState>,
    request: BrowseRequest,
) -> Result<BrowsePage, WorkshopError> {
    let credential = resolve_credential(&app, &state)?;
    if let Some(message) = nexus::rate_limit_cooldown() {
        return Err(WorkshopError::Api(message));
    }
    let response = nexus::http_client()
        .post(GRAPHQL_URL)
        .header("apikey", credential)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "query": BROWSE_QUERY,
            "variables": browse_variables(&request)
        }))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| WorkshopError::Network(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let rate_limit =
            (status.as_u16() == 429).then(|| nexus::rate_limit_message(response.headers()));
        let body = response.text().await.unwrap_or_default();
        return match status.as_u16() {
            401 | 403 => Err(WorkshopError::NotAuthenticated(format!(
                "HTTP {status}: {body}"
            ))),
            429 => Err(WorkshopError::Api(rate_limit.unwrap_or_else(|| {
                "Nexus Mods rate limit reached. Please try again later.".to_string()
            }))),
            code => Err(WorkshopError::Api(format!(
                "Nexus Mods returned HTTP {code}: {body}"
            ))),
        };
    }

    let response: GraphQlResponse = response
        .json()
        .await
        .map_err(|e| WorkshopError::Api(format!("Invalid GraphQL response: {e}")))?;
    if !response.errors.is_empty() {
        return Err(WorkshopError::Api(
            response
                .errors
                .into_iter()
                .map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    let data = response
        .data
        .ok_or_else(|| WorkshopError::Api("Nexus Mods returned no data.".to_string()))?;

    let total_count = data.mods.total_count;
    let mods: Vec<ModSummary> = data
        .mods
        .nodes
        .into_iter()
        .map(|item| {
            let category_id = item
                .mod_category
                .as_ref()
                .map(|category| category.category_id)
                .unwrap_or(0);
            let category_name = item
                .mod_category
                .as_ref()
                .map(|category| category.name.clone())
                .filter(|name| !name.is_empty())
                .or(item.category)
                .unwrap_or_default();
            ModSummary {
                id: item.mod_id,
                name: item.name,
                summary: item.summary.unwrap_or_default(),
                picture_url: item.picture_url.unwrap_or_default(),
                downloads: item.downloads,
                endorsements: item.endorsements,
                category_id,
                category_name,
                author: item.author.unwrap_or_default(),
                updated_at: item.updated_at.unwrap_or_default(),
            }
        })
        .collect();
    let nodes_count = mods.len() as u32;
    let next_offset = request.offset.saturating_add(nodes_count);

    Ok(BrowsePage {
        mods,
        nodes_count,
        total_count,
        next_offset,
        has_more: nodes_count > 0 && next_offset < total_count,
    })
}

#[tauri::command]
pub async fn get_mod_categories(
    app: AppHandle,
    state: State<'_, AuthState>,
) -> Result<Vec<ModCategory>, WorkshopError> {
    let credential = resolve_credential(&app, &state)?;
    if let Some((cached_at, categories)) = category_cache()
        .lock()
        .expect("category cache mutex poisoned")
        .as_ref()
    {
        if cached_at.elapsed() < CATEGORY_CACHE_TTL {
            return Ok(categories.clone());
        }
    }
    let url = reqwest::Url::parse(&format!("{V1_BASE_URL}/{GAME_DOMAIN}.json"))
        .map_err(|e| WorkshopError::Api(format!("Invalid URL: {e}")))?;

    let mut categories = extract_categories(&nexus_get_json(&credential, &url).await?);
    categories.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    if !categories.is_empty() {
        *category_cache()
            .lock()
            .expect("category cache mutex poisoned") =
            Some((std::time::Instant::now(), categories.clone()));
    }
    Ok(categories)
}

fn extract_categories(value: &serde_json::Value) -> Vec<ModCategory> {
    let array = value
        .as_array()
        .cloned()
        .or_else(|| value.get("categories").and_then(|v| v.as_array()).cloned())
        .or_else(|| value.get("data").and_then(|v| v.as_array()).cloned())
        .unwrap_or_default();

    let mut categories = Vec::new();
    for item in array {
        let Some(id) = item.get("category_id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let parent = item
            .get("parent_category")
            .and_then(|v| v.as_u64())
            .map(|p| p as u32);
        categories.push(ModCategory {
            id: id as u32,
            name,
            parent,
        });
    }
    categories
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> BrowseRequest {
        BrowseRequest {
            sort: "newest".to_string(),
            query: String::new(),
            category_names: Vec::new(),
            offset: 0,
            count: 24,
        }
    }

    #[test]
    fn builds_remote_search_and_category_filter() {
        let variables = browse_variables(&BrowseRequest {
            sort: "popular".to_string(),
            query: "spider man".to_string(),
            category_names: vec!["Characters".to_string()],
            offset: 48,
            count: 24,
        });
        assert_eq!(variables["offset"], 48);
        assert_eq!(variables["filter"]["nameStemmed"][0]["op"], "MATCHES");
        assert_eq!(
            variables["filter"]["categoryName"][0]["value"],
            "Characters"
        );
        assert_eq!(variables["sort"][0]["relevance"]["direction"], "DESC");
    }

    #[test]
    fn maps_browse_sort_and_clamps_page_size() {
        let mut popular = request();
        popular.sort = "popular".to_string();
        popular.count = 500;
        let variables = browse_variables(&popular);
        assert_eq!(variables["sort"][0]["downloads"]["direction"], "DESC");
        assert_eq!(variables["count"], MAX_PAGE_SIZE);

        let variables = browse_variables(&request());
        assert_eq!(variables["sort"][0]["createdAt"]["direction"], "DESC");
    }

    #[test]
    fn extracts_categories_from_array() {
        let value = serde_json::json!([
            { "category_id": 1, "name": "Skins", "parent_category": false },
            { "category_id": 2, "name": "Models", "parent_category": 1 }
        ]);
        let categories = extract_categories(&value);
        assert_eq!(categories.len(), 2);
        assert_eq!(categories[0].parent, None);
        assert_eq!(categories[1].parent, Some(1));
    }
}
