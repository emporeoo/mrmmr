import { Tooltip } from "@base-ui/react/tooltip"
import { CircleAlert, TriangleAlert } from "lucide-react"

import type { ModAssetConflictReport } from "@/lib/conflicts"
import { cn } from "@/lib/utils"

const VISIBLE_CONFLICTS = 6

export function AssetConflictBadge({ report }: { report: ModAssetConflictReport }) {
  if (report.conflicting_asset_count === 0) return null

  const visible = report.conflicts.slice(0, VISIBLE_CONFLICTS)
  const hidden = report.conflicting_asset_count - visible.length

  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        aria-label={`${report.conflicting_asset_count} conflicting assets. Show details.`}
        className="inline-flex h-5 items-center gap-1 rounded-sm border border-destructive/45 bg-destructive/10 px-1.5 text-[10px] font-semibold text-destructive transition-colors hover:border-destructive/70 hover:bg-destructive/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <TriangleAlert className="size-3" />
        {report.conflicting_asset_count} conflict{report.conflicting_asset_count === 1 ? "" : "s"}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side="top" sideOffset={8} collisionPadding={12} className="z-[100]">
          <Tooltip.Popup className="w-[min(28rem,calc(100vw-2rem))] rounded-sm border border-border bg-popover p-3 text-popover-foreground shadow-xl outline-none data-[ending-style]:opacity-0 data-[starting-style]:opacity-0 data-[ending-style]:scale-95 data-[starting-style]:scale-95 transition-[opacity,transform] duration-150">
            <div className="mb-2 flex items-start gap-2 border-b border-border pb-2">
              <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
              <div>
                <p className="text-xs font-semibold">Asset-level conflicts</p>
                <p className="mt-0.5 text-[11px] leading-4 text-muted-foreground">
                  These enabled mods replace the same Marvel Rivals assets. The game decides which file wins by load order.
                </p>
              </div>
            </div>
            <div className="max-h-64 space-y-2 overflow-y-auto pr-1">
              {visible.map((conflict) => (
                <div key={conflict.asset_path} className="border-l-2 border-destructive/45 pl-2">
                  <code className="block break-all font-mono text-[10px] leading-4 text-foreground">
                    {conflict.asset_path}
                  </code>
                  <p className="mt-0.5 text-[10px] leading-4 text-muted-foreground">
                    Also provided by {conflict.other_mods.map((mod) => mod.mod_name).join(", ")}
                  </p>
                </div>
              ))}
            </div>
            {hidden > 0 ? (
              <p className="mt-2 border-t border-border pt-2 text-[10px] text-muted-foreground">
                +{hidden} more conflicting asset{hidden === 1 ? "" : "s"}
              </p>
            ) : null}
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function AssetScanIssueBadge({ report }: { report: ModAssetConflictReport }) {
  if (report.scan_status === "complete") return null

  const failed = report.scan_status === "failed"
  const label = report.scan_status === "pending" ? "Scan pending" : "Scan incomplete"

  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        aria-label={`${label}. Show details.`}
        className={cn(
          "inline-flex h-5 items-center gap-1 rounded-sm border px-1.5 text-[10px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          failed
            ? "border-amber-500/45 bg-amber-500/10 text-amber-400 hover:bg-amber-500/15"
            : "border-border bg-muted text-muted-foreground hover:text-foreground",
        )}
      >
        <CircleAlert className="size-3" />
        {label}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side="top" sideOffset={8} collisionPadding={12} className="z-[100]">
          <Tooltip.Popup className="w-[min(24rem,calc(100vw-2rem))] rounded-sm border border-border bg-popover p-3 text-[11px] leading-4 text-popover-foreground shadow-xl outline-none data-[ending-style]:opacity-0 data-[starting-style]:opacity-0 transition-opacity duration-150">
            <p className="font-semibold">Conflict scan is incomplete</p>
            <p className="mt-1 text-muted-foreground">
              {report.scan_error ?? "This mod has not been indexed yet. Refresh the Installed page to retry."}
            </p>
            <p className="mt-1 text-muted-foreground">Existing conflict results may not include every asset in this mod.</p>
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
