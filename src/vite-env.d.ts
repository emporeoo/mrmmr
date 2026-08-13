/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_NEXUS_SSO_APPLICATION_SLUG?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
