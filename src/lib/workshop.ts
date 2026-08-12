import { invoke } from "@tauri-apps/api/core"

export interface ModSummary {
  id: number
  name: string
  summary: string
  picture_url: string
  downloads: number
  endorsements: number
  category_id: number
  category_name: string
  author: string
  updated_at: string
}

export interface ModCategory {
  id: number
  name: string
  parent: number | null
}

export type BrowseList = "newest" | "popular"

export interface BrowseRequest {
  sort: BrowseList
  query: string
  categoryNames: string[]
  offset: number
  count: number
}

export interface BrowsePage {
  mods: ModSummary[]
  nodesCount: number
  totalCount: number
  nextOffset: number
  hasMore: boolean
}

export function browseMods(request: BrowseRequest): Promise<BrowsePage> {
  return invoke<BrowsePage>("browse_mods", { request })
}

export function getModCategories(): Promise<ModCategory[]> {
  return invoke<ModCategory[]>("get_mod_categories")
}

export function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`
  return String(n)
}

export function formatDate(value: string | number): string {
  if (!value) return "—"
  const date = typeof value === "number" ? new Date(value * 1000) : new Date(value)
  if (Number.isNaN(date.getTime())) return "—"
  const now = new Date()
  const days = Math.floor((now.getTime() - date.getTime()) / 86_400_000)
  if (days <= 0) return "Today"
  if (days === 1) return "Yesterday"
  if (days < 30) return `${days} days ago`
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })
}
