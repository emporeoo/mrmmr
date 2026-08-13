import { useCallback, useEffect, useRef, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import { openUrl } from "@tauri-apps/plugin-opener"
import { open } from "@tauri-apps/plugin-dialog"
import { AlertCircle, CheckCircle2, Download, FolderOpen, Loader2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import { toast } from "@/lib/toast"
import { ARCHIVE_FILE_FILTER } from "@/lib/archives"
import {
  describeUtocError,
  detectUtocDownload,
  getUtocFilesUrl,
  getUtocStatus,
  installUtoc,
  installUtocFromArchive,
  progressLabel,
  type UtocStatus,
} from "@/lib/utoc"
import { useGameStore } from "@/store/game"
import { useAuthStore } from "@/store/auth"
import { useSetupStore } from "@/store/setup"

type Phase = "idle" | "checking" | "installed" | "not-installed" | "installing"

const POLL_INTERVAL = 1500
const POLL_ATTEMPTS = 200
let savedWorkflow: { phase: Phase; step: string | null } = {
  phase: "idle",
  step: null,
}

export function UtocManager() {
  const gameLocation = useGameStore((state) => state.location)
  const gameRunning = useGameStore((state) => state.processStatus.shipping_running)
  const session = useAuthStore((state) => state.session)
  const setUtocInstalled = useSetupStore((state) => state.setUtocInstalled)

  const [phase, setPhase] = useState<Phase>(savedWorkflow.phase)
  const [step, setStep] = useState<string | null>(savedWorkflow.step)
  const resumeWaiting = useRef(
    savedWorkflow.phase === "installing" && savedWorkflow.step === "waiting_for_download",
  )

  useEffect(() => {
    savedWorkflow = { phase, step }
  }, [phase, step])

  useEffect(() => {
    if (!gameLocation) {
      setPhase("idle")
      return
    }
    if (resumeWaiting.current) {
      resumeWaiting.current = false
      return
    }

    let cancelled = false
    setPhase("checking")

    getUtocStatus()
      .then((status) => {
        if (cancelled) return
        setUtocInstalled(status.installed)
        setPhase(status.installed ? "installed" : "not-installed")
      })
      .catch(() => {
        if (!cancelled) setPhase("not-installed")
      })

    return () => {
      cancelled = true
    }
  }, [gameLocation, setUtocInstalled])

  useEffect(() => {
    if (phase !== "installing") return

    let cancelled = false
    let unlisten: (() => void) | null = null
    listen<string>("utoc-progress", (event) => {
      if (!cancelled) setStep(event.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [phase])

  const finishInstall = useCallback(
    (status: UtocStatus) => {
      setUtocInstalled(status.installed)
      setPhase(status.installed ? "installed" : "not-installed")
      toast.success("Success", "UTOC Signature Bypass installed.")
    },
    [setUtocInstalled],
  )

  const handleInstallError = useCallback(
    async (err: unknown) => {
      const { title, description } = describeUtocError(err)
      toast.error(title, description)

      const status = await getUtocStatus().catch(() => null)
      setUtocInstalled(status?.installed ?? false)
      setPhase(status?.installed ? "installed" : "not-installed")
    },
    [setUtocInstalled],
  )

  const installFromPickedArchive = useCallback(
    async (archivePath: string) => {
      setStep("extracting")
      const status = await installUtocFromArchive(archivePath)
      finishInstall(status)
    },
    [finishInstall],
  )

  useEffect(() => {
    if (step !== "waiting_for_download") return

    let stopped = false
    let attempts = 0
    const id = window.setInterval(async () => {
      if (stopped) return
      if (document.visibilityState !== "visible") return

      attempts++
      if (attempts > POLL_ATTEMPTS) {
        stopped = true
        window.clearInterval(id)
        setPhase("not-installed")
        toast.error(
          "Failed",
          "Couldn't find the downloaded file. Use 'Browse for the downloaded file…'.",
        )
        return
      }

      try {
        const archivePath = await detectUtocDownload()
        if (archivePath) {
          stopped = true
          window.clearInterval(id)
          await installFromPickedArchive(archivePath)
        }
      } catch (err) {
        stopped = true
        window.clearInterval(id)
        void handleInstallError(err)
      }
    }, POLL_INTERVAL)

    return () => {
      stopped = true
      window.clearInterval(id)
    }
  }, [step, installFromPickedArchive, handleInstallError])

  const handleBrowseDownload = useCallback(async () => {
    setStep("picked")
    let selected: string | null
    try {
      const result = await open({
        title: "Select the downloaded UTOC mod archive",
        multiple: false,
        filters: [ARCHIVE_FILE_FILTER],
      })
      selected = typeof result === "string" ? result : null
    } catch {
      toast.error("Failed", "Couldn't open the file picker.")
      setStep("waiting_for_download")
      return
    }

    if (!selected) {
      setStep("waiting_for_download")
      return
    }

    try {
      await installFromPickedArchive(selected)
    } catch (err) {
      await handleInstallError(err)
    }
  }, [installFromPickedArchive, handleInstallError])

  const handleInstall = useCallback(async () => {
    setPhase("installing")
    setStep("starting")

    const isPremium = session?.user.is_premium ?? false
    if (isPremium) {
      try {
        const status = await installUtoc()
        finishInstall(status)
      } catch (err) {
        await handleInstallError(err)
      }
    } else {
      try {
        const url = await getUtocFilesUrl()
        await openUrl(url)
        setStep("waiting_for_download")
        toast.info("Waiting for download", "On the Nexus page, click 'Manual Download'.")
      } catch (err) {
        await handleInstallError(err)
      }
    }
  }, [session, finishInstall, handleInstallError])

  const cancelWaiting = useCallback(() => {
    setStep(null)
    setPhase("not-installed")
    savedWorkflow = { phase: "not-installed", step: null }
  }, [])

  if (!gameLocation) {
    return (
      <div className="rounded-sm border border-border bg-[#191919] p-3">
        <p className="text-sm font-medium">Game installation required</p>
        <p className="mt-1 text-xs text-muted-foreground">Locate Marvel Rivals before installing the bypass.</p>
      </div>
    )
  }

  if (phase === "checking") {
    return (
      <div className="flex min-h-16 items-center gap-2 rounded-sm border border-border bg-[#191919] px-3 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        <span className="text-sm">Checking…</span>
      </div>
    )
  }

  if (phase === "installing") {
    return (
      <div className="flex flex-col gap-3 rounded-sm border border-border bg-[#191919] p-3">
        <div className="flex items-center gap-2 text-muted-foreground">
          <Loader2 className="size-4 animate-spin" />
          <span className="text-sm">{progressLabel(step ?? "starting")}</span>
        </div>
        {step === "waiting_for_download" ? (
          <div className="flex flex-col gap-2">
            <Button size="sm" variant="outline" onClick={handleBrowseDownload} disabled={gameRunning}>
              <FolderOpen />
              Browse for the downloaded file…
            </Button>
            <Button size="sm" variant="ghost" onClick={cancelWaiting}>
              Cancel waiting
            </Button>
            <p className="text-muted-foreground text-xs">
              If Nexus shows a &quot;Slow Download&quot; button, click that to actually start the
              download.
            </p>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <Button size="sm" disabled>
              Installing…
            </Button>
          </div>
        )}
      </div>
    )
  }

  if (phase === "installed") {
    return (
      <div className="flex items-start gap-2.5 rounded-sm border border-emerald-500/20 bg-emerald-500/5 p-3">
        <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-emerald-500" />
        <p className="text-sm font-medium">UTOC Signature Bypass is installed.</p>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3 rounded-sm border border-border bg-[#191919] p-3">
      <div className="flex items-start gap-2.5">
        <AlertCircle className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div className="flex flex-col gap-0.5">
          <p className="text-sm font-medium">UTOC Signature Bypass is not installed.</p>
          <p className="text-muted-foreground text-xs">Required for Marvel Rivals mods to load.</p>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Button size="sm" onClick={handleInstall} disabled={gameRunning}>
          <Download />
          Install
        </Button>
        {gameRunning ? (
          <span className="text-xs text-destructive">Close Marvel Rivals to install.</span>
        ) : null}
      </div>
    </div>
  )
}
