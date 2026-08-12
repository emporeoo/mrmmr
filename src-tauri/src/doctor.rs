use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::{game, install, preferences, storage, utoc};

const INSTALLED_FILE: &str = "installed.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorRepair {
    QuarantineOrphan,
    RemoveStaleMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorFinding {
    pub id: String,
    pub severity: DoctorSeverity,
    pub title: String,
    pub description: String,
    pub path: Option<String>,
    pub mod_id: Option<u32>,
    pub mod_name: Option<String>,
    pub repair: Option<DoctorRepair>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DoctorSummary {
    pub critical: usize,
    pub warning: usize,
    pub info: usize,
    pub repairable: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub scanned_at: u64,
    pub game_running: bool,
    pub summary: DoctorSummary,
    pub findings: Vec<DoctorFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorRepairResult {
    pub repaired: usize,
    pub quarantined: usize,
    pub metadata_removed: usize,
    pub report: DoctorReport,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum DoctorError {
    #[serde(rename = "game_files_locked")]
    GameFilesLocked,
    #[serde(rename = "storage")]
    Storage(String),
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, DoctorError> {
    app.path().app_config_dir().map_err(|error| {
        DoctorError::Storage(format!("Could not resolve config directory: {error}"))
    })
}

fn installed_inventory(app: &AppHandle) -> Result<Vec<install::InstalledMod>, DoctorError> {
    let path = config_dir(app)?.join(INSTALLED_FILE);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
            DoctorError::Storage(format!(
                "Installed-mod metadata is damaged at '{}': {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(DoctorError::Storage(format!(
            "Could not read installed-mod metadata: {error}"
        ))),
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

fn disabled_path(path: &Path) -> PathBuf {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) if !name.ends_with(".disabled") => {
            path.with_file_name(format!("{name}.disabled"))
        }
        _ => path.to_path_buf(),
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn container_extension(path: &Path) -> Option<String> {
    let candidate = if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("disabled"))
    {
        path.file_stem().map(PathBuf::from)?
    } else {
        path.to_path_buf()
    };
    let extension = candidate.extension()?.to_str()?.to_ascii_lowercase();
    ["pak", "utoc", "ucas", "sig"]
        .contains(&extension.as_str())
        .then_some(extension)
}

fn finding(
    id: String,
    severity: DoctorSeverity,
    title: impl Into<String>,
    description: impl Into<String>,
) -> DoctorFinding {
    DoctorFinding {
        id,
        severity,
        title: title.into(),
        description: description.into(),
        path: None,
        mod_id: None,
        mod_name: None,
        repair: None,
    }
}

fn scan(app: &AppHandle) -> Result<DoctorReport, DoctorError> {
    let process =
        game::process_status().map_err(|error| DoctorError::Storage(error.to_string()))?;
    let mut findings = Vec::new();
    if process.shipping_running {
        findings.push(finding(
            "game-running".into(),
            DoctorSeverity::Warning,
            "Marvel Rivals is running",
            "Scanning is read-only, but repairs are locked until the game closes.",
        ));
    }

    let Some(location) =
        game::load_location(app).map_err(|error| DoctorError::Storage(error.to_string()))?
    else {
        findings.push(finding(
            "game-location".into(),
            DoctorSeverity::Critical,
            "Game installation is not configured",
            "Locate Marvel Rivals in Settings before managing or repairing mods.",
        ));
        return Ok(finalize(findings, process.shipping_running));
    };

    let bypass = utoc::check_installed(&location.path);
    if !bypass.installed {
        findings.push(finding(
            "utoc-missing".into(),
            DoctorSeverity::Critical,
            "UTOC Signature Bypass is incomplete",
            format!("Missing: {}.", bypass.missing.join(", ")),
        ));
    }

    let inventory = match installed_inventory(app) {
        Ok(inventory) => inventory,
        Err(DoctorError::Storage(message)) => {
            findings.push(finding(
                "metadata-corrupt".into(),
                DoctorSeverity::Critical,
                "Installed-mod metadata is damaged",
                message,
            ));
            return Ok(finalize(findings, process.shipping_running));
        }
        Err(error) => return Err(error),
    };

    let target = mods_dir(&location.path);
    let mut owners: HashMap<String, Vec<(u32, String)>> = HashMap::new();
    let mut tracked = HashSet::new();
    for item in &inventory {
        if item.files.is_empty() {
            let mut issue = finding(
                format!("empty-inventory-{}", item.mod_id),
                DoctorSeverity::Warning,
                "Installed mod has no tracked files",
                format!("{} has an empty file inventory.", item.name),
            );
            issue.mod_id = Some(item.mod_id);
            issue.mod_name = Some(item.name.clone());
            issue.repair = Some(DoctorRepair::RemoveStaleMetadata);
            findings.push(issue);
            continue;
        }

        let mut active_count = 0;
        let mut disabled_count = 0;
        let mut missing_count = 0;
        for file in &item.files {
            let active = PathBuf::from(file);
            let disabled = disabled_path(&active);
            let active_exists = active.is_file();
            let disabled_exists = disabled.is_file();
            tracked.insert(path_key(&active));
            owners
                .entry(path_key(&active))
                .or_default()
                .push((item.mod_id, item.name.clone()));
            match (active_exists, disabled_exists) {
                (true, true) => {
                    let mut issue = finding(
                        format!("duplicate-state-{}-{}", item.mod_id, path_key(&active)),
                        DoctorSeverity::Critical,
                        "Invalid disabled-file state",
                        "Both the active and .disabled copy exist. Choose which copy to keep.",
                    );
                    issue.path = Some(active.to_string_lossy().into_owned());
                    issue.mod_id = Some(item.mod_id);
                    issue.mod_name = Some(item.name.clone());
                    findings.push(issue);
                }
                (true, false) => active_count += 1,
                (false, true) => disabled_count += 1,
                (false, false) => {
                    missing_count += 1;
                    let mut issue = finding(
                        format!("missing-{}-{}", item.mod_id, path_key(&active)),
                        DoctorSeverity::Warning,
                        "Tracked mod file is missing",
                        format!(
                            "{} is missing from the ~mods folder.",
                            active.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    );
                    issue.path = Some(active.to_string_lossy().into_owned());
                    issue.mod_id = Some(item.mod_id);
                    issue.mod_name = Some(item.name.clone());
                    findings.push(issue);
                }
            }
        }
        if active_count > 0 && disabled_count > 0 {
            let mut issue = finding(
                format!("mixed-state-{}", item.mod_id),
                DoctorSeverity::Warning,
                "Mod is only partially enabled",
                "Some tracked files are active while others use the .disabled suffix.",
            );
            issue.mod_id = Some(item.mod_id);
            issue.mod_name = Some(item.name.clone());
            findings.push(issue);
        }
        if missing_count == item.files.len() {
            let mut issue = finding(
                format!("stale-metadata-{}", item.mod_id),
                DoctorSeverity::Warning,
                "Installed entry has no remaining files",
                format!(
                    "{} can be removed from MRMMR's installed inventory.",
                    item.name
                ),
            );
            issue.mod_id = Some(item.mod_id);
            issue.mod_name = Some(item.name.clone());
            issue.repair = Some(DoctorRepair::RemoveStaleMetadata);
            findings.push(issue);
        }
    }

    for (path, entries) in owners {
        let unique: HashSet<u32> = entries.iter().map(|entry| entry.0).collect();
        if unique.len() > 1 {
            let names = entries
                .iter()
                .map(|entry| entry.1.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let mut issue = finding(
                format!("duplicate-owner-{path}"),
                DoctorSeverity::Critical,
                "Multiple mods claim the same file",
                format!("Tracked by: {names}."),
            );
            issue.path = Some(path);
            findings.push(issue);
        }
    }

    if target.is_dir() {
        let mut companions: HashMap<String, HashSet<String>> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&target) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file()
                    || entry
                        .file_type()
                        .map(|kind| kind.is_symlink())
                        .unwrap_or(true)
                {
                    continue;
                }
                let Some(extension) = container_extension(&path) else {
                    continue;
                };
                let active = if path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("disabled"))
                {
                    PathBuf::from(path.to_string_lossy().trim_end_matches(".disabled"))
                } else {
                    path.clone()
                };
                companions
                    .entry(
                        active
                            .with_extension("")
                            .to_string_lossy()
                            .to_ascii_lowercase(),
                    )
                    .or_default()
                    .insert(extension);
                if !tracked.contains(&path_key(&active)) {
                    let mut issue = finding(
                        format!("orphan-{}", path_key(&path)),
                        DoctorSeverity::Warning,
                        "Untracked mod container",
                        format!(
                            "{} is not owned by any installed mod.",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    );
                    issue.path = Some(path.to_string_lossy().into_owned());
                    issue.repair = Some(DoctorRepair::QuarantineOrphan);
                    findings.push(issue);
                }
            }
        }
        for (stem, extensions) in companions {
            if extensions.contains("utoc") != extensions.contains("ucas") {
                let missing = if extensions.contains("utoc") {
                    ".ucas"
                } else {
                    ".utoc"
                };
                let mut issue = finding(
                    format!("companion-{stem}"),
                    DoctorSeverity::Critical,
                    "Container companion file is missing",
                    format!("This asset container is missing its required {missing} companion."),
                );
                issue.path = Some(stem);
                findings.push(issue);
            }
        }
    }

    if findings.is_empty() {
        findings.push(finding(
            "healthy".into(),
            DoctorSeverity::Info,
            "No problems found",
            "Installed metadata, mod containers, disabled states, and the UTOC bypass look healthy.",
        ));
    }
    Ok(finalize(findings, process.shipping_running))
}

fn finalize(findings: Vec<DoctorFinding>, game_running: bool) -> DoctorReport {
    let mut summary = DoctorSummary::default();
    for finding in &findings {
        match finding.severity {
            DoctorSeverity::Critical => summary.critical += 1,
            DoctorSeverity::Warning => summary.warning += 1,
            DoctorSeverity::Info => summary.info += 1,
        }
        if finding.repair.is_some() {
            summary.repairable += 1;
        }
    }
    DoctorReport {
        scanned_at: now(),
        game_running,
        summary,
        findings,
    }
}

#[tauri::command]
pub fn run_mod_doctor(app: AppHandle) -> Result<DoctorReport, DoctorError> {
    scan(&app)
}

#[tauri::command]
pub fn repair_mod_doctor(app: AppHandle) -> Result<DoctorRepairResult, DoctorError> {
    game::ensure_game_files_mutable().map_err(|error| match error {
        game::GameError::GameFilesLocked => DoctorError::GameFilesLocked,
        other => DoctorError::Storage(other.to_string()),
    })?;
    let report = scan(&app)?;
    let config = config_dir(&app)?;
    let quarantine = config.join("doctor-quarantine").join(now().to_string());
    let mut quarantined = 0;
    let mut stale_ids = HashSet::new();
    for finding in &report.findings {
        match finding.repair {
            Some(DoctorRepair::QuarantineOrphan) => {
                let Some(path) = finding.path.as_deref().map(PathBuf::from) else {
                    continue;
                };
                let Some(name) = path.file_name() else {
                    continue;
                };
                std::fs::create_dir_all(&quarantine).map_err(|error| {
                    DoctorError::Storage(format!("Could not create quarantine folder: {error}"))
                })?;
                std::fs::rename(&path, quarantine.join(name)).map_err(|error| {
                    DoctorError::Storage(format!(
                        "Could not quarantine '{}': {error}",
                        path.display()
                    ))
                })?;
                quarantined += 1;
            }
            Some(DoctorRepair::RemoveStaleMetadata) => {
                if let Some(mod_id) = finding.mod_id {
                    stale_ids.insert(mod_id);
                }
            }
            None => {}
        }
    }
    let mut metadata_removed = 0;
    if !stale_ids.is_empty() {
        let mut inventory = installed_inventory(&app)?;
        let before = inventory.len();
        inventory.retain(|item| !stale_ids.contains(&item.mod_id));
        metadata_removed = before - inventory.len();
        storage::write_json_atomic(&config.join(INSTALLED_FILE), &inventory)
            .map_err(DoctorError::Storage)?;
    }
    let repaired = quarantined + metadata_removed;
    Ok(DoctorRepairResult {
        repaired,
        quarantined,
        metadata_removed,
        report: scan(&app)?,
    })
}

#[derive(Serialize)]
struct DiagnosticGame {
    configured: bool,
    source: Option<String>,
    process_running: bool,
}

#[derive(Serialize)]
struct DiagnosticMod {
    mod_id: u32,
    name: String,
    version: String,
    enabled: bool,
    missing: bool,
    nexus_file_id: Option<u32>,
    files: Vec<String>,
}

#[derive(Serialize)]
struct DiagnosticBundle {
    schema_version: u32,
    app_version: &'static str,
    generated_at: u64,
    game: DiagnosticGame,
    utoc: Option<utoc::UtocStatus>,
    preferences: Option<preferences::Preferences>,
    installed: Vec<DiagnosticMod>,
    doctor: DoctorReport,
}

#[tauri::command]
pub fn export_diagnostics(app: AppHandle, destination: String) -> Result<String, DoctorError> {
    let location =
        game::load_location(&app).map_err(|error| DoctorError::Storage(error.to_string()))?;
    let process =
        game::process_status().map_err(|error| DoctorError::Storage(error.to_string()))?;
    let installed = install::installed_snapshot(&app)
        .unwrap_or_default()
        .into_iter()
        .map(|item| DiagnosticMod {
            mod_id: item.mod_id,
            name: item.name,
            version: item.version,
            enabled: item.enabled,
            missing: item.missing,
            nexus_file_id: item.nexus_file_id,
            files: item
                .files
                .iter()
                .map(|path| {
                    Path::new(path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect(),
        })
        .collect();
    let mut doctor = scan(&app)?;
    for finding in &mut doctor.findings {
        finding.path = finding.path.as_deref().map(|path| {
            Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    }
    let bundle = DiagnosticBundle {
        schema_version: 1,
        app_version: env!("CARGO_PKG_VERSION"),
        generated_at: now(),
        game: DiagnosticGame {
            configured: location.is_some(),
            source: location.as_ref().map(|location| location.source.clone()),
            process_running: process.shipping_running,
        },
        utoc: location
            .as_ref()
            .map(|location| utoc::check_installed(&location.path)),
        preferences: preferences::load(&app).ok(),
        installed,
        doctor,
    };
    let path = PathBuf::from(destination);
    storage::write_json_atomic(&path, &bundle).map_err(DoctorError::Storage)?;
    Ok(path.to_string_lossy().into_owned())
}
