import { invoke } from "@tauri-apps/api/core"

export interface GameLocation {
  path: string
  source: "steam" | "epic" | "manual"
}

export interface GameProcessStatus {
  game_running: boolean
  shipping_running: boolean
}

export type GameErrorKind =
  | "not_a_game"
  | "platform_not_running"
  | "game_already_running"
  | "game_files_locked"
  | "storage"

export interface GameError {
  kind: GameErrorKind
  message?: string
}

export function detectGame(): Promise<GameLocation | null> {
  return invoke<GameLocation | null>("detect_game")
}

export function getGameLocation(): Promise<GameLocation | null> {
  return invoke<GameLocation | null>("get_game_location")
}

export function saveGameLocation(path: string): Promise<GameLocation> {
  return invoke<GameLocation>("save_game_location", { path })
}

export function ensureModsFolder(): Promise<string> {
  return invoke<string>("ensure_mods_folder")
}

export function openModsFolder(): Promise<string> {
  return invoke<string>("open_mods_folder")
}

export function launchGame(): Promise<string> {
  return invoke<string>("launch_game")
}

export function getGameProcessStatus(): Promise<GameProcessStatus> {
  return invoke<GameProcessStatus>("get_game_process_status")
}

export function closeGame(): Promise<void> {
  return invoke<void>("close_game")
}

export function describeGameError(err: unknown): { title: string; description: string } {
  const e = err as GameError
  switch (e?.kind) {
    case "not_a_game":
      return {
        title: "That doesn't look like Marvel Rivals",
        description: "Pick the folder that contains MarvelRivals_Launcher.exe.",
      }
    case "platform_not_running":
      return {
        title: "Steam or Epic Games isn't running",
        description: "Open Steam or Epic Games Launcher, then try again.",
      }
    case "game_already_running":
      return {
        title: "Marvel Rivals is already running",
        description: "Close the game and its launcher before starting another instance.",
      }
    case "game_files_locked":
      return {
        title: "Marvel Rivals is running",
        description: "Close the game before changing its mod or bypass files.",
      }
    case "storage":
      return {
        title: "Failed",
        description: e.message ?? "Couldn't access the game location on this device.",
      }
    default:
      return {
        title: "Failed",
        description: "Something went wrong. Please try again.",
      }
  }
}
