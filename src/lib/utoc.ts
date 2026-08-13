import { invoke } from "@tauri-apps/api/core"

export interface UtocStatus {
  installed: boolean
  win64_dir: string
  missing: string[]
}

export type UtocErrorKind =
  | "not_authenticated"
  | "game_not_found"
  | "network"
  | "api"
  | "storage"
  | "no_files"
  | "no_download_link"
  | "download"
  | "extract"
  | "missing_files"
  | "install"
  | "game_files_locked"

export interface UtocError {
  kind: UtocErrorKind
  message?: string
}

export function getUtocStatus(): Promise<UtocStatus> {
  return invoke<UtocStatus>("utoc_status")
}

export function installUtoc(): Promise<UtocStatus> {
  return invoke<UtocStatus>("install_utoc")
}

export function getUtocFilesUrl(): Promise<string> {
  return invoke<string>("utoc_files_url")
}

export function detectUtocDownload(): Promise<string | null> {
  return invoke<string | null>("utoc_detect_download")
}

export function installUtocFromArchive(archivePath: string): Promise<UtocStatus> {
  return invoke<UtocStatus>("utoc_install_from_archive", { archivePath })
}

const PROGRESS_LABELS: Record<string, string> = {
  verifying_archive: "Verifying Nexus download...",
  fetching_mod: "Fetching mod info…",
  fetching_files: "Getting available files…",
  fetching_download_link: "Requesting download link…",
  downloading: "Downloading…",
  downloading_for_preview: "Downloading for preview...",
  opening_download_page: "Opening the Nexus download page…",
  waiting_for_download: "Waiting for the download…",
  extracting: "Extracting…",
  locating_paks: "Locating .pak files…",
  scanning_assets: "Checking internal asset conflicts…",
  copying: "Copying files…",
  installing: "Installing…",
  saving: "Saving…",
  verifying: "Verifying…",
  done: "Done",
}

export function progressLabel(step: string): string {
  return PROGRESS_LABELS[step] ?? "Working…"
}

export function describeUtocError(err: unknown): { title: string; description: string } {
  const e = err as UtocError
  switch (e?.kind) {
    case "not_authenticated":
      return {
        title: "Not signed in",
        description: e.message ?? "Sign in with your Nexus Mods account first.",
      }
    case "game_not_found":
      return {
        title: "Marvel Rivals couldn't be found",
        description: "Locate the game on the Settings page first.",
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
        description: e.message ?? "The downloaded file couldn't be extracted.",
      }
    case "missing_files":
      return {
        title: "Install failed",
        description: "The download didn't contain the required files.",
      }
    case "install":
      return {
        title: "Install failed",
        description: e.message ?? "The files couldn't be copied.",
      }
    case "game_files_locked":
      return {
        title: "Marvel Rivals is running",
        description: "Close the game before installing the UTOC Signature Bypass.",
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
    default:
      return {
        title: "Failed",
        description: "Something went wrong. Please try again.",
      }
  }
}
