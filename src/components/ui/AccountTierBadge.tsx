import { Crown } from "lucide-react"

import { accountTier, type NexusUser } from "@/lib/nexus"
import { cn } from "@/lib/utils"

interface AccountTierBadgeProps {
  user: NexusUser
  className?: string
}

export function AccountTierBadge({ user, className }: AccountTierBadgeProps) {
  const tier = accountTier(user)
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1 rounded-sm border px-1.5 text-[9px] font-semibold tracking-wide uppercase",
        tier === "premium" &&
          "border-amber-400/50 bg-amber-400/10 text-amber-400",
        tier === "supporter" &&
          "border-sky-400/40 bg-sky-400/10 text-sky-400",
        tier === "free" && "border-border bg-muted text-muted-foreground",
        className,
      )}
    >
      {tier === "premium" ? <Crown className="size-2.5" /> : null}
      {tier}
    </span>
  )
}
