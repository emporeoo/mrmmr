import { useCallback, useState } from "react"
import { open } from "@tauri-apps/plugin-dialog"
import { AlertCircle, CheckCircle2, FolderOpen, Loader2, RefreshCw } from "lucide-react"

import { Button } from "@/components/ui/button"
import { toast } from "@/lib/toast"
import { describeGameError, openModsFolder, type GameLocation } from "@/lib/game"
import { useGameStore } from "@/store/game"

const SOURCE_LABELS: Record<GameLocation["source"], string> = {
  steam: "Steam library",
  epic: "Epic Games",
  manual: "Custom folder",
}

export function GameDetection() {
  const status = useGameStore((state) => state.status)
  const location = useGameStore((state) => state.location)
  const initialize = useGameStore((state) => state.initialize)
  const saveLocation = useGameStore((state) => state.saveLocation)

  const [browsing, setBrowsing] = useState(false)
  const [openingModsFolder, setOpeningModsFolder] = useState(false)

  const handleBrowse = useCallback(async () => {
    let selected: string | null
    try {
      const result = await open({
        title: "Select your Marvel Rivals installation",
        directory: true,
        multiple: false,
      })
      selected = typeof result === "string" ? result : null
    } catch {
      toast.error("Failed", "Couldn't open the folder picker.")
      return
    }
    if (!selected) return

    setBrowsing(true)
    try {
      await saveLocation(selected)
      toast.success("Success", "Game location saved.")
    } catch (err) {
      const { title, description } = describeGameError(err)
      toast.error(title, description)
    } finally {
      setBrowsing(false)
    }
  }, [saveLocation])

  const handleOpenModsFolder = useCallback(async () => {
    setOpeningModsFolder(true)
    try {
      await openModsFolder()
    } catch (err) {
      const { title, description } = describeGameError(err)
      toast.error(title, description)
    } finally {
      setOpeningModsFolder(false)
    }
  }, [])

  if (status === "loading") {
    return (
      <div className="flex min-h-16 items-center gap-2 rounded-sm border border-border bg-[#191919] px-3 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        <span className="text-sm">Searching Steam and Epic Games libraries…</span>
      </div>
    )
  }

  if (status === "found" && location) {
    return (
      <div className="flex flex-wrap items-center justify-between gap-4 rounded-sm border border-border bg-[#191919] p-3">
        <div className="flex min-w-0 flex-1 items-start gap-2.5">
          <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-emerald-500" />
          <div className="flex min-w-0 flex-col gap-0.5">
            <p className="text-sm font-medium">Found in {SOURCE_LABELS[location.source]}</p>
            <p className="mt-1 break-all font-mono text-[11px] leading-relaxed text-muted-foreground">
              {location.path}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            variant="outline"
            size="sm"
            onClick={handleOpenModsFolder}
            disabled={openingModsFolder}
          >
            {openingModsFolder ? <Loader2 className="animate-spin" /> : <FolderOpen />}
            Open mods folder
          </Button>
          <Button variant="outline" size="sm" onClick={handleBrowse} disabled={browsing}>
            Change
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-3 rounded-sm border border-border bg-[#191919] p-3">
      <div className="flex items-center gap-2.5">
        <AlertCircle className="size-4 shrink-0 text-muted-foreground" />
        <p className="text-sm font-medium">Not found</p>
      </div>
      <p className="text-muted-foreground text-xs">
        We couldn&apos;t find Marvel Rivals on this device automatically. Select the game folder
        manually.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={() => void initialize()}>
          <RefreshCw />
          Search again
        </Button>
        <Button variant="outline" size="sm" onClick={handleBrowse} disabled={browsing}>
          <FolderOpen />
          Choose folder
        </Button>
      </div>
    </div>
  )
}
