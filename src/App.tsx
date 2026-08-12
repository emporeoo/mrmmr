import { useEffect } from "react"
import { Tooltip } from "@base-ui/react/tooltip"

import { AppShell } from "@/components/layout/AppShell"
import { AuthScreen } from "@/components/auth/AuthScreen"
import { AppIcon } from "@/components/ui/AppIcon"
import { Toaster } from "@/components/ui/toaster"
import { useAuthStore } from "@/store/auth"

function App() {
  const status = useAuthStore((state) => state.status)
  const initialize = useAuthStore((state) => state.initialize)

  useEffect(() => {
    void initialize()
  }, [initialize])

  return (
    <Tooltip.Provider delay={300} closeDelay={100}>
      {status === "loading" ? (
        <div className="flex min-h-screen items-center justify-center bg-background">
          <div className="page-enter flex flex-col items-center gap-3">
            <AppIcon className="size-10 rounded-sm" />
            <div className="h-0.5 w-16 overflow-hidden bg-muted">
              <div className="h-full w-1/2 animate-pulse bg-primary" />
            </div>
            <span className="text-[10px] font-medium uppercase tracking-[0.14em] text-muted-foreground">
              Loading MRMMR
            </span>
          </div>
        </div>
      ) : status === "authenticated" ? (
        <AppShell />
      ) : (
        <AuthScreen />
      )}
      <Toaster />
    </Tooltip.Provider>
  )
}

export default App
