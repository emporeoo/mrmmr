import { create } from "zustand"

import {
  detectGame,
  ensureModsFolder,
  getGameLocation,
  getGameProcessStatus,
  saveGameLocation,
  type GameLocation,
  type GameProcessStatus,
} from "@/lib/game"
import { getUtocStatus } from "@/lib/utoc"
import { useSetupStore } from "@/store/setup"

export type GameStatus = "loading" | "found" | "not-found"

interface GameStore {
  status: GameStatus
  location: GameLocation | null
  processStatus: GameProcessStatus
  refreshProcessStatus: () => Promise<void>
  initialize: () => Promise<void>
  saveLocation: (path: string) => Promise<void>
}

function prepareModsFolder() {
  void ensureModsFolder().catch(() => {
    // The ~mods folder is best-effort; it is re-created on demand later.
  })
  void getUtocStatus()
    .then((status) => useSetupStore.getState().setUtocInstalled(status.installed))
    .catch(() => {})
}

export const useGameStore = create<GameStore>((set) => ({
  status: "loading",
  location: null,
  processStatus: { game_running: false, shipping_running: false },

  refreshProcessStatus: async () => {
    try {
      set({ processStatus: await getGameProcessStatus() })
    } catch {
      // Process inspection is best-effort in the UI; the backend still guards every mutation.
    }
  },

  initialize: async () => {
    set({ status: "loading" })
    try {
      const saved = await getGameLocation()
      if (saved) {
        set({ status: "found", location: saved })
        prepareModsFolder()
        return
      }

      const detected = await detectGame()
      if (detected) {
        set({ status: "found", location: detected })
        prepareModsFolder()
      } else {
        set({ status: "not-found", location: null })
      }
    } catch {
      set({ status: "not-found", location: null })
    }
  },

  saveLocation: async (path) => {
    const location = await saveGameLocation(path)
    set({ status: "found", location })
    prepareModsFolder()
  },
}))
