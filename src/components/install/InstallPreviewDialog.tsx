import { useEffect, useRef } from "react"
import { createPortal } from "react-dom"
import {
  FilePlus2,
  FileWarning,
  HardDrive,
  Loader2,
  Replace,
  ScanSearch,
  ShieldCheck,
  Trash2,
  TriangleAlert,
  X,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import type { InstallPreview, InstallPreviewAction } from "@/lib/install"
import { cn } from "@/lib/utils"

function formatBytes(bytes: number): string {
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

const ACTION_META: Record<
  InstallPreviewAction,
  { label: string; icon: typeof FilePlus2; className: string }
> = {
  add: { label: "Add", icon: FilePlus2, className: "text-emerald-400" },
  replace: { label: "Replace", icon: Replace, className: "text-primary" },
  remove: { label: "Remove", icon: Trash2, className: "text-muted-foreground" },
  blocked: { label: "Blocked", icon: FileWarning, className: "text-destructive" },
}

export function InstallPreviewDialog({
  preview,
  busy,
  onConfirm,
  onCancel,
}: {
  preview: InstallPreview
  busy: boolean
  onConfirm: () => void
  onCancel: () => void
}) {
  const closeButtonRef = useRef<HTMLButtonElement>(null)
  const dialogRef = useRef<HTMLElement>(null)

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !busy) onCancel()
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
    return () => window.removeEventListener("keydown", handleKeyDown)
  }, [busy, onCancel])

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

    return () => {
      if (scrollContainer) {
        scrollContainer.style.overflow = previousContainerOverflow ?? ""
      }
      document.body.style.overflow = previousBodyOverflow
      previousFocus?.focus()
    }
  }, [])

  const assetReport = preview.asset_conflicts
  const visibleAssetConflicts = assetReport.conflicts.slice(0, 12)
  const hiddenAssetConflicts = assetReport.conflicting_asset_count - visibleAssetConflicts.length
  const summary = [
    `${preview.adds} container file${preview.adds === 1 ? "" : "s"} added`,
    `${preview.replaces} replaced`,
    `${preview.removes} removed`,
    `${preview.blocked_files} blocked`,
    `${assetReport.conflicting_asset_count} asset conflict${assetReport.conflicting_asset_count === 1 ? "" : "s"}`,
  ].join(", ")

  return createPortal(
    <div
      className="fixed inset-0 z-[70] grid place-items-center bg-black/70 p-5"
      onClick={(event) => event.stopPropagation()}
      onMouseDown={(event) => {
        if (event.currentTarget === event.target && !busy) onCancel()
      }}
    >
      <section
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="install-preview-title"
        aria-describedby="install-preview-summary"
        className="flex max-h-[min(720px,calc(100vh-2.5rem))] w-full max-w-2xl animate-[page-enter_180ms_ease-out_both] flex-col overflow-hidden rounded-sm border border-border bg-card shadow-[0_18px_48px_rgba(0,0,0,0.45)]"
      >
        <header className="flex items-start gap-4 border-b border-border px-5 py-4">
          <div className="min-w-0 flex-1">
            <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-primary">
              Installation preview
            </p>
            <h2
              id="install-preview-title"
              className="mt-1 whitespace-normal break-words text-base font-semibold leading-snug"
            >
              {preview.mod_name}
            </h2>
            <p id="install-preview-summary" className="mt-1 text-xs text-muted-foreground">
              {summary}
            </p>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            aria-label="Close installation preview"
            onClick={onCancel}
            disabled={busy}
            className="grid size-8 place-items-center rounded-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="grid gap-3 border-b border-border bg-[#191919] px-5 py-4 sm:grid-cols-2">
          <div className="flex items-start gap-2.5">
            <ShieldCheck
              className={cn(
                "mt-0.5 size-4 shrink-0",
                preview.archive_verified ? "text-emerald-400" : "text-primary",
              )}
            />
            <div className="min-w-0">
              <p className="text-xs font-medium">
                {preview.archive_verified
                  ? `${preview.archives.length === 1 ? "Archive" : "Archives"} verified`
                  : "Nexus archive metadata"}
              </p>
              <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                {preview.archive_verified
                  ? `The checksum${preview.archives.length === 1 ? "" : "s"} match Nexus Mods metadata.`
                  : "Nexus did not publish a checksum for every selected file."}
              </p>
            </div>
          </div>
          <div className="flex items-start gap-2.5">
            <HardDrive className="mt-0.5 size-4 shrink-0 text-primary" />
            <div className="min-w-0">
              <p className="text-xs font-medium">
                {formatBytes(preview.required_bytes)} required
              </p>
              <p className="mt-0.5 whitespace-normal break-all text-[11px] leading-relaxed text-muted-foreground">
                {preview.available_bytes == null
                  ? preview.archives.length === 1
                    ? preview.archive_name
                    : `${preview.archives.length} grouped archives`
                  : `${formatBytes(preview.available_bytes)} available | ${
                      preview.archives.length === 1
                        ? preview.archive_name
                        : `${preview.archives.length} grouped archives`
                    }`}
              </p>
            </div>
          </div>
        </div>

        {preview.blocked_files > 0 ? (
          <div className="border-b border-destructive/30 bg-destructive/10 px-5 py-3 text-xs leading-relaxed text-destructive">
            Installation is blocked because {preview.blocked_files} container filename
            {preview.blocked_files === 1 ? " is" : "s are"} already owned by another mod or unmanaged.
            Remove the blocking file, then prepare the mod again.
          </div>
        ) : null}
        {!preview.enough_space ? (
          <div className="border-b border-destructive/30 bg-destructive/10 px-5 py-3 text-xs leading-relaxed text-destructive">
            Installation is blocked because the game drive does not have enough free space.
          </div>
        ) : null}
        {assetReport.conflicting_asset_count > 0 ? (
          <div className="flex items-start gap-2 border-b border-amber-500/30 bg-amber-500/10 px-5 py-3 text-xs leading-relaxed text-amber-300">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" />
            <p>
              This mod changes {assetReport.conflicting_asset_count} game asset
              {assetReport.conflicting_asset_count === 1 ? "" : "s"} also changed by {assetReport.affected_mod_count} enabled mod
              {assetReport.affected_mod_count === 1 ? "" : "s"}. Installation is allowed, but load order determines which version wins.
            </p>
          </div>
        ) : null}
        {assetReport.scan_status !== "complete" ? (
          <div className="flex items-start gap-2 border-b border-border bg-muted/40 px-5 py-3 text-[11px] leading-relaxed text-muted-foreground">
            <ScanSearch className="mt-0.5 size-4 shrink-0" />
            <p>
              Asset scan incomplete. {assetReport.scan_error ?? "Some internal assets could not be inspected."}
            </p>
          </div>
        ) : null}

        <div className="min-h-0 flex-1 overflow-y-auto">
          {assetReport.conflicting_asset_count > 0 ? (
            <section className="border-b border-border bg-amber-500/[0.03] px-5 py-3">
              <p className="text-[10px] font-semibold uppercase tracking-wide text-amber-300">
                Conflicting game assets
              </p>
              <div className="mt-2 space-y-2">
                {visibleAssetConflicts.map((conflict) => (
                  <div key={conflict.asset_path} className="border-l-2 border-amber-500/40 pl-2">
                    <code className="block break-all font-mono text-[10px] leading-relaxed text-foreground">
                      {conflict.asset_path}
                    </code>
                    <p className="mt-0.5 break-words text-[10px] leading-relaxed text-muted-foreground">
                      Also changed by {conflict.other_mods.map((mod) => mod.mod_name).join(", ")}
                    </p>
                  </div>
                ))}
              </div>
              {hiddenAssetConflicts > 0 ? (
                <p className="mt-2 text-[10px] text-muted-foreground">
                  +{hiddenAssetConflicts} more conflicting asset{hiddenAssetConflicts === 1 ? "" : "s"}
                </p>
              ) : null}
            </section>
          ) : null}
          {preview.archives.length > 1 ? (
            <div className="border-b border-border bg-background/30 px-5 py-3">
              <p className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                Grouped Nexus archives
              </p>
              <div className="mt-1.5 space-y-1">
                {preview.archives.map((archive, index) => (
                  <p key={`${archive.file_id ?? index}-${archive.file_name}`} className="break-all text-[11px] leading-relaxed text-foreground/80">
                    Part {index + 1}: {archive.file_name}
                  </p>
                ))}
              </div>
            </div>
          ) : null}
          <div className="border-b border-border bg-background/30 px-5 py-2">
            <p className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
              Container file changes · {assetReport.scanned_asset_count} internal asset
              {assetReport.scanned_asset_count === 1 ? "" : "s"} scanned
            </p>
          </div>
          {preview.files.map((file, index) => {
            const meta = ACTION_META[file.action]
            const Icon = meta.icon
            return (
              <div
                key={`${file.action}-${file.name}-${index}`}
                className="flex items-start gap-3 border-b border-border px-5 py-2.5 last:border-b-0"
              >
                <Icon className={cn("size-4 shrink-0", meta.className)} />
                <div className="min-w-0 flex-1">
                  <p className="break-all text-xs font-medium leading-relaxed">{file.name}</p>
                  {file.owner_name ? (
                    <p className="mt-0.5 break-words text-[10px] leading-relaxed text-muted-foreground">
                      Owned by {file.owner_name}
                      {file.owner_mod_id ? ` | Nexus mod ${file.owner_mod_id}` : ""}
                    </p>
                  ) : null}
                </div>
                <span className={cn("text-[10px] font-semibold uppercase tracking-wide", meta.className)}>
                  {meta.label}
                </span>
                <span className="w-14 text-right text-[10px] text-muted-foreground">
                  {formatBytes(file.size_bytes)}
                </span>
              </div>
            )
          })}
        </div>

        <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-border px-5 py-4">
          <p className="text-[11px] text-muted-foreground">
            Nothing has been changed in your game folder yet.
          </p>
          <div className="flex gap-2">
            <Button variant="outline" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={onConfirm} disabled={busy || !preview.can_install}>
              {busy ? <Loader2 className="animate-spin" /> : null}
              {busy ? "Installing..." : preview.replaces > 0 ? "Apply update" : "Install mod"}
            </Button>
          </div>
        </footer>
      </section>
    </div>,
    document.body,
  )
}
