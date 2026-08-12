import { useState } from "react"
import {
  BadgeInfo,
  Compass,
  Gamepad2,
  Layers,
  Loader2,
  LogOut,
  PanelLeftClose,
  PanelLeftOpen,
  Power,
  Settings2,
  Stethoscope,
  TriangleAlert,
} from "lucide-react"

import { AccountTierBadge } from "@/components/ui/AccountTierBadge"
import { AppIcon } from "@/components/ui/AppIcon"
import { Button } from "@/components/ui/button"
import { UserAvatar } from "@/components/ui/UserAvatar"
import type { AssetConflictSummary } from "@/lib/conflicts"
import { describeGameError, closeGame, launchGame } from "@/lib/game"
import { toast } from "@/lib/toast"
import { cn } from "@/lib/utils"
import { useAuthStore } from "@/store/auth"
import { useGameStore } from "@/store/game"
import { useNavStore, type Page } from "@/store/nav"

const NAV_ITEMS: { page: Page; label: string; icon: typeof Compass }[] = [
  { page: "workshop", label: "Workshop", icon: Compass },
  { page: "installed", label: "Installed", icon: Layers },
  { page: "doctor", label: "Mod Doctor", icon: Stethoscope },
  { page: "settings", label: "Settings", icon: Settings2 },
  { page: "credits", label: "Credits", icon: BadgeInfo },
]

