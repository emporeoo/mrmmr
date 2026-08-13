import { useCallback, useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import { open } from "@tauri-apps/plugin-dialog"
import { openUrl } from "@tauri-apps/plugin-opener"

import { ARCHIVE_FILE_FILTER } from "@/lib/archives"
import {
  describeInstallError,
  commitModInstall,
  detectModDownload,
  discardModInstall,
  getModInstallOptions,
  getModFilesUrl,
  prepareModInstall,
  prepareModInstallFromArchive,
  uninstallMod,
  type InstallPreview,
  type InstalledMod,
  type ModInstallOption,
} from "@/lib/install"
import { toast } from "@/lib/toast"
import { useAuthStore } from "@/store/auth"
import { useGameStore } from "@/store/game"
import { useNavStore } from "@/store/nav"
import { useSetupStore } from "@/store/setup"

export type ModInstallPhase =
  | "idle"
  | "choosing-files"
  | "preparing"
  | "preview"
  | "installing"
  | "waiting-download"
  | "installed"

const POLL_INTERVAL = 1500
const POLL_ATTEMPTS = 200
const WORKFLOW_TTL = 30 * 60 * 1000

interface CachedWorkflow {
  phase: ModInstallPhase
  step: string | null
  preview: InstallPreview | null
  installOptions: ModInstallOption[] | null
  chosenOption: ModInstallOption | null
  updatedAt: number
}

const workflows = new Map<number, CachedWorkflow>()

function cachedWorkflow(modId: number): CachedWorkflow | null {
  const cached = workflows.get(modId)
  if (!cached) return null
  if (Date.now() - cached.updatedAt > WORKFLOW_TTL) {
    workflows.delete(modId)
    return null
  }
  return cached
}

export function useModInstall(
  modId: number,
  modName: string,
  initialInstalled = false,
  onInstalled?: (installed: InstalledMod) => void,
) {
  const session = useAuthStore((state) => state.session)
  const gameStatus = useGameStore((state) => state.status)
  const gameRunning = useGameStore((state) => state.processStatus.shipping_running)
  const utocInstalled = useSetupStore((state) => state.utocInstalled)
  const setPage = useNavStore((state) => state.setPage)

  const initial = cachedWorkflow(modId)
  const [phase, setPhase] = useState<ModInstallPhase>(
    initial?.phase ?? (initialInstalled ? "installed" : "idle"),
  )
  const [step, setStep] = useState<string | null>(initial?.step ?? null)
  const [preview, setPreview] = useState<InstallPreview | null>(initial?.preview ?? null)
  const [installOptions, setInstallOptions] = useState<ModInstallOption[] | null>(
    initial?.installOptions ?? null,
  )
  const [chosenOption, setChosenOption] = useState<ModInstallOption | null>(
    initial?.chosenOption ?? null,
  )
  const [browsing, setBrowsing] = useState(false)
  const [uninstallConfirm, setUninstallConfirm] = useState(false)
  const [uninstalling, setUninstalling] = useState(false)

  useEffect(() => {
    if (initialInstalled && phase === "idle") setPhase("installed")
    if (!initialInstalled && phase === "installed") setPhase("idle")
  }, [initialInstalled]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    workflows.set(modId, {
      phase,
      step,
      preview,
      installOptions,
      chosenOption,
      updatedAt: Date.now(),
    })
  }, [modId, phase, step, preview, installOptions, chosenOption])

  useEffect(() => {
    if (
      phase !== "preparing" &&
      phase !== "installing" &&
      phase !== "waiting-download"
    ) {
      return
    }

    let cancelled = false
    let unlisten: (() => void) | null = null
    listen<string>("mod-install-progress", (event) => {
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

  const finish = useCallback(
    (installed: InstalledMod) => {
      workflows.delete(modId)
      setPhase("installed")
      setStep(null)
      setPreview(null)
      setInstallOptions(null)
      setChosenOption(null)
      toast.success("Success", `${installed.name} installed.`)
      onInstalled?.(installed)
      window.dispatchEvent(new Event("mrmmr-installed-changed"))
    },
    [modId, onInstalled],
  )

  const handleError = useCallback((err: unknown) => {
    const { title, description } = describeInstallError(err)
    toast.error(title, description)
    setPhase(initialInstalled ? "installed" : "idle")
    setStep(null)
    setPreview(null)
    setInstallOptions(null)
    setChosenOption(null)
    workflows.delete(modId)
  }, [initialInstalled, modId])

  const showPreview = useCallback((next: InstallPreview) => {
    setPreview(next)
    setPhase("preview")
    setStep(null)
  }, [])

  const runInstall = useCallback(async () => {
    if (gameRunning) {
      toast.error("Marvel Rivals is running", "Close the game before changing mod files.")
      return
    }
    if (gameStatus !== "found") {
      toast.error("Setup required", "Locate Marvel Rivals in Settings first.")
      setPage("settings")
      return
    }
    if (!utocInstalled) {
      toast.error("Setup required", "Install the UTOC Signature Bypass in Settings first.")
      setPage("settings")
      return
    }

    setPhase("preparing")
    setStep("fetching_files")
    try {
      const options = await getModInstallOptions(modId)
      setInstallOptions(options)
      setPhase("choosing-files")
      setStep(null)
    } catch (err) {
      handleError(err)
    }
  }, [gameRunning, gameStatus, utocInstalled, modId, handleError, setPage])

  const confirmInstallOption = useCallback(async (option: ModInstallOption) => {
    setChosenOption(option)
    setInstallOptions(null)
    try {
      if (session?.user.is_premium) {
        setPhase("preparing")
        setStep("downloading_for_preview")
        showPreview(await prepareModInstall(modId, option.files.map((file) => file.file_id)))
      } else {
        const url = await getModFilesUrl(modId)
        await openUrl(url)
        setPhase("waiting-download")
        setStep("waiting_for_download")
        toast.info(
          option.multipart ? "Download every listed part" : "Waiting for download",
          option.multipart
            ? `Download the ${option.files.length} selected Nexus files, then choose them together in MRMMR.`
            : "On the Nexus page, click 'Manual Download'.",
        )
      }
    } catch (err) {
      handleError(err)
    }
  }, [session, modId, showPreview, handleError])

  const cancelInstallOptions = useCallback(() => {
    setInstallOptions(null)
    setChosenOption(null)
    setStep(null)
    setPhase(initialInstalled ? "installed" : "idle")
  }, [initialInstalled])

  useEffect(() => {
    if (phase !== "waiting-download") return
    if (!chosenOption || chosenOption.files.length !== 1) return

    let stopped = false
    let attempts = 0
    const id = window.setInterval(async () => {
      if (stopped) return
      if (document.visibilityState !== "visible") return

      attempts++
      if (attempts > POLL_ATTEMPTS) {
        stopped = true
        window.clearInterval(id)
        setPhase(initialInstalled ? "installed" : "idle")
        toast.error(
          "Failed",
          "Couldn't find the downloaded file. Use 'Browse for the downloaded file…'.",
        )
        return
      }

      try {
        const path = await detectModDownload(
          modId,
          chosenOption.files.map((file) => file.file_id),
        )
        if (path) {
          stopped = true
          window.clearInterval(id)
          setPhase("preparing")
          setStep("verifying_archive")
          showPreview(await prepareModInstallFromArchive(
            modId,
            [path],
            chosenOption.files.map((file) => file.file_id),
          ))
        }
      } catch (err) {
        stopped = true
        window.clearInterval(id)
        handleError(err)
      }
    }, POLL_INTERVAL)

    return () => {
      stopped = true
      window.clearInterval(id)
    }
  }, [phase, modId, chosenOption, initialInstalled, showPreview, handleError])

  const browse = useCallback(async () => {
    setBrowsing(true)
    setStep("picked")
    let selected: string[]
    try {
      const result = await open({
        title: chosenOption?.multipart
          ? `Select every downloaded part for ${modName}`
          : `Select the downloaded archive for ${modName}`,
        multiple: chosenOption?.multipart ?? false,
        filters: [ARCHIVE_FILE_FILTER],
      })
      selected = typeof result === "string" ? [result] : Array.isArray(result) ? result : []
    } catch {
      toast.error("Failed", "Couldn't open the file picker.")
      setBrowsing(false)
      setPhase("waiting-download")
      return
    }
    setBrowsing(false)

    if (selected.length === 0) {
      setPhase("waiting-download")
      return
    }
    const expectedCount = chosenOption?.files.length ?? 1
    if (selected.length !== expectedCount) {
      toast.error(
        "Wrong number of archives",
        `Select all ${expectedCount} archive${expectedCount === 1 ? "" : "s"} from the confirmed Nexus file group.`,
      )
      setPhase("waiting-download")
      return
    }

    try {
      setPhase("preparing")
      setStep("verifying_archive")
      showPreview(await prepareModInstallFromArchive(
        modId,
        selected,
        chosenOption?.files.map((file) => file.file_id) ?? [],
      ))
    } catch (err) {
      handleError(err)
    }
  }, [modId, modName, chosenOption, showPreview, handleError])

  const confirmInstall = useCallback(async () => {
    if (!preview?.can_install) return
    setPhase("installing")
    setStep("copying")
    try {
      finish(await commitModInstall(preview.plan_id))
    } catch (err) {
      handleError(err)
    }
  }, [preview, finish, handleError])

  const cancelPreview = useCallback(async () => {
    const planId = preview?.plan_id
    setPreview(null)
    setChosenOption(null)
    setStep(null)
    setPhase(initialInstalled ? "installed" : "idle")
    workflows.delete(modId)
    if (planId) {
      try {
        await discardModInstall(planId)
      } catch {
        // The backend also drops staged plans when the app exits or a newer plan replaces them.
      }
    }
  }, [preview, initialInstalled, modId])

  const cancelWaiting = useCallback(() => {
    setStep(null)
    setChosenOption(null)
    setInstallOptions(null)
    setPhase(initialInstalled ? "installed" : "idle")
    workflows.delete(modId)
  }, [initialInstalled, modId])

  const runUninstall = useCallback(async () => {
    if (!uninstallConfirm) {
      setUninstallConfirm(true)
      window.setTimeout(() => setUninstallConfirm(false), 4000)
      return
    }
    setUninstallConfirm(false)
    setUninstalling(true)
    try {
      await uninstallMod(modId)
      setPhase("idle")
      setStep(null)
      toast.info("Uninstalled", `${modName} removed.`)
      window.dispatchEvent(new Event("mrmmr-installed-changed"))
    } catch (err) {
      const { title, description } = describeInstallError(err)
      toast.error(title, description)
    } finally {
      setUninstalling(false)
    }
  }, [uninstallConfirm, modId, modName])

  return {
    phase,
    step,
    preview,
    installOptions,
    chosenOption,
    browsing,
    gameRunning,
    uninstalling,
    uninstallConfirm,
    runInstall,
    confirmInstallOption,
    cancelInstallOptions,
    browse,
    confirmInstall,
    cancelPreview,
    cancelWaiting,
    runUninstall,
  }
}
