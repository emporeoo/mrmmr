import { useEffect, useState } from "react"
import { getVersion } from "@tauri-apps/api/app"
import { CheckCircle2, CircleAlert, CircleUserRound, Gamepad2, Loader2, Undo2 } from "lucide-react"

import { getLastInstallChange, undoLastInstallChange, type UndoStatus } from "@/lib/install"
import { toast } from "@/lib/toast"
import { useAuthStore } from "@/store/auth"
import { useGameStore } from "@/store/game"
import { useSetupStore } from "@/store/setup"

export function AppStatusBar() {
  const user = useAuthStore((state) => state.session?.user)
  const gameStatus = useGameStore((state) => state.status)
  const location = useGameStore((state) => state.location)
  const utocInstalled = useSetupStore((state) => state.utocInstalled)
  const [version, setVersion] = useState("")
  const [undoStatus, setUndoStatus] = useState<UndoStatus>({ available: false })
  const [undoing, setUndoing] = useState(false)

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {})
  }, [])

  useEffect(() => {
    const refreshUndo = () => {
      getLastInstallChange()
        .then(setUndoStatus)
        .catch(() => setUndoStatus({ available: false }))
    }
    refreshUndo()
    window.addEventListener("mrmmr-installed-changed", refreshUndo)
    return () => window.removeEventListener("mrmmr-installed-changed", refreshUndo)
  }, [])

  async function handleUndo() {
    setUndoing(true)
    try {
      await undoLastInstallChange()
      setUndoStatus({ available: false })
      window.dispatchEvent(new Event("mrmmr-installed-changed"))
      toast.success("Change undone", "The previous installed-mod state has been restored.")
    } catch {
      toast.error("Couldn't undo change", "The mod changed after installation or its backup is unavailable.")
      getLastInstallChange().then(setUndoStatus).catch(() => {})
    } finally {
      setUndoing(false)
    }
  }

  const source = location
    ? location.source === "epic"
      ? "Epic installation"
      : location.source === "steam"
        ? "Steam installation"
        : "Custom installation"
    : null

  return (
    <footer
      aria-label="Application status"
      className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-[#181818] px-3 text-[10px] text-muted-foreground"
    >
      <span className="flex items-center gap-1.5">
        <CheckCircle2 className="size-3 text-emerald-500" />
        Nexus connected
      </span>
      <span className="hidden min-w-0 items-center gap-1.5 sm:flex">
        <Gamepad2 className="size-3" />
        <span className="truncate">
          {gameStatus === "loading" ? "Locating Marvel Rivals" : source ?? "Game not located"}
        </span>
      </span>
      <span className="hidden items-center gap-1.5 md:flex">
        {utocInstalled ? (
          <CheckCircle2 className="size-3 text-emerald-500" />
        ) : (
          <CircleAlert className="size-3 text-primary" />
        )}
        UTOC {utocInstalled ? "ready" : "required"}
      </span>
      {undoStatus.available ? (
        <button
          type="button"
          onClick={() => void handleUndo()}
          disabled={undoing}
          title={`Undo ${undoStatus.label ?? "last install change"}`}
          className="flex max-w-56 items-center gap-1.5 truncate rounded-sm px-1.5 py-0.5 text-primary transition-colors hover:bg-primary/10 disabled:opacity-50"
        >
          {undoing ? <Loader2 className="size-3 shrink-0 animate-spin" /> : <Undo2 className="size-3 shrink-0" />}
          <span className="hidden truncate lg:inline">
            Undo {undoStatus.label ?? "last install"}
          </span>
        </button>
      ) : null}
      <span className="ml-auto flex items-center gap-1.5">
        <CircleUserRound className="size-3" />
        {user?.is_premium ? "Premium" : user?.is_supporter ? "Supporter" : "Free"}
      </span>
      {version ? <span>MRMMR {version}</span> : null}
    </footer>
  )
}
