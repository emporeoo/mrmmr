import { invoke } from "@tauri-apps/api/core"

export interface Preferences {
  auto_delete_mod_archives: boolean
}

export function getPreferences(): Promise<Preferences> {
  return invoke<Preferences>("get_preferences")
}

export function setAutoDeleteModArchives(enabled: boolean): Promise<Preferences> {
  return invoke<Preferences>("set_auto_delete_mod_archives", { enabled })
}

export function resetAllData(): Promise<void> {
  return invoke<void>("reset_all_data")
}
