import { useRef, useState } from "react"
import { ExternalLink, Info, Loader2, ShieldCheck, X } from "lucide-react"

import { AppIcon } from "@/components/ui/AppIcon"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card"
import {
  isNexusSsoConfigured,
  NEXUS_SSO_APPLICATION_SLUG,
} from "@/lib/nexusSso"
import { useAuthStore } from "@/store/auth"

export function AuthScreen() {
  const signIn = useAuthStore((state) => state.signIn)
  const [authorizing, setAuthorizing] = useState(false)
  const controllerRef = useRef<AbortController | null>(null)
  const configured = isNexusSsoConfigured()

  async function handleSignIn() {
    if (!configured || authorizing) return
    const controller = new AbortController()
    controllerRef.current = controller
    setAuthorizing(true)
    try {
      await signIn(controller.signal)
    } catch {
      // Authentication errors are shown by the shared notification system.
    } finally {
      setAuthorizing(false)
      controllerRef.current = null
    }
  }

  function handleCancel() {
    controllerRef.current?.abort()
    setAuthorizing(false)
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-5 py-8">
      <main className="page-enter w-full max-w-[420px]">
        <div className="mb-5 flex items-center gap-3">
          <AppIcon className="size-10 rounded-sm" />
          <div>
            <p className="text-base font-bold tracking-[0.08em]">MRMMR</p>
            <p className="text-xs text-muted-foreground">
              Lightweight modern mod manager for Marvel Rivals. Modding made simple.
            </p>
          </div>
        </div>

        <Card className="gap-0 overflow-hidden py-0 shadow-[0_12px_36px_rgba(0,0,0,0.24)]">
          <CardHeader className="gap-2 border-b border-border px-6 py-5">
            <h1 className="text-lg font-semibold tracking-[-0.02em]">
              Connect your Nexus Mods account
            </h1>
            <p className="text-sm leading-relaxed text-muted-foreground">
              MRMMR opens the official Nexus Mods sign-in page in your browser. Sign in there and
              approve access to continue.
            </p>
          </CardHeader>

          <CardContent className="gap-4 px-6 py-5">
            <div className="flex items-start gap-2 rounded-sm border border-border bg-[#191919] p-3">
              <Info className="mt-0.5 size-4 shrink-0 text-primary" />
              <p className="text-xs leading-relaxed text-muted-foreground">
                MRMMR never asks for your Nexus Mods password or requires you to copy a credential
                from your account settings.
              </p>
            </div>

            {!configured ? (
              <div className="rounded-sm border border-primary/40 bg-primary/5 p-3">
                <p className="text-xs font-semibold text-primary">Application registration pending</p>
                <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                  The SSO framework is ready. Nexus Mods must assign the application slug before
                  sign-in can be enabled. Registration placeholder: {NEXUS_SSO_APPLICATION_SLUG}
                </p>
              </div>
            ) : null}
          </CardContent>

          <CardFooter className="flex-col gap-4 border-t border-border bg-[#191919] px-6 py-5">
            <Button
              type="button"
              disabled={!configured || authorizing}
              onClick={() => void handleSignIn()}
              className="h-10 w-full"
            >
              {authorizing ? <Loader2 className="animate-spin" /> : <ExternalLink />}
              {authorizing
                ? "Waiting for Nexus Mods authorization..."
                : configured
                  ? "Continue with Nexus Mods"
                  : "Awaiting Nexus Mods registration"}
            </Button>
            {authorizing ? (
              <Button
                type="button"
                variant="ghost"
                onClick={handleCancel}
                className="h-9 w-full"
              >
                <X />
                Cancel
              </Button>
            ) : null}
            <div className="flex items-start gap-2 text-[11px] leading-relaxed text-muted-foreground">
              <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-primary" />
              <p>
                Nexus Mods issues MRMMR an application-scoped authorization after approval. It is
                encrypted with Windows DPAPI and stored only on this device.
              </p>
            </div>
          </CardFooter>
        </Card>
      </main>
    </div>
  )
}
