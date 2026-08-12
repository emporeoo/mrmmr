import { useCallback, useEffect, useRef, useState } from "react"

import { getAssetConflictSummary, type AssetConflictSummary } from "@/lib/conflicts"

export function useAssetConflictSummary() {
  const [summary, setSummary] = useState<AssetConflictSummary | null>(null)
  const requestId = useRef(0)

  const refresh = useCallback(async () => {
    const currentRequest = ++requestId.current
    try {
      const next = await getAssetConflictSummary()
      if (requestId.current === currentRequest) setSummary(next)
    } catch {
      // Conflict analysis is diagnostic. Local mod management remains available.
      if (requestId.current === currentRequest) setSummary(null)
    }
  }, [])

  useEffect(() => {
    void refresh()
    window.addEventListener("mrmmr-installed-changed", refresh)
    return () => {
      requestId.current += 1
      window.removeEventListener("mrmmr-installed-changed", refresh)
    }
  }, [refresh])

  return summary
}
