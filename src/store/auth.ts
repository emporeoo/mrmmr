import { create } from "zustand"

import { toast } from "@/lib/toast"
import {
  clearAuth,
  completeSso,
  describeAuthError,
  getAuthSession,
  refreshAuthSession,
  type AuthSession,
} from "@/lib/nexus"
import { NexusSsoError, requestNexusSsoCredential } from "@/lib/nexusSso"

export type AuthStatus = "loading" | "unauthenticated" | "authenticated"

interface AuthStore {
  status: AuthStatus
  session: AuthSession | null
  initialize: () => Promise<void>
  signIn: (signal?: AbortSignal) => Promise<void>
  signOut: () => Promise<void>
}

export const useAuthStore = create<AuthStore>((set) => ({
  status: "loading",
  session: null,

  initialize: async () => {
    try {
      const session = await getAuthSession()
      if (!session) {
        set({ status: "unauthenticated", session: null })
        return
      }

      set({ status: "authenticated", session })

      try {
        const fresh = await refreshAuthSession()
        if (fresh) {
          set({ session: fresh })
        } else {
          set({ status: "unauthenticated", session: null })
          toast.error("Authorization expired", "Sign in with Nexus Mods again.")
        }
      } catch {
        // Keep the stored session if revalidation fails due to a network error.
      }
    } catch {
      set({ status: "unauthenticated", session: null })
    }
  },

  signIn: async (signal?: AbortSignal) => {
    try {
      const credential = await requestNexusSsoCredential(signal)
      const session = await completeSso(credential)
      set({ status: "authenticated", session })
      toast.success("Success", `Authenticated as ${session.user.name}.`)
    } catch (err) {
      if (err instanceof NexusSsoError && err.kind === "sso_cancelled") throw err
      const { title, description } = describeAuthError(err)
      toast.error(title, description)
      throw err
    }
  },

  signOut: async () => {
    try {
      await clearAuth()
    } catch {
      // Signing out still succeeds from the UI's perspective.
    }
    set({ status: "unauthenticated", session: null })
    toast.info("Signed out")
  },
}))
