import { useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { BadgeInfo, ExternalLink, UserRound } from "lucide-react"

import { PageHeader } from "@/components/layout/PageHeader"
import { Button } from "@/components/ui/button"
import { GithubIcon, KoFiIcon, NexusModsIcon } from "@/components/ui/BrandIcons"
import { toast } from "@/lib/toast"

const NEXUS_PROFILE_URL = "https://www.nexusmods.com/marvelrivals/mods/11829"
const NEXUS_AVATAR_URL = "https://avatars.nexusmods.com/60677496/100"
const KOFI_URL = "https://ko-fi.com/emporeo"
const GITHUB_REPO_URL = "https://github.com/emporeoo"

async function openExternal(url: string) {
  try {
    await openUrl(url)
  } catch {
    toast.error("Couldn't open link", "Open the link in your browser and try again.")
  }
}

export function CreditsPage() {
  const [avatarFailed, setAvatarFailed] = useState(false)

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        icon={<BadgeInfo className="size-4" />}
        title="Credits"
        description="The people behind Marvel Rivals Mod Manager Redux."
      />

      <div className="mx-auto w-full max-w-5xl flex-1 px-6 py-5">
        <section className="overflow-hidden rounded-sm border border-border bg-card">
          <div className="flex flex-wrap items-center gap-4 p-5">
            <div className="grid size-11 shrink-0 place-items-center overflow-hidden rounded-sm border border-border bg-muted">
              {avatarFailed ? (
                <UserRound className="size-5 text-primary" />
              ) : (
                <img
                  src={NEXUS_AVATAR_URL}
                  alt="emporeo"
                  onError={() => setAvatarFailed(true)}
                  className="size-11 object-cover"
                />
              )}
            </div>
            <div className="min-w-0 flex-1">
              <h2 className="text-sm font-semibold">emporeo</h2>
              <p className="mt-0.5 text-xs text-muted-foreground">Developer</p>
            </div>
            <div className="flex flex-col gap-2">
              <Button variant="outline" onClick={() => void openExternal(KOFI_URL)}>
                <KoFiIcon className="size-4 text-[#ff6433]" />
                Support on Ko-fi
                <ExternalLink className="size-3.5 text-muted-foreground" />
              </Button>

              <Button variant="outline" onClick={() => void openExternal(NEXUS_PROFILE_URL)}>
                <NexusModsIcon className="size-4 text-primary" />
                Nexus Mods page
                <ExternalLink className="size-3.5 text-muted-foreground" />
              </Button>
              
              <Button variant="outline" onClick={() => void openExternal(GITHUB_REPO_URL)}>
                <GithubIcon className="size-4" />
                View on GitHub
                <ExternalLink className="size-3.5 text-muted-foreground" />
              </Button>
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}
