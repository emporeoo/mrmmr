import { useCallback, useEffect, useState } from "react"
import { save } from "@tauri-apps/plugin-dialog"
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  FileWarning,
  Loader2,
  RefreshCw,
  Stethoscope,
  Wrench,
} from "lucide-react"

import { PageHeader } from "@/components/layout/PageHeader"
import { Button } from "@/components/ui/button"
import {
  describeDoctorError,
  exportDiagnostics,
  repairModDoctor,
  runModDoctor,
  type DoctorFinding,
  type DoctorReport,
} from "@/lib/doctor"
import { toast } from "@/lib/toast"
import { cn } from "@/lib/utils"
import { useGameStore } from "@/store/game"

export function DoctorPage() {
  const [report, setReport] = useState<DoctorReport | null>(null)
  const [scanning, setScanning] = useState(true)
  const [repairing, setRepairing] = useState(false)
  const [exporting, setExporting] = useState(false)
  const gameRunning = useGameStore((state) => state.processStatus.shipping_running)

  const scan = useCallback(async () => {
    setScanning(true)
    try {
      setReport(await runModDoctor())
    } catch (error) {
      const detail = describeDoctorError(error)
      toast.error(detail.title, detail.description)
    } finally {
      setScanning(false)
    }
  }, [])

  useEffect(() => {
    void scan()
  }, [scan])

  async function repair() {
    setRepairing(true)
    try {
      const result = await repairModDoctor()
      setReport(result.report)
      window.dispatchEvent(new Event("mrmmr-installed-changed"))
      toast.success(
        "Safe repairs complete",
        `${result.quarantined} file(s) quarantined and ${result.metadata_removed} stale entry(s) removed.`,
      )
    } catch (error) {
      const detail = describeDoctorError(error)
      toast.error(detail.title, detail.description)
    } finally {
      setRepairing(false)
    }
  }

  async function exportReport() {
    const destination = await save({
      title: "Export MRMMR diagnostics",
      defaultPath: `mrmmr-diagnostics-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "JSON diagnostic report", extensions: ["json"] }],
    })
    if (!destination) return
    setExporting(true)
    try {
      await exportDiagnostics(destination)
      toast.success("Diagnostics exported", "The report excludes your API key and full game path.")
    } catch (error) {
      const detail = describeDoctorError(error)
      toast.error(detail.title, detail.description)
    } finally {
      setExporting(false)
    }
  }

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        icon={<Stethoscope className="size-4 text-primary" />}
        title="Mod Doctor"
        description="Inspect installed files, ownership, disabled states, metadata, and required setup."
        trailing={
          <div className="flex items-center gap-2">
            <Button size="sm" variant="outline" onClick={() => void exportReport()} disabled={exporting}>
              {exporting ? <Loader2 className="animate-spin" /> : <Download />}
              Export diagnostics
            </Button>
            <Button size="sm" variant="outline" onClick={() => void scan()} disabled={scanning}>
              {scanning ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              Scan again
            </Button>
          </div>
        }
      />

      <div className="mx-auto w-full max-w-5xl flex-1 space-y-4 px-6 py-5">
        {report ? (
          <div className="grid grid-cols-2 overflow-hidden rounded-sm border border-border bg-card md:grid-cols-4">
            <Metric label="Critical" value={report.summary.critical} tone="critical" />
            <Metric label="Warnings" value={report.summary.warning} tone="warning" />
            <Metric label="Healthy notes" value={report.summary.info} />
            <Metric label="Safe repairs" value={report.summary.repairable} />
          </div>
        ) : null}

        {gameRunning ? (
          <div className="flex items-start gap-3 border border-destructive/40 bg-destructive/10 p-3 text-sm">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-destructive" />
            <div>
              <p className="font-semibold">Repairs are locked while Marvel Rivals is running.</p>
              <p className="mt-0.5 text-xs text-muted-foreground">
                Scanning and diagnostic export remain available.
              </p>
            </div>
          </div>
        ) : null}

        {scanning && !report ? (
          <div className="grid min-h-48 place-items-center rounded-sm border border-border bg-card text-sm text-muted-foreground">
            <span className="flex items-center gap-2"><Loader2 className="size-4 animate-spin" /> Inspecting local files…</span>
          </div>
        ) : report ? (
          <div className="overflow-hidden rounded-sm border border-border bg-card">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-3">
              <div>
                <h2 className="text-sm font-semibold">Diagnostic findings</h2>
                <p className="mt-0.5 text-xs text-muted-foreground">
                  Automatic repairs only quarantine orphan files and remove fully stale metadata.
                </p>
              </div>
              <Button
                size="sm"
                onClick={() => void repair()}
                disabled={repairing || gameRunning || report.summary.repairable === 0}
              >
                {repairing ? <Loader2 className="animate-spin" /> : <Wrench />}
                Repair safe issues
              </Button>
            </div>
            {report.findings.map((finding) => (
              <Finding key={finding.id} finding={finding} />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  )
}

function Metric({
  label,
  value,
  tone,
}: {
  label: string
  value: number
  tone?: "critical" | "warning"
}) {
  return (
    <div className="border-b border-r border-border p-4 last:border-r-0 md:border-b-0">
      <p className="text-[10px] font-semibold uppercase tracking-[0.1em] text-muted-foreground">{label}</p>
      <p className={cn(
        "mt-1 text-xl font-bold",
        tone === "critical" && value > 0 && "text-destructive",
        tone === "warning" && value > 0 && "text-primary",
      )}>{value}</p>
    </div>
  )
}

function Finding({ finding }: { finding: DoctorFinding }) {
  const Icon =
    finding.severity === "critical"
      ? FileWarning
      : finding.severity === "warning"
        ? AlertTriangle
        : CheckCircle2
  return (
    <div className="flex items-start gap-3 border-b border-border px-4 py-3 last:border-b-0">
      <Icon className={cn(
        "mt-0.5 size-4 shrink-0",
        finding.severity === "critical"
          ? "text-destructive"
          : finding.severity === "warning"
            ? "text-primary"
            : "text-emerald-500",
      )} />
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <p className="text-sm font-semibold">{finding.title}</p>
          {finding.repair ? (
            <span className="border border-primary/35 bg-primary/10 px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider text-primary">
              Safe repair
            </span>
          ) : null}
        </div>
        <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{finding.description}</p>
        {finding.mod_name ? <p className="mt-1 text-xs">Mod: {finding.mod_name}</p> : null}
        {finding.path ? (
          <code className="mt-1 block break-all text-[11px] text-muted-foreground">{finding.path}</code>
        ) : null}
      </div>
    </div>
  )
}
