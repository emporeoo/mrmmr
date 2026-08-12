import { lazy, Suspense, useEffect, useRef } from "react"

import { AppStatusBar } from "@/components/layout/AppStatusBar"
import { Sidebar } from "@/components/layout/Sidebar"
import { useAssetConflictSummary } from "@/hooks/useAssetConflictSummary"
import { useGameStore } from "@/store/game"
import { useNavStore } from "@/store/nav"

const WorkshopPage = lazy(() =>
  import("@/components/workshop/WorkshopPage").then(({ WorkshopPage }) => ({
    default: WorkshopPage,
  })),
)
const InstalledPage = lazy(() =>
  import("@/components/installed/InstalledPage").then(({ InstalledPage }) => ({
    default: InstalledPage,
  })),
)
const DoctorPage = lazy(() =>
  import("@/components/doctor/DoctorPage").then(({ DoctorPage }) => ({
    default: DoctorPage,
  })),
)
const SettingsPage = lazy(() =>
  import("@/components/settings/SettingsPage").then(({ SettingsPage }) => ({
    default: SettingsPage,
  })),
)
const CreditsPage = lazy(() =>
  import("@/components/credits/CreditsPage").then(({ CreditsPage }) => ({
    default: CreditsPage,
  })),
)

function PageLoading() {
  return (
    <div className="grid min-h-full place-items-center text-xs text-muted-foreground">
      Loading page…
    </div>
  )
}

export function AppShell() {
  const page = useNavStore((state) => state.page)
  const initializeGame = useGameStore((state) => state.initialize)
  const refreshProcessStatus = useGameStore((state) => state.refreshProcessStatus)
  const scrollRef = useRef<HTMLElement | null>(null)
  const conflictSummary = useAssetConflictSummary()

  useEffect(() => {
    void initializeGame()
  }, [initializeGame])

  useEffect(() => {
    let timer: number | null = null
    const stop = () => {
      if (timer !== null) window.clearInterval(timer)
      timer = null
    }
    const syncPolling = () => {
      stop()
      if (document.visibilityState !== "visible") return
      void refreshProcessStatus()
      timer = window.setInterval(() => void refreshProcessStatus(), 2500)
    }
    syncPolling()
    document.addEventListener("visibilitychange", syncPolling)
    return () => {
      stop()
      document.removeEventListener("visibilitychange", syncPolling)
    }
  }, [refreshProcessStatus])

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 })
  }, [page])

  return (
    <div className="flex h-screen overflow-hidden bg-background text-foreground">
      <Sidebar conflictSummary={conflictSummary} />
      <div className="flex min-w-0 flex-1 flex-col">
        <main
          ref={scrollRef}
          data-app-scroll-container
          className="min-h-0 min-w-0 flex-1 overflow-y-auto"
        >
          <Suspense fallback={<PageLoading />}>
            <div key={page} className="page-enter min-h-full">
              {page === "workshop" ? (
                <WorkshopPage scrollRef={scrollRef} />
              ) : page === "installed" ? (
                <InstalledPage />
              ) : page === "doctor" ? (
                <DoctorPage />
              ) : page === "credits" ? (
                <CreditsPage />
              ) : (
                <SettingsPage />
              )}
            </div>
          </Suspense>
        </main>
        <AppStatusBar />
      </div>
    </div>
  )
}
