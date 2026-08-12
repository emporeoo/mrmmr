import { create } from "zustand"

import { toast } from "@/lib/toast"
import {
  authenticate,
  clearAuth,
  describeAuthError,
  getAuthSession,
  refreshAuthSession,
  type AuthSession,
} from "@/lib/nexus"

export type AuthStatus = "loading" | "unauthenticated" | "authenticated"

interface AuthStore {
  status: AuthStatus
  session: AuthSession | null
  initialize: () => Promise<void>
  signIn: (apiKey: string, remember: boolean) => Promise<void>
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
          toast.error("Failed", "Your API key is no longer valid. Sign in again.")
        }
      } catch {
        // Network error while revalidating — keep the stored session.
      }
    } catch {
      set({ status: "unauthenticated", session: null })
    }
  },

  signIn: async (apiKey, remember) => {
    try {
      const session = await authenticate(apiKey, remember)
      set({ status: "authenticated", session })
      toast.success("Success", `Authenticated as ${session.user.name}.`)
    } catch (err) {
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
