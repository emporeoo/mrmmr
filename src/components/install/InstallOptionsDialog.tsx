import { useEffect, useMemo, useRef, useState } from "react"
import { createPortal } from "react-dom"
import {
  Download,
  FilePlus2,
  Files,
  FileWarning,
  HardDrive,
  Layers3,
  Replace,
  Trash2,
  X,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import type { ModInstallOption } from "@/lib/install"
import { cn } from "@/lib/utils"

const ACTION_META = {
  add: { label: "Add", icon: FilePlus2, className: "text-emerald-400" },
  replace: { label: "Replace", icon: Replace, className: "text-primary" },
  remove: { label: "Remove", icon: Trash2, className: "text-muted-foreground" },
  blocked: { label: "Blocked", icon: FileWarning, className: "text-destructive" },
} as const

function formatBytes(bytes?: number | null): string {
  if (bytes == null) return "Size unavailable"
  if (bytes < 1024) return `${bytes} B`
  const units = ["KB", "MB", "GB"]
  let value = bytes / 1024
  let unit = units[0]
  for (let index = 1; index < units.length && value >= 1024; index++) {
    value /= 1024
    unit = units[index]
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${unit}`
}

export function InstallOptionsDialog({
  modName,
  options,
  premium,
  onConfirm,
  onCancel,
}: {
  modName: string
  options: ModInstallOption[]
  premium: boolean
  onConfirm: (option: ModInstallOption) => void
  onCancel: () => void
}) {
  const defaultId = useMemo(
    () => options.find((option) => option.recommended)?.id ?? options[0]?.id,
    [options],
  )
  const [selectedId, setSelectedId] = useState(defaultId)
  const closeButtonRef = useRef<HTMLButtonElement>(null)
  const dialogRef = useRef<HTMLElement>(null)
  const selected = options.find((option) => option.id === selectedId) ?? options[0]

  useEffect(() => {
    const previousFocus = document.activeElement as HTMLElement | null
    const scrollContainer = document.querySelector<HTMLElement>(
      "[data-app-scroll-container]",
    )
    const previousContainerOverflow = scrollContainer?.style.overflow
    const previousBodyOverflow = document.body.style.overflow
    if (scrollContainer) scrollContainer.style.overflow = "hidden"
    document.body.style.overflow = "hidden"
    closeButtonRef.current?.focus()

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onCancel()
      if (event.key === "Tab") {
        const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])',
        )
        if (!focusable?.length) return
        const first = focusable[0]
        const last = focusable[focusable.length - 1]
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault()
          last.focus()
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault()
          first.focus()
        }
      }
    }
    window.addEventListener("keydown", handleKeyDown)
    return () => {
      window.removeEventListener("keydown", handleKeyDown)
      if (scrollContainer) scrollContainer.style.overflow = previousContainerOverflow ?? ""
      document.body.style.overflow = previousBodyOverflow
      previousFocus?.focus()
    }
  }, [onCancel])

  return createPortal(
    <div
      className="fixed inset-0 z-[70] grid place-items-center bg-black/70 p-5"
      onClick={(event) => event.stopPropagation()}
      onMouseDown={(event) => {
        if (event.currentTarget === event.target) onCancel()
      }}
    >
      <section
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="install-options-title"
        aria-describedby="install-options-description"
        className="flex max-h-[min(760px,calc(100vh-2.5rem))] w-full max-w-2xl animate-[page-enter_180ms_ease-out_both] flex-col overflow-hidden rounded-sm border border-border bg-card shadow-[0_18px_48px_rgba(0,0,0,0.45)]"
      >
        <header className="flex items-start gap-4 border-b border-border px-5 py-4">
          <div className="min-w-0 flex-1">
            <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-primary">
              Nexus download plan
            </p>
            <h2
              id="install-options-title"
              className="mt-1 whitespace-normal break-words text-base font-semibold leading-snug"
            >
              {modName}
            </h2>
            <p
              id="install-options-description"
              className="mt-1 text-xs leading-relaxed text-muted-foreground"
            >
              Choose the Nexus file or detected multi-part group to inspect.
            </p>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            aria-label="Close Nexus download plan"
            onClick={onCancel}
            className="grid size-8 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="border-b border-border bg-[#191919] px-5 py-3 text-[11px] leading-relaxed text-muted-foreground">
          {selected?.content_preview_available
            ? "Nexus has indexed this archive's container files, so predicted additions, replacements, removals, and blocked filenames are shown before download. Internal game-asset conflicts are inspected after staging."
            : "Nexus provides archive metadata, but indexed container contents are unavailable for this choice. Exact container changes and game-asset conflicts appear after temporary staging."}
          {" "}Nothing is installed until you approve the final installation preview.
        </div>

        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
          {options.map((option) => {
            const active = option.id === selected?.id
            return (
              <button
                key={option.id}
                type="button"
                role="radio"
                aria-checked={active}
                onClick={() => setSelectedId(option.id)}
                className={cn(
                  "w-full rounded-sm border p-3 text-left transition-colors",
                  active
                    ? "border-primary/60 bg-primary/8"
                    : "border-border bg-background/40 hover:border-[#4b4b4b] hover:bg-muted/40",
                )}
              >
                <div className="flex flex-wrap items-start gap-3">
                  <span
                    className={cn(
                      "mt-0.5 grid size-8 shrink-0 place-items-center rounded-sm border",
                      active
                        ? "border-primary/40 bg-primary/10 text-primary"
                        : "border-border bg-muted text-muted-foreground",
                    )}
                  >
                    {option.multipart ? <Layers3 className="size-4" /> : <Files className="size-4" />}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="break-words text-sm font-semibold">{option.label}</span>
                      {option.recommended ? (
                        <span className="rounded-sm border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide text-primary">
                          Recommended
                        </span>
                      ) : null}
                    </div>
                    <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-muted-foreground">
                      <span>{option.files.length} archive{option.files.length === 1 ? "" : "s"}</span>
                      <span className="flex items-center gap-1">
                        <HardDrive className="size-3" />
                        {formatBytes(option.total_size_bytes)}
                      </span>
                    </p>
                    {active && option.content_preview_available ? (
                      <p className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[10px]">
                        <span className="text-emerald-400">{option.predicted_adds} added</span>
                        <span className="text-primary">{option.predicted_replaces} replaced</span>
                        <span className="text-muted-foreground">{option.predicted_removes} removed</span>
                        <span className={option.predicted_blocked_files ? "text-destructive" : "text-muted-foreground"}>
                          {option.predicted_blocked_files} blocked
                        </span>
                      </p>
                    ) : null}
                    <div className="mt-2 space-y-1 border-t border-border/70 pt-2">
                      {option.files.map((file) => (
                        <div key={file.file_id}>
                          <p className="break-all text-[10px] leading-relaxed text-muted-foreground">
                            {file.part_number ? `Part ${file.part_number}: ` : ""}
                            {file.file_name}
                            {file.version ? ` | v${file.version}` : ""}
                          </p>
                          {active && option.content_preview_available ? (
                            <div className="mt-1 space-y-0.5 border-l border-border pl-2">
                              {file.contents.map((content, index) => {
                                const meta = ACTION_META[content.action]
                                const Icon = meta.icon
                                return (
                                  <div
                                    key={`${content.path}-${index}`}
                                    className="flex min-w-0 items-start gap-1.5 py-0.5"
                                  >
                                    <Icon className={cn("mt-0.5 size-3 shrink-0", meta.className)} />
                                    <span className="min-w-0 flex-1 break-all text-[10px] leading-relaxed text-foreground/75">
                                      {content.path || content.file_name}
                                      {content.owner_name ? ` | owned by ${content.owner_name}` : ""}
                                    </span>
                                    <span className={cn("shrink-0 text-[9px] font-semibold uppercase", meta.className)}>
                                      {meta.label}
                                    </span>
                                  </div>
                                )
                              })}
                            </div>
                          ) : null}
                        </div>
                      ))}
                      {active && option.predicted_removed_files.length > 0 ? (
                        <div className="mt-1 space-y-0.5 border-l border-border pl-2">
                          {option.predicted_removed_files.map((file) => (
                            <div key={file} className="flex items-start gap-1.5 py-0.5">
                              <Trash2 className="mt-0.5 size-3 shrink-0 text-muted-foreground" />
                              <span className="min-w-0 flex-1 break-all text-[10px] text-foreground/75">{file}</span>
                              <span className="shrink-0 text-[9px] font-semibold uppercase text-muted-foreground">Remove</span>
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  </div>
                </div>
              </button>
            )
          })}
        </div>

        <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-border px-5 py-4">
          <p className="text-[11px] text-muted-foreground">
            {selected?.multipart ? "Detected numbered parts will be managed as one mod." : "One archive selected."}
          </p>
          <div className="flex gap-2">
            <Button variant="outline" onClick={onCancel}>Cancel</Button>
            <Button onClick={() => selected && onConfirm(selected)} disabled={!selected}>
              <Download />
              {premium ? "Download and inspect" : "Continue to Nexus"}
            </Button>
          </div>
        </footer>
      </section>
    </div>,
    document.body,
  )
}
