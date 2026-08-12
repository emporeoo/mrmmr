import { useEffect, useState, type ReactNode } from "react"
import { Archive, Gamepad2, Loader2, RotateCcw, Settings2, ShieldCheck } from "lucide-react"

import { PageHeader } from "@/components/layout/PageHeader"
import { GameDetection } from "@/components/settings/GameDetection"
import { UtocManager } from "@/components/settings/UtocManager"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  getPreferences,
  resetAllData,
  setAutoDeleteModArchives,
} from "@/lib/settings"
import { toast } from "@/lib/toast"
import { useAuthStore } from "@/store/auth"
import { useGameStore } from "@/store/game"

function SettingsSection({
  icon,
  title,
  description,
  required = false,
  children,
}: {
  icon: ReactNode
  title: string
  description: string
  required?: boolean
  children: ReactNode
}) {
  return (
    <section className="grid gap-5 border-b border-border p-5 last:border-b-0 lg:grid-cols-[220px_minmax(0,1fr)]">
      <div>
        <div className="flex items-center gap-2">
          <span className="text-primary">{icon}</span>
          <h2 className="text-sm font-semibold">{title}</h2>
          {required ? (
            <span className="rounded-sm border border-destructive/50 bg-destructive/10 px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-[0.08em] text-destructive">
              Required setup
            </span>
          ) : null}
        </div>
        <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">{description}</p>
      </div>
      <div className="min-w-0">{children}</div>
    </section>
  )
}

export function SettingsPage() {
  const signOut = useAuthStore((state) => state.signOut)
  const gameRunning = useGameStore((state) => state.processStatus.shipping_running)
  const [confirmingReset, setConfirmingReset] = useState(false)
  const [resetting, setResetting] = useState(false)
  const [autoDeleteArchives, setAutoDeleteArchives] = useState(false)
  const [loadingPreferences, setLoadingPreferences] = useState(true)
  const [savingPreferences, setSavingPreferences] = useState(false)

  useEffect(() => {
    getPreferences()
      .then((preferences) => setAutoDeleteArchives(preferences.auto_delete_mod_archives))
      .catch(() => toast.error("Couldn't load settings", "Download preferences are unavailable."))
      .finally(() => setLoadingPreferences(false))
  }, [])

  async function handleAutoDeleteArchives(enabled: boolean) {
    setSavingPreferences(true)
    try {
      const preferences = await setAutoDeleteModArchives(enabled)
      setAutoDeleteArchives(preferences.auto_delete_mod_archives)
      toast.success("Preference saved", "Archive cleanup has been updated.")
    } catch {
      toast.error("Couldn't save setting", "The archive cleanup preference was not changed.")
    } finally {
      setSavingPreferences(false)
    }
  }

  async function handleReset() {
    if (!confirmingReset) {
      setConfirmingReset(true)
      window.setTimeout(() => setConfirmingReset(false), 4000)
      return
    }
    setConfirmingReset(false)
    setResetting(true)
    try {
      await resetAllData()
      await signOut()
      toast.success("Application reset", "Local MRMMR data has been removed.")
    } catch (error) {
      const locked = (error as { kind?: string })?.kind === "game_files_locked"
      toast.error(
        locked ? "Marvel Rivals is running" : "Reset failed",
        locked
          ? "Close the game before resetting MRMMR so every game file can be removed."
          : "MRMMR couldn't remove all local data.",
      )
    } finally {
      setResetting(false)
    }
  }

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        icon={<Settings2 className="size-4" />}
        title="Settings"
        description="Configure Marvel Rivals, mod requirements, and local file handling."
      />

      <div className="mx-auto w-full max-w-5xl flex-1 px-6 py-5">
        <div className="overflow-hidden rounded-sm border border-border bg-card">
          <SettingsSection
            icon={<Gamepad2 className="size-4" />}
            title="Game installation"
            required
            description="MRMMR detects Steam and Epic installations automatically. You can still choose a custom folder."
          >
            <GameDetection />
          </SettingsSection>

          <SettingsSection
            icon={<ShieldCheck className="size-4" />}
            title="UTOC Signature Bypass"
            required
            description="This required compatibility component allows Marvel Rivals to load installed mods."
          >
            <UtocManager />
          </SettingsSection>

          <SettingsSection
            icon={<Archive className="size-4" />}
            title="Downloaded archives"
            description="Control what happens to ZIP, 7z, RAR, TAR, GZ, and TGZ files after installation."
          >
            <label className="flex cursor-pointer items-start gap-3 rounded-sm border border-border bg-[#191919] p-3">
              <Checkbox
                className="mt-0.5"
                checked={autoDeleteArchives}
                disabled={loadingPreferences || savingPreferences}
                onCheckedChange={(enabled) => void handleAutoDeleteArchives(enabled)}
              />
              <span className="flex min-w-0 flex-col gap-1">
                <span className="text-sm font-medium">
                  Delete archives after successful extraction
                </span>
                <span className="text-xs leading-relaxed text-muted-foreground">
                  Failed, cancelled, or mismatched installs keep their original archive.
                </span>
              </span>
              {savingPreferences ? (
                <Loader2 className="ml-auto size-4 shrink-0 animate-spin text-muted-foreground" />
              ) : null}
            </label>
          </SettingsSection>

          <SettingsSection
            icon={<RotateCcw className="size-4" />}
            title="Reset application"
            description="Remove saved credentials, preferences, game location, installed-mod metadata, and the UTOC bypass."
          >
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="max-w-md text-xs leading-relaxed text-muted-foreground">
                This does not uninstall Marvel Rivals. The action cannot be undone.
              </p>
              <Button
                variant="destructive"
                onClick={handleReset}
                disabled={resetting || gameRunning}
                title={gameRunning ? "Close Marvel Rivals before resetting application data." : undefined}
              >
                {resetting ? <Loader2 className="animate-spin" /> : <RotateCcw />}
                {confirmingReset ? "Confirm reset" : "Reset all data"}
              </Button>
              {gameRunning ? (
                <p className="w-full text-xs text-destructive">
                  Close Marvel Rivals to unlock reset and game-file changes.
                </p>
              ) : null}
            </div>
          </SettingsSection>
        </div>
      </div>
    </div>
  )
}
