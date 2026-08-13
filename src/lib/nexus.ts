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
}

export type AccountTier = "free" | "supporter" | "premium"

export function accountTier(user: NexusUser): AccountTier {
  if (user.is_premium) return "premium"
  if (user.is_supporter) return "supporter"
  return "free"
}

export type AuthErrorKind =
  | "missing_credential"
  | "invalid_credential"
  | "network"
  | "storage"
  | "sso_not_configured"
  | "sso_connection"
  | "sso_rejected"
  | "sso_timeout"

export interface AuthError {
  kind: AuthErrorKind
  message?: string
}

export function completeSso(credential: string): Promise<AuthSession> {
  return invoke<AuthSession>("complete_sso", { credential })
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
    case "sso_not_configured":
      return {
        title: "Registration pending",
        description: e.message ?? "Nexus Mods has not assigned MRMMR its application slug yet.",
      }
    case "invalid_credential":
      return {
        title: "Authorization expired",
        description: "Nexus Mods rejected this authorization. Sign in again.",
      }
    case "sso_timeout":
      return {
        title: "Sign-in timed out",
        description: e.message ?? "Start the Nexus Mods sign-in again.",
      }
    case "sso_rejected":
      return {
        title: "Sign-in not approved",
        description: e.message ?? "Nexus Mods did not authorize this sign-in.",
      }
    case "sso_connection":
      return {
        title: "Sign-in failed",
        description: e.message ?? "Could not connect to Nexus Mods SSO.",
      }
    case "network":
      return {
        title: "Failed",
        description: e.message ?? "Couldn't reach Nexus Mods. Check your connection and try again.",
      }
    case "storage":
      return {
        title: "Failed",
        description: "Couldn't save the Nexus authorization on this device.",
      }
    default:
      return {
        title: "Failed",
        description: "Something went wrong. Please try again.",
      }
  }
}
