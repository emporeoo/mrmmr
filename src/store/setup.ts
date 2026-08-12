import { create } from "zustand"

interface SetupStore {
  utocInstalled: boolean
  setUtocInstalled: (installed: boolean) => void
}

export const useSetupStore = create<SetupStore>((set) => ({
  utocInstalled: false,
  setUtocInstalled: (installed) => set({ utocInstalled: installed }),
}))
