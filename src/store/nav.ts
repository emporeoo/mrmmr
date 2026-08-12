import { create } from "zustand"

export type Page = "workshop" | "installed" | "doctor" | "settings" | "credits"

interface NavStore {
  page: Page
  sidebarOpen: boolean
  setPage: (page: Page) => void
  toggleSidebar: () => void
}

export const useNavStore = create<NavStore>((set) => ({
  page: "workshop",
  sidebarOpen: true,
  setPage: (page) => set({ page }),
  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
}))
