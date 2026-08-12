import { useCallback, useEffect, useRef } from "react"
import { AlertCircle, CheckCircle2, Info, TriangleAlert, X } from "lucide-react"

import { cn } from "@/lib/utils"
import { useToastStore, type ToastItem } from "@/lib/toast"

const ICONS: Record<ToastItem["variant"], typeof Info> = {
  success: CheckCircle2,
  error: AlertCircle,
  warning: TriangleAlert,
  info: Info,
}

const ACCENTS: Record<ToastItem["variant"], string> = {
  success: "border-l-emerald-500",
  error: "border-l-destructive",
  warning: "border-l-primary",
  info: "border-l-[#707070]",
}

const ICON_CLASS: Record<ToastItem["variant"], string> = {
  success: "text-emerald-500",
  error: "text-destructive",
  warning: "text-primary",
  info: "text-muted-foreground",
}

function ToastCard({ item, onDismiss }: { item: ToastItem; onDismiss: () => void }) {
  const Icon = ICONS[item.variant]
  const remaining = useRef(item.variant === "error" ? 8000 : 4500)
  const startedAt = useRef(0)
  const timeout = useRef<number | null>(null)

  const resume = useCallback(() => {
    if (item.leaving || timeout.current !== null) return
    startedAt.current = Date.now()
    timeout.current = window.setTimeout(onDismiss, remaining.current)
  }, [item.leaving, onDismiss])

  const pause = useCallback(() => {
    if (timeout.current === null) return
    window.clearTimeout(timeout.current)
    timeout.current = null
    remaining.current = Math.max(0, remaining.current - (Date.now() - startedAt.current))
  }, [])

  useEffect(() => {
    resume()
    return pause
  }, [resume, pause])

  return (
    <div
      role={item.variant === "error" ? "alert" : "status"}
      onMouseEnter={pause}
      onMouseLeave={resume}
      onFocusCapture={pause}
      onBlurCapture={resume}
      className={cn(
        "flex items-start gap-3 rounded-sm border border-l-[3px] border-border bg-[#202020] px-3 py-3 shadow-[0_8px_24px_rgba(0,0,0,0.28)]",
        ACCENTS[item.variant],
        item.leaving ? "toast-exit" : "toast-enter",
      )}
    >
      <Icon className={cn("mt-0.5 size-4 shrink-0", ICON_CLASS[item.variant])} />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-semibold leading-tight">{item.title}</p>
        {item.description ? (
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{item.description}</p>
        ) : null}
      </div>
      <button
        type="button"
        aria-label="Dismiss notification"
        onClick={onDismiss}
        className="-m-1 grid size-7 shrink-0 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
      >
        <X className="size-3.5" />
      </button>
    </div>
  )
}

export function Toaster() {
  const toasts = useToastStore((state) => state.toasts)
  const dismiss = useToastStore((state) => state.dismiss)

  return (
    <div
      aria-label="Notifications"
      className="pointer-events-none fixed right-4 bottom-10 z-50 flex w-[min(360px,calc(100vw-2rem))] flex-col gap-2"
    >
      {toasts.map((item) => (
        <div key={item.id} className="pointer-events-auto">
          <ToastCard item={item} onDismiss={() => dismiss(item.id)} />
        </div>
      ))}
    </div>
  )
}
