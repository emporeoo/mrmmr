import appIconUrl from "../../../src-tauri/icons/128x128.png"

import { cn } from "@/lib/utils"

export function AppIcon({ className }: { className?: string }) {
  return (
    <img
      src={appIconUrl}
      alt=""
      aria-hidden="true"
      draggable={false}
      className={cn("shrink-0 object-contain", className)}
    />
  )
}
