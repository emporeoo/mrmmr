import { useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { ExternalLink, Eye, EyeOff, Info, KeyRound, Loader2, ShieldCheck } from "lucide-react"

import { AppIcon } from "@/components/ui/AppIcon"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardFooter, CardHeader } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useAuthStore } from "@/store/auth"

const NEXUS_API_KEYS_URL = "https://next.nexusmods.com/settings/api-keys"

export function AuthScreen() {
  const signIn = useAuthStore((state) => state.signIn)
  const [apiKey, setApiKey] = useState("")
  const [remember, setRemember] = useState(true)
  const [showKey, setShowKey] = useState(false)
  const [validating, setValidating] = useState(false)
  const canContinue = apiKey.trim().length > 0 && !validating

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!canContinue) return
    setValidating(true)
    try {
      await signIn(apiKey.trim(), remember)
    } catch {
      // Authentication errors are shown by the shared notification system.
    } finally {
      setValidating(false)
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-5 py-8">
      <main className="page-enter w-full max-w-[420px]">
        <div className="mb-5 flex items-center gap-3">
          <AppIcon className="size-10 rounded-sm" />
          <div>
            <p className="text-base font-bold tracking-[0.08em]">MRMMR</p>
            <p className="text-xs text-muted-foreground">Lightweight modern mod manager for Marvel Rivals - Modding made simple.</p>
          </div>
        </div>

        <form onSubmit={handleSubmit}>
          <Card className="gap-0 overflow-hidden py-0 shadow-[0_12px_36px_rgba(0,0,0,0.24)]">
            <CardHeader className="gap-2 border-b border-border px-6 py-5">
              {/* <div className="flex items-center gap-2 text-primary">
                <KeyRound className="size-4" />
                <span className="text-xs font-semibold uppercase tracking-[0.1em]">Nexus Mods</span>
              </div> */}
              <div className="flex items-center gap-2">
                <KeyRound className="size-4" />
                <h1 className="text-lg font-semibold tracking-[-0.02em]">Connect your account</h1>
                {/* <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                  Use your personal API key to browse and install Marvel Rivals mods.
                </p> */}
              </div>
            </CardHeader>

            <CardContent className="gap-5 px-6 py-5">
              <div className="flex flex-col gap-2">
                <Label htmlFor="api-key">Personal API key</Label>
                <div className="relative">
                  <Input
                    id="api-key"
                    type={showKey ? "text" : "password"}
                    value={apiKey}
                    onChange={(event) => setApiKey(event.target.value)}
                    placeholder="Paste your Nexus Mods API key"
                    autoFocus
                    autoComplete="off"
                    spellCheck={false}
                    className="h-10 pr-10"
                  />
                  <button
                    type="button"
                    aria-label={showKey ? "Hide API key" : "Show API key"}
                    title={showKey ? "Hide API key" : "Show API key"}
                    onClick={() => setShowKey((value) => !value)}
                    className="absolute top-1/2 right-2 grid size-8 -translate-y-1/2 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  >
                    {showKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                  </button>
                </div>
                <button
                  type="button"
                  onClick={() => openUrl(NEXUS_API_KEYS_URL)}
                  className="inline-flex items-center gap-1.5 self-start rounded-sm text-xs font-medium text-primary hover:underline"
                >
                  Open Nexus Mods API keys
                  <ExternalLink className="size-3.5" />
                </button>
                <p className="flex items-start gap-1.5 text-[11px] leading-relaxed text-muted-foreground">
                  <Info className="mt-0.5 size-3.5 shrink-0 text-primary" />
                  On that page, scroll to the bottom and copy the key under Personal API Key.
                </p>
              </div>

              <label className="flex cursor-pointer items-center gap-2.5">
                <Checkbox id="remember-key" checked={remember} onCheckedChange={setRemember} />
                <span className="text-sm">Remember this account on this device</span>
              </label>
            </CardContent>

            <CardFooter className="flex-col gap-4 border-t border-border bg-[#191919] px-6 py-5">
              <Button type="submit" disabled={!canContinue} className="h-10 w-full">
                {validating ? <Loader2 className="animate-spin" /> : null}
                {validating ? "Verifying key…" : "Continue"}
              </Button>
              <div className="flex items-start gap-2 text-[11px] leading-relaxed text-muted-foreground">
                <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-primary" />
                <p>
                  Your key is encrypted with Windows and is only sent to the Nexus Mods API.
                </p>
              </div>
            </CardFooter>
          </Card>
        </form>
      </main>
    </div>
  )
}
