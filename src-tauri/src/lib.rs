mod asset_conflicts;
mod auth;
mod doctor;
mod game;
mod install;
mod nexus;
mod preferences;
mod secure;
mod storage;
mod utoc;
mod workshop;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(auth::AuthState::default())
        .manage(asset_conflicts::AssetConflictState::default())
        .manage(install::InstallState::default())
        .setup(|app| {
            install::recover_pending(app.handle()).map_err(|error| -> Box<dyn std::error::Error> {
                format!("Could not recover an interrupted mod change: {error:?}").into()
            })
        })
        .invoke_handler(tauri::generate_handler![
            auth::get_auth_session,
            auth::complete_sso,
            auth::refresh_auth_session,
            auth::clear_auth,
            auth::reset_all_data,
            doctor::run_mod_doctor,
            doctor::repair_mod_doctor,
            doctor::export_diagnostics,
            preferences::get_preferences,
            preferences::set_auto_delete_mod_archives,
            game::detect_game,
            game::get_game_location,
            game::save_game_location,
            game::ensure_mods_folder,
            game::open_mods_folder,
            game::launch_game,
            game::get_game_process_status,
            game::close_game,
            utoc::utoc_status,
            utoc::install_utoc,
            utoc::utoc_files_url,
            utoc::utoc_detect_download,
            utoc::utoc_install_from_archive,
            workshop::browse_mods,
            workshop::get_mod_categories,
            install::get_mod_install_options,
            install::prepare_mod_install,
            install::prepare_mod_install_from_archive,
            install::commit_mod_install,
            install::discard_mod_install,
            install::get_last_install_change,
            install::undo_last_install_change,
            install::mod_files_url,
            install::detect_mod_download,
            install::get_installed_mods,
            install::get_installed_stats,
            install::uninstall_mod,
            install::set_mod_enabled,
            install::check_mod_updates,
            asset_conflicts::get_asset_conflict_summary,
            asset_conflicts::get_asset_conflicts
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
