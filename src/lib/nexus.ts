import { invoke } from "@tauri-apps/api/core"

export interface NexusUser {
  user_id: number
  name: string
  profile_url?: string | null
  is_premium: boolean
  is_supporter: boolean
  is_admin: boolean
}

export interface AuthSession {
  user: NexusUser
  remembered: boolean
}

export type AccountTier = "free" | "supporter" | "premium"

export function accountTier(user: NexusUser): AccountTier {
  if (user.is_premium) return "premium"
  if (user.is_supporter) return "supporter"
  return "free"
}

export type AuthErrorKind = "empty_api_key" | "invalid_api_key" | "network" | "storage"

export interface AuthError {
  kind: AuthErrorKind
  message?: string
}

export function authenticate(apiKey: string, remember: boolean): Promise<AuthSession> {
  return invoke<AuthSession>("authenticate", { apiKey, remember })
}

export function getAuthSession(): Promise<AuthSession | null> {
  return invoke<AuthSession | null>("get_auth_session")
}

export function refreshAuthSession(): Promise<AuthSession | null> {
  return invoke<AuthSession | null>("refresh_auth_session")
}

export function clearAuth(): Promise<void> {
  return invoke<void>("clear_auth")
}

export function describeAuthError(err: unknown): { title: string; description: string } {
  const e = err as AuthError
  switch (e?.kind) {
    case "invalid_api_key":
      return {
        title: "Failed",
        description: "API key is not accepted. Check it and try again.",
      }
    case "network":
      return {
        title: "Failed",
        description: "Couldn't reach Nexus Mods. Check your connection and try again.",
      }
    case "storage":
      return {
        title: "Failed",
        description: "Couldn't save your API key on this device.",
      }
    default:
      return {
        title: "Failed",
        description: "Something went wrong. Please try again.",
      }
  }
}
