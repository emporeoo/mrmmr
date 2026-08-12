import { useEffect, useState } from "react"

import { cn } from "@/lib/utils"

interface UserAvatarProps {
  name: string
  imageUrl?: string | null
  className?: string
}

export function UserAvatar({ name, imageUrl, className }: UserAvatarProps) {
  const [attempt, setAttempt] = useState(0)

  const candidates = [imageUrl].filter((url): url is string => Boolean(url))

  useEffect(() => setAttempt(0), [imageUrl])

  const current = candidates[attempt]

  const initials = name
    .split(/\s+/)
    .map((part) => part[0])
    .join("")
    .slice(0, 2)
    .toUpperCase()

  if (current) {
    return (
      <img
        src={current}
        alt={name}
        onError={() => setAttempt((value) => value + 1)}
        className={cn("bg-muted size-8 shrink-0 rounded-md object-cover", className)}
      />
    )
  }

  return (
    <div
      className={cn(
        "bg-muted grid size-8 shrink-0 place-items-center rounded-md text-[11px] font-semibold",
        className,
      )}
    >
      {initials}
    </div>
  )
}
