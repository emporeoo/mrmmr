import { invoke } from "@tauri-apps/api/core"

import type { PreviewAssetConflictReport } from "@/lib/conflicts"

export interface InstalledMod {
  mod_id: number
  name: string
  version: string
  files: string[]
  installed_at: number
  nexus_file_id?: number | null
  archive_name?: string | null
  archive_md5?: string | null
  parts: InstalledPart[]
  picture_url?: string | null
  enabled: boolean
  missing: boolean
}

export interface InstalledPart {
  nexus_file_id?: number | null
  archive_name: string
  archive_md5: string
}

export interface InstalledStats {
  mod_count: number
  enabled_count: number
  disabled_count: number
  missing_count: number
  total_size_bytes: number
}

export type InstallPreviewAction = "add" | "replace" | "remove" | "blocked"

export interface InstallPreviewFile {
  name: string
  size_bytes: number
  action: InstallPreviewAction
  owner_mod_id?: number | null
  owner_name?: string | null
}

export interface InstallPreview {
  plan_id: string
  mod_id: number
  mod_name: string
  version: string
  archive_name: string
  archive_md5: string
  archive_verified: boolean
  required_bytes: number
  available_bytes?: number | null
  enough_space: boolean
  adds: number
  replaces: number
  removes: number
  blocked_files: number
  asset_conflicts: PreviewAssetConflictReport
  can_install: boolean
  archives: InstallPreviewArchive[]
  files: InstallPreviewFile[]
}

export interface InstallPreviewArchive {
  file_id?: number | null
  file_name: string
  md5: string
  verified: boolean
}

export interface ModInstallFileOption {
  file_id: number
  display_name: string
  file_name: string
  version?: string | null
  category_name?: string | null
  size_bytes?: number | null
  part_number?: number | null
  contents: ModInstallContentFile[]
}

export interface ModInstallContentFile {
  path: string
  file_name: string
  size_bytes?: number | null
  action: InstallPreviewAction
  owner_mod_id?: number | null
  owner_name?: string | null
}

export interface ModInstallOption {
  id: string
  label: string
  multipart: boolean
  recommended: boolean
  total_size_bytes?: number | null
  content_preview_available: boolean
  predicted_adds: number
  predicted_replaces: number
  predicted_removes: number
  predicted_blocked_files: number
  predicted_removed_files: string[]
  files: ModInstallFileOption[]
}

export interface UndoStatus {
  available: boolean
  label?: string | null
  created_at?: number | null
}

export function getModInstallOptions(modId: number): Promise<ModInstallOption[]> {
  return invoke<ModInstallOption[]>("get_mod_install_options", { modId })
}

export function prepareModInstall(
  modId: number,
  fileIds: number[],
): Promise<InstallPreview> {
  return invoke<InstallPreview>("prepare_mod_install", { modId, fileIds })
}

export function prepareModInstallFromArchive(
  modId: number,
  archivePaths: string[],
  fileIds: number[],
): Promise<InstallPreview> {
  return invoke<InstallPreview>("prepare_mod_install_from_archive", {
    modId,
    archivePaths,
    fileIds,
  })
}

export function commitModInstall(planId: string): Promise<InstalledMod> {
  return invoke<InstalledMod>("commit_mod_install", { planId })
}

export function discardModInstall(planId: string): Promise<void> {
  return invoke<void>("discard_mod_install", { planId })
}

export function getLastInstallChange(): Promise<UndoStatus> {
  return invoke<UndoStatus>("get_last_install_change")
}

export function undoLastInstallChange(): Promise<InstalledMod[]> {
  return invoke<InstalledMod[]>("undo_last_install_change")
}

export function uninstallMod(modId: number): Promise<void> {
  return invoke<void>("uninstall_mod", { modId })
}

export function setModEnabled(modId: number, enabled: boolean): Promise<InstalledMod> {
  return invoke<InstalledMod>("set_mod_enabled", { modId, enabled })
}

export interface ModUpdate {
  mod_id: number
  name: string
  installed_version: string
  latest_version: string
  has_update: boolean
  picture_url?: string | null
}

export function checkModUpdates(): Promise<ModUpdate[]> {
  return invoke<ModUpdate[]>("check_mod_updates")
}

export function getModFilesUrl(modId: number): Promise<string> {
  return invoke<string>("mod_files_url", { modId })
}

export function detectModDownload(modId: number, fileIds: number[]): Promise<string | null> {
  return invoke<string | null>("detect_mod_download", { modId, fileIds })
}

export function getInstalledMods(): Promise<InstalledMod[]> {
  return invoke<InstalledMod[]>("get_installed_mods")
}

export function getInstalledStats(): Promise<InstalledStats> {
  return invoke<InstalledStats>("get_installed_stats")
}

export type InstallErrorKind =
  | "setup_required"
  | "not_authenticated"
  | "game_not_found"
  | "network"
  | "api"
  | "storage"
  | "no_files"
  | "no_download_link"
  | "download"
  | "extract"
  | "no_paks"
  | "install"
  | "archive_mismatch"
  | "game_files_locked"

export function describeInstallError(err: unknown): { title: string; description: string } {
  const e = err as { kind?: InstallErrorKind; message?: string }
  switch (e?.kind) {
    case "setup_required":
      return {
        title: "Setup required",
        description: e.message ?? "Finish the setup in Settings first.",
      }
    case "game_not_found":
      return {
        title: "Marvel Rivals couldn't be found",
        description: "Locate the game in Settings first.",
      }
    case "no_paks":
      return {
        title: "Install failed",
        description: "The mod archive didn't contain any .pak files.",
      }
    case "no_files":
      return {
        title: "Failed",
        description: "No files are available for this mod.",
      }
    case "no_download_link":
      return {
        title: "Failed",
        description: "Couldn't get a download link from Nexus Mods.",
      }
    case "download":
      return {
        title: "Download failed",
        description: e.message ?? "Couldn't download the mod.",
      }
    case "extract":
      return {
        title: "Extraction failed",
        description: e.message ?? "The archive couldn't be extracted.",
      }
    case "install":
      return {
        title: "Install failed",
        description: e.message ?? "The files couldn't be copied.",
      }
    case "archive_mismatch":
      return {
        title: "Wrong mod archive",
        description:
          e.message ?? "That downloaded file does not belong to the selected Nexus mod.",
      }
    case "game_files_locked":
      return {
        title: "Marvel Rivals is running",
        description: "Close the game before installing, toggling, or removing mod files.",
      }
    case "storage":
      return {
        title: "Failed",
        description: e.message ?? "Couldn't save the mod's metadata.",
      }
    case "network":
      return {
        title: "Couldn't reach Nexus Mods",
        description: "Check your connection and try again.",
      }
    case "api":
      return {
        title: "Nexus Mods error",
        description: e.message ?? "Something went wrong.",
      }
    case "not_authenticated":
      return {
        title: "Not signed in",
        description: e.message ?? "Sign in with your Nexus Mods account first.",
      }
    default:
      return {
        title: "Failed",
        description: "Something went wrong. Please try again.",
      }
  }
}
