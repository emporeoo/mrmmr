import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import {
  Box,
  CalendarDays,
  Check,
  Compass,
  Download,
  ExternalLink,
  FolderOpen,
  Loader2,
  Search,
  Trash2,
  UserRound,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { AccountTierBadge } from "@/components/ui/AccountTierBadge"
import { EmptyState } from "@/components/ui/EmptyState"
import { PageHeader } from "@/components/layout/PageHeader"
import { InstallPreviewDialog } from "@/components/install/InstallPreviewDialog"
import { InstallOptionsDialog } from "@/components/install/InstallOptionsDialog"
import { Skeleton } from "@/components/ui/Skeleton"
import { useModInstall } from "@/hooks/useModInstall"
import { getInstalledMods } from "@/lib/install"
import { cn } from "@/lib/utils"
import { progressLabel } from "@/lib/utoc"
import { useAuthStore } from "@/store/auth"
import {
  browseMods,
  formatCount,
  formatDate,
  getModCategories,
  type ModCategory,
  type ModSummary,
} from "@/lib/workshop"

const PER_PAGE = 24

type Sort = "newest" | "popular"

const SORTS: { value: Sort; label: string }[] = [
  { value: "newest", label: "Newest" },
  { value: "popular", label: "Popular" },
]

interface WorkshopPageProps {
  scrollRef?: RefObject<HTMLElement | null>
}

export function WorkshopPage({ scrollRef }: WorkshopPageProps) {
  const user = useAuthStore((state) => state.session?.user)
  const [sort, setSort] = useState<Sort>("newest")
  const [searchInput, setSearchInput] = useState("")
  const [query, setQuery] = useState("")
  const [categoryId, setCategoryId] = useState<number | null>(null)
  const [categories, setCategories] = useState<ModCategory[]>([])

  const [mods, setMods] = useState<ModSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [pageError, setPageError] = useState<string | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const [nextOffset, setNextOffset] = useState(0)
  const [retry, setRetry] = useState(0)
  const [installedIds, setInstalledIds] = useState<Set<number>>(new Set())

  const sentinelRef = useRef<HTMLDivElement | null>(null)
  const requestGeneration = useRef(0)
  const loadingMoreRef = useRef(false)

  useEffect(() => {
    const refreshInstalled = () => {
      getInstalledMods()
        .then((list) => setInstalledIds(new Set(list.map((m) => m.mod_id))))
        .catch(() => {})
    }
    refreshInstalled()
    window.addEventListener("mrmmr-installed-changed", refreshInstalled)
    return () => window.removeEventListener("mrmmr-installed-changed", refreshInstalled)
  }, [])

  useEffect(() => {
    getModCategories()
      .then(setCategories)
      .catch(() => {
        // Categories are optional; browsing still works without them.
      })
  }, [])

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setQuery(searchInput.trim())
    }, 350)
    return () => window.clearTimeout(timer)
  }, [searchInput])

  const categoryNames = useMemo(() => {
    if (categoryId === null) return []
    const category = categories.find((candidate) => candidate.id === categoryId)
    return category ? [category.name] : []
  }, [categoryId, categories])

  useEffect(() => {
    let cancelled = false
    const generation = ++requestGeneration.current
    loadingMoreRef.current = false
    setLoading(true)
    setLoadingMore(false)
    setError(null)
    setPageError(null)
    setMods([])
    setHasMore(false)
    setNextOffset(0)

    browseMods({
      sort,
      query,
      categoryNames,
      offset: 0,
      count: PER_PAGE,
    })
      .then((page) => {
        if (cancelled || requestGeneration.current !== generation) return
        setMods(page.mods)
        setNextOffset(page.nextOffset)
        setHasMore(page.hasMore)
      })
      .catch((err) => {
        if (cancelled || requestGeneration.current !== generation) return
        setError((err as { message?: string })?.message ?? "Couldn't load mods.")
      })
      .finally(() => {
        if (!cancelled && requestGeneration.current === generation) setLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [sort, query, categoryNames, retry])

  const loadMore = useCallback(async () => {
    if (loading || !hasMore || loadingMoreRef.current) return
    const generation = requestGeneration.current
    loadingMoreRef.current = true
    setLoadingMore(true)
    setPageError(null)
    try {
      const page = await browseMods({
        sort,
        query,
        categoryNames,
        offset: nextOffset,
        count: PER_PAGE,
      })
      if (requestGeneration.current !== generation) return
      setMods((current) => {
        const seen = new Set(current.map((mod) => mod.id))
        return [...current, ...page.mods.filter((mod) => !seen.has(mod.id))]
      })
      setNextOffset(page.nextOffset)
      setHasMore(page.hasMore)
    } catch (err) {
      if (requestGeneration.current !== generation) return
      setPageError((err as { message?: string })?.message ?? "Couldn't load more mods.")
    } finally {
      if (requestGeneration.current === generation) {
        loadingMoreRef.current = false
        setLoadingMore(false)
      }
    }
  }, [loading, hasMore, sort, query, categoryNames, nextOffset])

  useEffect(() => {
    if (loading || loadingMore || !hasMore || pageError) return
    const el = sentinelRef.current
    const root = scrollRef?.current
    if (!el || !root) return

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting) void loadMore()
      },
      { root, rootMargin: "300px" },
    )
    observer.observe(el)

    // Fallback in case IntersectionObserver doesn't fire: check position on scroll.
    const check = () => {
      const rect = el.getBoundingClientRect()
      const rootRect = root.getBoundingClientRect()
      if (rect.top - rootRect.bottom < 400) void loadMore()
    }
    root.addEventListener("scroll", check, { passive: true })
    check()

    return () => {
      observer.disconnect()
      root.removeEventListener("scroll", check)
    }
  }, [loading, loadingMore, hasMore, pageError, loadMore, scrollRef])

  const openModPage = useCallback((modId: number) => {
    void openUrl(`https://www.nexusmods.com/marvelrivals/mods/${modId}`)
  }, [])

  return (
    <div className="flex min-h-full flex-col">
      <PageHeader
        icon={<Compass className="size-4 text-primary" />}
        title="Workshop"
        description="Browse Marvel Rivals mods from Nexus Mods."
        trailing={
          user ? (
            <div className="hidden max-w-sm items-center gap-2 border-l border-border pl-4 lg:flex">
              <AccountTierBadge user={user} />
              <div className="flex flex-col gap-0.5">
                <span className="text-[11px] font-medium">
                  {user.is_premium ? "Direct installation enabled" : "Browser download required"}
                </span>
                <span className="text-muted-foreground max-w-64 text-[10px] leading-snug">
                  {user.is_premium
                    ? "MRMMR downloads and installs Nexus files automatically."
                    : "Download from Nexus in your browser; MRMMR detects and installs the archive."}
                </span>
              </div>
            </div>
          ) : null
        }
      />

      <div className="mx-auto flex w-full max-w-[1400px] flex-1 flex-col px-6 py-5">
        <div className="sticky top-0 z-10 -mx-1 mb-5 border border-border bg-[#191919] p-2 shadow-[0_4px_12px_rgba(0,0,0,0.16)]">
          <div className="flex flex-wrap items-center gap-2">
            <label className="relative min-w-56 flex-1">
              <span className="sr-only">Search Nexus Mods</span>
              <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <input
                type="search"
                value={searchInput}
                onChange={(event) => setSearchInput(event.target.value)}
                placeholder="Search all Marvel Rivals mods…"
                spellCheck={false}
                className="h-9 w-full rounded-sm border border-input bg-[#151515] pr-3 pl-9 text-sm placeholder:text-muted-foreground"
              />
            </label>

            <div className="flex h-9 items-center rounded-sm border border-border bg-[#151515] p-0.5">
              {SORTS.map(({ value, label }) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => setSort(value)}
                  className={cn(
                    "h-7 rounded-sm px-3 text-xs font-medium transition-colors",
                    sort === value
                      ? "bg-muted text-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {label}
                </button>
              ))}
            </div>

            <select
              aria-label="Filter by category"
              value={categoryId ?? ""}
              onChange={(event) =>
                setCategoryId(event.target.value ? Number(event.target.value) : null)
              }
              className="h-9 min-w-44 rounded-sm border border-input bg-[#151515] px-2.5 text-sm"
            >
              <option value="">All categories</option>
              {categories.map((category) => (
                <option key={category.id} value={category.id}>
                  {category.name}
                </option>
              ))}
            </select>
          </div>
          <div className="mt-2 flex items-center justify-between border-t border-border px-1 pt-2 text-[11px] text-muted-foreground">
            <span aria-live="polite">
              {loading
                ? "Loading Nexus Mods catalogue…"
                : query || categoryId !== null
                  ? `${mods.length} matching mod${mods.length === 1 ? "" : "s"}`
                  : `${mods.length} mod${mods.length === 1 ? "" : "s"} loaded`}
            </span>
            {query ? <span>Search: “{query}”</span> : <span>Powered by Nexus Mods</span>}
          </div>
        </div>

      {loading ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-4">
          {Array.from({ length: 8 }, (_, index) => (
            <div key={index} className="overflow-hidden rounded-sm border border-border bg-card">
              <Skeleton className="aspect-video w-full rounded-none" />
              <div className="space-y-3 p-3">
                <Skeleton className="h-4 w-3/4" />
                <Skeleton className="h-3 w-full" />
                <Skeleton className="h-8 w-full" />
              </div>
            </div>
          ))}
        </div>
      ) : error ? (
        <EmptyState
          icon={<Box className="size-5" />}
          title="Couldn't load the workshop"
          description={error}
          action={
            <Button variant="outline" size="sm" onClick={() => setRetry((value) => value + 1)}>
              Try again
            </Button>
          }
        />
      ) : mods.length === 0 ? (
        <EmptyState
          icon={<Search className="size-5" />}
          title="No matching mods"
          description="Try a broader search or choose a different category."
        />
      ) : (
        <>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-4">
            {mods.map((mod) => (
              <ModCard
                key={mod.id}
                mod={mod}
                installed={installedIds.has(mod.id)}
                onOpen={() => openModPage(mod.id)}
              />
            ))}
          </div>
          {hasMore && !pageError ? (
            <div ref={sentinelRef} className="flex justify-center py-6 text-muted-foreground">
              {loadingMore ? (
                <span className="flex items-center gap-2 text-xs">
                  <Loader2 className="size-3.5 animate-spin" />
                  Loading more mods…
                </span>
              ) : null}
            </div>
          ) : null}
          {pageError ? (
            <div className="flex flex-col items-center gap-2 py-3 text-center">
              <p className="text-muted-foreground text-xs">{pageError}</p>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setPageError(null)
                  void loadMore()
                }}
              >
                Try again
              </Button>
            </div>
          ) : null}
        </>
      )}
      </div>
    </div>
  )
}