export function Sidebar({ conflictSummary }: { conflictSummary?: AssetConflictSummary | null }) {
  const session = useAuthStore((state) => state.session)
  const signOut = useAuthStore((state) => state.signOut)
  const page = useNavStore((state) => state.page)
  const setPage = useNavStore((state) => state.setPage)
  const open = useNavStore((state) => state.sidebarOpen)
  const toggleSidebar = useNavStore((state) => state.toggleSidebar)
  const gameFound = useGameStore((state) => state.status === "found")
  const processStatus = useGameStore((state) => state.processStatus)
  const refreshProcessStatus = useGameStore((state) => state.refreshProcessStatus)

  const [signingOut, setSigningOut] = useState(false)
  const [launching, setLaunching] = useState(false)
  const gameRunning = processStatus.shipping_running
  const launcherOpen = processStatus.game_running && !processStatus.shipping_running
  const user = session?.user

  async function handleSignOut() {
    setSigningOut(true)
    try {
      await signOut()
    } finally {
      setSigningOut(false)
    }
  }

  async function handleLaunch() {
    setLaunching(true)
    try {
      if (gameRunning) {
        await closeGame()
        toast.success("Marvel Rivals closed", "The game process was shut down.")
      } else {
        await launchGame()
        toast.success("Launching Marvel Rivals", "The game launcher is starting.")
      }
    } catch (error) {
      const { title, description } = describeGameError(error)
      toast.error(title, description)
    } finally {
      await refreshProcessStatus()
      setLaunching(false)
    }
  }

  return (
    <aside
      className={cn(
        "relative z-30 flex shrink-0 flex-col border-r border-sidebar-border bg-sidebar transition-[width] duration-200 ease-out",
        open ? "w-56" : "w-16",
      )}
    >
      <div className="flex h-[72px] items-center border-b border-sidebar-border px-3">
        <AppIcon className="size-8 rounded-sm" />
        <div
          className={cn(
            "ml-2 min-w-0 overflow-hidden transition-[opacity,width] duration-150",
            open ? "w-28 opacity-100" : "w-0 opacity-0",
          )}
        >
          <p className="whitespace-nowrap text-sm font-bold tracking-[0.08em]">MRMMR</p>
          <p className="whitespace-nowrap text-[9px] uppercase tracking-[0.12em] text-muted-foreground">
            Mod manager
          </p>
        </div>
        <button
          type="button"
          aria-label={open ? "Collapse sidebar" : "Expand sidebar"}
          title={open ? "Collapse sidebar" : "Expand sidebar"}
          onClick={toggleSidebar}
          className={cn(
            "ml-auto grid size-8 shrink-0 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-foreground",
            !open &&
              "absolute -right-3 top-6 size-6 border border-sidebar-border bg-sidebar shadow-sm",
          )}
        >
          {open ? (
            <PanelLeftClose className="size-4" />
          ) : (
            <PanelLeftOpen className="size-3.5" />
          )}
        </button>
      </div>

      <nav aria-label="Primary navigation" className="flex flex-col gap-1 p-2">
        {NAV_ITEMS.map(({ page: target, label, icon: Icon }) => {
          const conflictCount =
            target === "installed" ? (conflictSummary?.affected_mod_count ?? 0) : 0
          const accessibleLabel =
            conflictCount > 0
              ? `${label}, ${conflictCount} mod${conflictCount === 1 ? " has" : "s have"} asset conflicts`
              : label
          return (
            <button
              key={target}
              type="button"
              aria-current={page === target ? "page" : undefined}
              aria-label={accessibleLabel}
              title={!open ? label : undefined}
              onClick={() => setPage(target)}
              className={cn(
                "relative flex h-9 items-center rounded-sm px-3 text-sm font-medium transition-colors duration-150",
                page === target
                  ? "bg-sidebar-accent text-foreground before:absolute before:inset-y-2 before:left-0 before:w-0.5 before:bg-primary"
                  : "text-muted-foreground hover:bg-sidebar-accent/70 hover:text-foreground",
              )}
            >
              <Icon className="size-4 shrink-0" />
              <span
                className={cn(
                  "ml-3 overflow-hidden whitespace-nowrap text-left transition-opacity duration-150",
                  open ? "opacity-100" : "opacity-0",
                )}
              >
                {label}
              </span>
              {conflictCount > 0 ? (
                open ? (
                  <span
                    title={`${conflictCount} installed mod${conflictCount === 1 ? " has" : "s have"} asset conflicts`}
                    className="ml-auto inline-flex h-5 min-w-5 items-center justify-center gap-1 rounded-sm border border-destructive/45 bg-destructive/10 px-1 text-[10px] font-semibold text-destructive"
                  >
                    <TriangleAlert className="size-3" />
                    {conflictCount}
                  </span>
                ) : (
                  <span className="absolute right-1 top-1 grid size-3.5 place-items-center rounded-full border border-sidebar bg-destructive text-[8px] font-bold leading-none text-destructive-foreground">
                    {conflictCount > 9 ? "9+" : conflictCount}
                  </span>
                )
              ) : null}
            </button>
          )
        })}
      </nav>

      <div className="mt-auto border-t border-sidebar-border p-2">
        <Button
          onClick={handleLaunch}
          disabled={!gameFound || launching || launcherOpen}
          title={
            !gameFound
              ? "Locate Marvel Rivals in Settings first"
              : gameRunning
                ? "Close Marvel Rivals"
                : launcherOpen
                  ? "The Marvel Rivals launcher is open"
                : "Launch Marvel Rivals"
          }
          aria-label={
            gameRunning
              ? "Close Marvel Rivals"
              : launcherOpen
                ? "Marvel Rivals launcher is open"
                : "Launch Marvel Rivals"
          }
          className={cn(
            "w-full",
            open ? "justify-center" : "px-0",
            gameRunning
              ? "bg-red-600 text-white hover:bg-red-600/90"
              : launcherOpen
                ? "bg-muted text-muted-foreground"
              : "bg-emerald-600 text-white hover:bg-emerald-600/90",
          )}
        >
          {launching ? (
            <Loader2 className="animate-spin" />
          ) : gameRunning ? (
            <Power />
          ) : (
            <Gamepad2 />
          )}
          {open ? (
            <span>
              {launching
                ? "Working…"
                : gameRunning
                  ? "Close game"
                  : launcherOpen
                    ? "Launcher open"
                    : "Launch game"}
            </span>
          ) : null}
        </Button>
      </div>

      <div className="flex min-h-[58px] items-center gap-2 border-t border-sidebar-border p-2">
        <UserAvatar name={user?.name ?? "?"} imageUrl={user?.profile_url} />
        <div
          className={cn(
            "min-w-0 flex-1 overflow-hidden transition-opacity duration-150",
            open ? "opacity-100" : "pointer-events-none opacity-0",
          )}
        >
          <p className="truncate text-xs font-medium">{user?.name}</p>
          {user ? <AccountTierBadge user={user} className="mt-1" /> : null}
        </div>
        {open ? (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Sign out"
            title="Sign out"
            onClick={handleSignOut}
            disabled={signingOut}
            className="text-muted-foreground hover:text-destructive"
          >
            {signingOut ? <Loader2 className="animate-spin" /> : <LogOut />}
          </Button>
        ) : null}
      </div>
    </aside>
  )
}
