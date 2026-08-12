import { invoke } from "@tauri-apps/api/core"

export type AssetScanStatus = "complete" | "partial" | "failed" | "pending"

export interface AssetConflictSummary {
  conflicting_asset_count: number
  affected_mod_count: number
  scan_incomplete_mod_count: number
}

export interface AssetConflictPeer {
  mod_id: number
  mod_name: string
  enabled: boolean
}

export interface AssetConflictDetail {
  asset_path: string
  other_mods: AssetConflictPeer[]
}

export interface PreviewAssetConflictReport {
  scan_status: AssetScanStatus
  scan_error?: string | null
  scanned_asset_count: number
  conflicting_asset_count: number
  affected_mod_count: number
  conflicts: AssetConflictDetail[]
}

export interface ModAssetConflictReport {
  mod_id: number
  mod_name: string
  enabled: boolean
  scan_status: AssetScanStatus
  scan_error?: string | null
  conflicting_asset_count: number
  conflicts: AssetConflictDetail[]
}

export interface AssetConflictReport {
  summary: AssetConflictSummary
  mods: ModAssetConflictReport[]
}

export function getAssetConflictSummary() {
  return invoke<AssetConflictSummary>("get_asset_conflict_summary")
}

export function getAssetConflicts() {
  return invoke<AssetConflictReport>("get_asset_conflicts")
}
