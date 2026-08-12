import { useCallback, useEffect, useMemo, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import {
  Box,
  ExternalLink,
  FolderOpen,
  Layers,
  Loader2,
  Power,
  RefreshCw,
  Trash2,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/EmptyState"
import { PageHeader } from "@/components/layout/PageHeader"
import { InstallPreviewDialog } from "@/components/install/InstallPreviewDialog"
import { InstallOptionsDialog } from "@/components/install/InstallOptionsDialog"
import {
  AssetConflictBadge,
  AssetScanIssueBadge,
} from "@/components/installed/AssetConflictBadge"
import { Skeleton } from "@/components/ui/Skeleton"
import { useModInstall } from "@/hooks/useModInstall"
import {
  getAssetConflicts,
  type AssetConflictReport,
  type ModAssetConflictReport,
} from "@/lib/conflicts"
import {
  checkModUpdates,
  describeInstallError,
  getInstalledMods,
  getInstalledStats,
  setModEnabled,
  uninstallMod,
  type InstalledMod,
  type InstalledStats,
  type ModUpdate,
} from "@/lib/install"
import { toast } from "@/lib/toast"
import { cn } from "@/lib/utils"
import { progressLabel } from "@/lib/utoc"
import { useNavStore } from "@/store/nav"
import { useAuthStore } from "@/store/auth"

export function InstalledPage() {
  const [mods, setMods] = useState<InstalledMod[]>([])
  const [updates, setUpdates] = useState<Map<number, ModUpdate>>(new Map())
  const [conflictReport, setConflictReport] = useState<AssetConflictReport | null>(null)
  const [loading, setLoading] = useState(true)
  const [stats, setStats] = useState<InstalledStats | null>(null)
  const setPage = useNavStore((state) => state.setPage)

  const refresh = useCallback(async () => {
    try {
      const [list, nextStats] = await Promise.all([getInstalledMods(), getInstalledStats()])
      setMods(list)
      setStats(nextStats)
    } catch {
      toast.error("Failed", "Couldn't load installed mods.")
    } finally {
      setLoading(false)
    }

    try {
      setConflictReport(await getAssetConflicts())
    } catch {
      setConflictReport(null)
    }

    try {
      const updateList = await checkModUpdates()
      setUpdates(new Map(updateList.map((update) => [update.mod_id, update])))
    } catch {
      // Nexus update checks are best-effort. Local management must remain available.
      setUpdates(new Map())
    }
  }, [])

  useEffect(() => {
    void refresh()
    const handleChanged = () => void refresh()
    window.addEventListener("mrmmr-installed-changed", handleChanged)
    return () => window.removeEventListener("mrmmr-installed-changed", handleChanged)
  }, [refresh])

  const conflictsByMod = useMemo(
    () => new Map(conflictReport?.mods.map((report) => [report.mod_id, report]) ?? []),
    [conflictReport],
  )

  const handleChanged = useCallback(
    (updated?: InstalledMod) => {
      if (!updated) {
        void refresh()
        return
      }
      setMods((current) =>
        current.map((mod) => (mod.mod_id === updated.mod_id ? updated : mod)),
      )
      setUpdates((current) => {
        const next = new Map(current)
        next.delete(updated.mod_id)
        return next
      })
    },
    [refresh],
  )

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        icon={<Layers className="size-4 text-primary" />}
        title="Installed"
        description="Manage your installed Marvel Rivals mods."
        trailing={
          !loading && stats && stats.mod_count > 0 ? (
            <div className="flex items-center gap-3 text-xs text-muted-foreground">
              <span><strong className="font-semibold text-foreground">{stats.mod_count}</strong> mods</span>
              <span><strong className="font-semibold text-foreground">{stats.enabled_count}</strong> enabled</span>
              <span><strong className="font-semibold text-foreground">{formatBytes(stats.total_size_bytes)}</strong> total</span>
            </div>
          ) : null
        }
      />

      <div className="mx-auto w-full max-w-5xl flex-1 px-6 py-5">
      {loading ? (
        <div className="overflow-hidden rounded-sm border border-border bg-card">
          {Array.from({ length: 4 }, (_, index) => (
            <div key={index} className="flex items-center gap-4 border-b border-border p-4 last:border-0">
              <Skeleton className="size-10 shrink-0" />
              <div className="flex-1 space-y-2">
                <Skeleton className="h-3.5 w-48" />
                <Skeleton className="h-3 w-64" />
              </div>
              <Skeleton className="h-8 w-56" />
            </div>
          ))}
        </div>
      ) : mods.length === 0 ? (
        <EmptyState
          icon={<Box className="size-5" />}
          title="No mods installed"
          description="Browse the Workshop and install a mod. It will appear here automatically."
          action={<Button size="sm" onClick={() => setPage("workshop")}>Browse Workshop</Button>}
        />
      ) : (
        <div className="overflow-hidden rounded-sm border border-border bg-card">
          {mods.map((mod) => (
            <InstalledModRow
              key={mod.mod_id}
              mod={mod}
              update={updates.get(mod.mod_id)}
              conflictReport={conflictsByMod.get(mod.mod_id)}
              onChanged={handleChanged}
            />
          ))}
        </div>
      )}
      </div>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ["KB", "MB", "GB", "TB"]
  let value = bytes / 1024
  let unit = units[0]
  for (let index = 1; index < units.length && value >= 1024; index++) {
    value /= 1024
    unit = units[index]
  }
  return `${value >= 10 ? value.toFixed(1) : value.toFixed(2)} ${unit}`
}

function InstalledModRow({
  mod,
  update,
  conflictReport,
  onChanged,
}: {
  mod: InstalledMod
  update?: ModUpdate
  conflictReport?: ModAssetConflictReport
  onChanged: (updated?: InstalledMod) => void
}) {
  const {
    phase,
    step,
    preview,
    installOptions,
    chosenOption,
    browsing,
    gameRunning,
    runInstall,
    confirmInstallOption,
    cancelInstallOptions,
    browse,
    confirmInstall,
    cancelPreview,
    cancelWaiting,
  } = useModInstall(
    mod.mod_id,
    mod.name,
    true,
    onChanged,
  )
  const premium = useAuthStore((state) => state.session?.user.is_premium ?? false)

  const [busy, setBusy] = useState(false)
  const [confirm, setConfirm] = useState(false)

  async function handleToggle() {
    setBusy(true)
    try {
      const updated = await setModEnabled(mod.mod_id, !mod.enabled)
      onChanged(updated)
      window.dispatchEvent(new Event("mrmmr-installed-changed"))
      toast.success(mod.enabled ? "Disabled" : "Enabled", mod.name)
    } catch (err) {
      const { title, description } = describeInstallError(err)
      toast.error(title, description)
    } finally {
      setBusy(false)
    }
  }

  async function handleUninstall() {
    if (!confirm) {
      setConfirm(true)
      window.setTimeout(() => setConfirm(false), 4000)
      return
    }
    setConfirm(false)
    setBusy(true)
    try {
      await uninstallMod(mod.mod_id)
      onChanged()
      window.dispatchEvent(new Event("mrmmr-installed-changed"))
      toast.info("Uninstalled", `${mod.name} removed.`)
    } catch (error) {
      const { title, description } = describeInstallError(error)
      toast.error(title, description)
    } finally {
      setBusy(false)
    }
  }

  const installing = phase === "installing" || phase === "preparing"
  const waiting = phase === "waiting-download"
  const nexusUrl = `https://www.nexusmods.com/marvelrivals/mods/${mod.mod_id}`
  const thumbnailUrl = mod.picture_url ?? update?.picture_url

  async function openModPage() {
    try {
      await openUrl(nexusUrl)
    } catch {
      toast.error("Couldn't open Nexus Mods", "Open the mod page in your browser and try again.")
    }
  }

  return (
    <div className="flex min-h-[76px] flex-wrap items-center gap-4 border-b border-border px-4 py-3 last:border-b-0">
      <button
        type="button"
        onClick={() => void openModPage()}
        aria-label={`Open ${mod.name} on Nexus Mods`}
        title="Open on Nexus Mods"
        className={cn(
          "size-10 shrink-0 overflow-hidden rounded-sm border border-border bg-muted transition-colors hover:border-primary/60",
          !mod.enabled && !mod.missing && "opacity-55",
        )}
      >
        <ModThumbnail key={thumbnailUrl ?? "fallback"} name={mod.name} imageUrl={thumbnailUrl} />
      </button>
      <div className="min-w-44 flex-1">
          <button
            type="button"
            onClick={() => void openModPage()}
            title="Open on Nexus Mods"
            className="group flex max-w-full items-center gap-1.5 text-left text-sm font-semibold hover:text-primary"
          >
            <span className="truncate">{mod.name}</span>
            <ExternalLink className="size-3.5 shrink-0 text-muted-foreground transition-colors group-hover:text-primary" />
          </button>
          <span className="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
            <span
              className={cn(
                "inline-block size-1.5 rounded-full",
                mod.missing
                  ? "bg-destructive"
                  : mod.enabled
                    ? "bg-emerald-500"
                    : "bg-muted-foreground/40",
              )}
            />
            <span>{mod.missing ? "Files missing" : mod.enabled ? "Enabled" : "Disabled"}</span>
            {mod.version ? <span>· v{mod.version}</span> : null}
            {mod.parts.length > 1 ? <span>· {mod.parts.length} grouped parts</span> : null}
            {conflictReport ? <AssetConflictBadge report={conflictReport} /> : null}
            {conflictReport ? <AssetScanIssueBadge report={conflictReport} /> : null}
            {update?.has_update && !installing ? (
              <span className="rounded-sm border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                v{update.latest_version} available
              </span>
            ) : null}
          </span>
      </div>

        <div className="ml-auto flex shrink-0 items-center gap-2">
          {installing ? (
            <Button size="sm" disabled>
              <Loader2 className="animate-spin" />
              {progressLabel(step ?? "starting")}
            </Button>
          ) : waiting ? (
            <div className="flex flex-wrap items-center justify-end gap-2">
              <span className="text-xs text-muted-foreground">
                {chosenOption?.multipart
                  ? `Waiting for ${chosenOption.files.length} downloads...`
                  : "Waiting for download..."}
              </span>
              <Button size="sm" variant="outline" onClick={browse} disabled={browsing}>
                <FolderOpen />
                {chosenOption?.multipart ? "Choose all downloaded parts" : "Choose downloaded file"}
              </Button>
              <Button size="sm" variant="ghost" onClick={cancelWaiting}>
                Cancel
              </Button>
            </div>
          ) : (
            <>
              <Button
                size="sm"
                variant={mod.enabled ? "outline" : "secondary"}
                onClick={handleToggle}
                disabled={busy || mod.missing || gameRunning}
                title={
                  gameRunning
                    ? "Close Marvel Rivals before changing mod files."
                    : mod.missing
                      ? "Reinstall the mod before toggling it."
                      : undefined
                }
              >
                {busy ? <Loader2 className="animate-spin" /> : <Power />}
                {mod.enabled ? "Disable" : "Enable"}
              </Button>
              <Button
                size="sm"
                onClick={() => void runInstall()}
                disabled={busy || gameRunning || mod.missing || !update?.has_update}
                title={!update?.has_update ? "This mod is up to date." : undefined}
              >
                <RefreshCw />
                Update
              </Button>
              <Button
                size="sm"
                variant="destructive"
                onClick={handleUninstall}
                disabled={busy || gameRunning}
                title={gameRunning ? "Close Marvel Rivals before uninstalling mods." : undefined}
              >
                <Trash2 />
                {confirm ? "Confirm" : "Uninstall"}
              </Button>
            </>
          )}
        </div>
        {preview ? (
          <InstallPreviewDialog
            preview={preview}
            busy={phase === "installing"}
            onConfirm={() => void confirmInstall()}
            onCancel={() => void cancelPreview()}
          />
        ) : null}
        {installOptions ? (
          <InstallOptionsDialog
            modName={mod.name}
            options={installOptions}
            premium={premium}
            onConfirm={(option) => void confirmInstallOption(option)}
            onCancel={cancelInstallOptions}
          />
        ) : null}
    </div>
  )
}

function ModThumbnail({ name, imageUrl }: { name: string; imageUrl?: string | null }) {
  const [failed, setFailed] = useState(false)

  if (!imageUrl || failed) {
    return (
      <span className="grid size-full place-items-center">
        <Layers className="size-4 text-muted-foreground" />
      </span>
    )
  }

  return (
    <img
      src={imageUrl}
      alt={`${name} thumbnail`}
      loading="lazy"
      className="size-full object-cover"
      onError={() => setFailed(true)}
    />
  )
}