function ModCard({
  mod,
  installed,
  onOpen,
}: {
  mod: ModSummary
  installed: boolean
  onOpen: () => void
}) {
  const [imageFailed, setImageFailed] = useState(false)
  const {
    phase,
    step,
    preview,
    installOptions,
    chosenOption,
    browsing,
    gameRunning,
    uninstalling,
    uninstallConfirm,
    runInstall,
    confirmInstallOption,
    cancelInstallOptions,
    browse,
    confirmInstall,
    cancelPreview,
    cancelWaiting,
    runUninstall,
  } = useModInstall(mod.id, mod.name, installed)
  const premium = useAuthStore.getState().session?.user.is_premium ?? false

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === "Enter") onOpen()
      }}
      className="group flex cursor-pointer flex-col overflow-hidden rounded-sm border border-border bg-card transition-[border-color,transform,box-shadow] duration-150 hover:-translate-y-px hover:border-[#484848] hover:shadow-[0_8px_20px_rgba(0,0,0,0.2)]"
    >
      <div className="relative aspect-video overflow-hidden bg-muted">
        {mod.picture_url && !imageFailed ? (
          <img
            src={mod.picture_url}
            alt={mod.name}
            loading="lazy"
            onError={() => setImageFailed(true)}
            className="h-full w-full object-cover transition-transform duration-200 group-hover:scale-[1.025]"
          />
        ) : (
          <div className="grid h-full w-full place-items-center">
            <Box className="text-muted-foreground/40 size-8" />
          </div>
        )}
        <span className="absolute top-2 right-2 grid size-7 place-items-center rounded-sm border border-white/10 bg-black/75 text-white opacity-0 transition-opacity group-hover:opacity-100">
          <ExternalLink className="size-3.5" />
        </span>
      </div>

      <div className="flex flex-1 flex-col p-3">
        <div className="mb-2 flex items-start justify-between gap-2">
          <h3 className="line-clamp-2 text-sm font-semibold leading-snug">{mod.name}</h3>
          {installed ? (
            <span className="shrink-0 rounded-sm border border-emerald-500/30 bg-emerald-500/10 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide text-emerald-400">
              Installed
            </span>
          ) : null}
        </div>
        <p className="line-clamp-2 min-h-8 text-xs leading-relaxed text-muted-foreground">
          {mod.summary}
        </p>

        <div className="mt-3 flex min-w-0 items-center gap-3 border-t border-border pt-2 text-[10px] text-muted-foreground">
          <span className="flex min-w-0 items-center gap-1">
            <UserRound className="size-3 shrink-0" />
            <span className="truncate">{mod.author || "Unknown author"}</span>
          </span>
          <span className="ml-auto flex shrink-0 items-center gap-1">
            <CalendarDays className="size-3" />
            {formatDate(mod.updated_at)}
          </span>
        </div>
        <div className="mt-1.5 flex items-center justify-between text-[10px] text-muted-foreground">
          <span className="max-w-[65%] truncate">{mod.category_name || "Uncategorized"}</span>
          <span className="flex items-center gap-1">
            <Download className="size-3" />
            {formatCount(mod.downloads)}
          </span>
        </div>

        <div className="mt-3" onClick={(event) => event.stopPropagation()}>
        {phase === "installed" ? (
          <Button
            size="sm"
            variant="outline"
            className="group/installed w-full justify-center"
            onClick={(event) => {
              event.stopPropagation()
              void runUninstall()
            }}
            disabled={uninstalling || gameRunning}
            title={gameRunning ? "Close Marvel Rivals before uninstalling mods." : undefined}
          >
            {uninstallConfirm ? (
              <>
                <Trash2 className="text-destructive" />
                Confirm uninstall
              </>
            ) : (
              <>
                <Check className="group-hover/installed:hidden text-emerald-500" />
                <Trash2 className="text-destructive hidden group-hover/installed:block" />
                <span className="group-hover/installed:hidden text-emerald-500">Installed</span>
                <span className="text-destructive hidden group-hover/installed:block">
                  Uninstall
                </span>
              </>
            )}
          </Button>
        ) : phase === "waiting-download" ? (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="size-3 animate-spin" />
              <span className="text-xs">{progressLabel(step ?? "waiting_for_download")}</span>
            </div>
            <Button size="sm" variant="outline" onClick={browse} disabled={browsing}>
              <FolderOpen />
              {chosenOption?.multipart ? "Choose all downloaded parts" : "Choose downloaded file"}
            </Button>
            <Button size="sm" variant="ghost" onClick={cancelWaiting}>
              Cancel
            </Button>
          </div>
        ) : phase === "installing" || phase === "preparing" ? (
          <Button
            size="sm"
            disabled
            onClick={(event) => event.stopPropagation()}
            className="w-full justify-center"
          >
            <Loader2 className="animate-spin" />
            {progressLabel(step ?? "starting")}
          </Button>
        ) : (
          <Button
            size="sm"
            className="w-full"
            onClick={(event) => {
              event.stopPropagation()
              void runInstall()
            }}
            disabled={gameRunning}
            title={gameRunning ? "Close Marvel Rivals before installing mods." : undefined}
          >
            <Download />
            Install
          </Button>
        )}
        </div>
      </div>
      {preview ? (
        <InstallPreviewDialog
          preview={preview}
          busy={phase === "installing"}
          onConfirm={() => void confirmInstall()}
          onCancel={() => void cancelPreview()}
        />
      ) : null}
      {installOptions ? (
        <InstallOptionsDialog
          modName={mod.name}
          options={installOptions}
          premium={premium}
          onConfirm={(option) => void confirmInstallOption(option)}
          onCancel={cancelInstallOptions}
        />
      ) : null}
    </div>
  )
}
