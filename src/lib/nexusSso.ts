import { openUrl } from "@tauri-apps/plugin-opener"

const NEXUS_SSO_SOCKET_URL = "wss://sso.nexusmods.com"
const NEXUS_SSO_AUTHORIZE_URL = "https://www.nexusmods.com/sso"
const SSO_TIMEOUT_MS = 5 * 60 * 1000

// Nexus Mods will replace this registration placeholder with MRMMR's assigned
// application slug. Do not borrow another application's slug.
export const NEXUS_SSO_APPLICATION_SLUG = "PENDING_NEXUS_APPLICATION_SLUG"

type NexusSsoErrorKind =
  | "sso_not_configured"
  | "sso_connection"
  | "sso_rejected"
  | "sso_timeout"

export class NexusSsoError extends Error {
  readonly kind: NexusSsoErrorKind

  constructor(kind: NexusSsoErrorKind, message: string) {
    super(message)
    this.name = "NexusSsoError"
    this.kind = kind
  }
}

interface SsoResponse {
  success: boolean
  data?: {
    connection_token?: string
    api_key?: string
  }
  error?: string | null
}

export function isNexusSsoConfigured(): boolean {
  return (
    NEXUS_SSO_APPLICATION_SLUG.length > 0 &&
    !NEXUS_SSO_APPLICATION_SLUG.startsWith("PENDING_")
  )
}

function authorizationUrl(requestId: string): string {
  const url = new URL(NEXUS_SSO_AUTHORIZE_URL)
  url.searchParams.set("id", requestId)
  url.searchParams.set("application", NEXUS_SSO_APPLICATION_SLUG)
  return url.toString()
}

/**
 * Authenticate through Nexus Mods SSO protocol 2.
 *
 * The official SSO service returns an application-scoped credential through
 * this short-lived WebSocket. The credential is never requested from the user
 * and is passed directly to the Rust backend for validation and encrypted
 * storage.
 */
export function requestNexusSsoCredential(): Promise<string> {
  if (!isNexusSsoConfigured()) {
    return Promise.reject(
      new NexusSsoError(
        "sso_not_configured",
        "Nexus Mods has not assigned MRMMR its application slug yet.",
      ),
    )
  }

  return new Promise((resolve, reject) => {
    const requestId = crypto.randomUUID()
    const socket = new WebSocket(NEXUS_SSO_SOCKET_URL)
    let settled = false

    const timeout = window.setTimeout(() => {
      fail(
        new NexusSsoError(
          "sso_timeout",
          "Nexus Mods authorization timed out. Start the sign-in again.",
        ),
      )
    }, SSO_TIMEOUT_MS)

    function cleanup() {
      window.clearTimeout(timeout)
      if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
        socket.close()
      }
    }

    function succeed(credential: string) {
      if (settled) return
      settled = true
      cleanup()
      resolve(credential)
    }

    function fail(error: NexusSsoError) {
      if (settled) return
      settled = true
      cleanup()
      reject(error)
    }

    socket.addEventListener("open", () => {
      socket.send(
        JSON.stringify({
          id: requestId,
          token: null,
          protocol: 2,
        }),
      )

      void openUrl(authorizationUrl(requestId)).catch(() => {
        fail(
          new NexusSsoError(
            "sso_connection",
            "MRMMR could not open the Nexus Mods authorization page.",
          ),
        )
      })
    })

    socket.addEventListener("message", (event) => {
      let response: SsoResponse
      try {
        response = JSON.parse(String(event.data)) as SsoResponse
      } catch {
        fail(new NexusSsoError("sso_connection", "Nexus Mods returned an invalid SSO response."))
        return
      }

      if (!response.success) {
        fail(
          new NexusSsoError(
            "sso_rejected",
            response.error || "Nexus Mods did not authorize this sign-in.",
          ),
        )
        return
      }

      const credential = response.data?.api_key?.trim()
      if (credential) succeed(credential)
    })

    socket.addEventListener("error", () => {
      fail(new NexusSsoError("sso_connection", "Could not connect to Nexus Mods SSO."))
    })

    socket.addEventListener("close", () => {
      if (!settled) {
        fail(
          new NexusSsoError(
            "sso_connection",
            "The Nexus Mods SSO connection closed before authorization completed.",
          ),
        )
      }
    })
  })
}
