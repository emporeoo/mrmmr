import { invoke } from "@tauri-apps/api/core"

export type DoctorSeverity = "critical" | "warning" | "info"
export type DoctorRepair = "quarantine_orphan" | "remove_stale_metadata"

export interface DoctorFinding {
  id: string
  severity: DoctorSeverity
  title: string
  description: string
  path?: string | null
  mod_id?: number | null
  mod_name?: string | null
  repair?: DoctorRepair | null
}

export interface DoctorReport {
  scanned_at: number
  game_running: boolean
  summary: {
    critical: number
    warning: number
    info: number
    repairable: number
  }
  findings: DoctorFinding[]
}

export interface DoctorRepairResult {
  repaired: number
  quarantined: number
  metadata_removed: number
  report: DoctorReport
}

export function runModDoctor(): Promise<DoctorReport> {
  return invoke<DoctorReport>("run_mod_doctor")
}

export function repairModDoctor(): Promise<DoctorRepairResult> {
  return invoke<DoctorRepairResult>("repair_mod_doctor")
}

export function exportDiagnostics(destination: string): Promise<string> {
  return invoke<string>("export_diagnostics", { destination })
}

export function describeDoctorError(error: unknown) {
  const value = error as { kind?: string; message?: string }
  if (value?.kind === "game_files_locked") {
    return {
      title: "Marvel Rivals is running",
      description: "Close the game before repairing game files.",
    }
  }
  return {
    title: "Mod Doctor failed",
    description: value?.message ?? "The diagnostic operation could not be completed.",
  }
}
