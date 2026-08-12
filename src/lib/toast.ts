import { create } from "zustand"

export type ToastVariant = "success" | "error" | "warning" | "info"

export interface ToastItem {
  id: number
  variant: ToastVariant
  title: string
  description?: string
  leaving?: boolean
}

interface ToastState {
  toasts: ToastItem[]
  queue: ToastItem[]
  push: (toast: Omit<ToastItem, "id">) => number
  dismiss: (id: number) => void
}

const MAX_VISIBLE = 3
const EXIT_DURATION = 150
let nextId = 1

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  queue: [],

  push: (toast) => {
    const item = { ...toast, id: nextId++ }
    set((state) =>
      state.toasts.length < MAX_VISIBLE
        ? { toasts: [...state.toasts, item] }
        : { queue: [...state.queue, item] },
    )
    return item.id
  },

  dismiss: (id) => {
    set((state) => ({
      toasts: state.toasts.map((item) =>
        item.id === id && !item.leaving ? { ...item, leaving: true } : item,
      ),
      queue: state.queue.filter((item) => item.id !== id),
    }))
    window.setTimeout(() => {
      set((state) => {
        const toasts = state.toasts.filter((item) => item.id !== id)
        if (toasts.length >= MAX_VISIBLE || state.queue.length === 0) return { toasts }
        const [next, ...queue] = state.queue
        return { toasts: [...toasts, next], queue }
      })
    }, EXIT_DURATION)
  },
}))

function show(toast: Omit<ToastItem, "id">) {
  useToastStore.getState().push(toast)
}

export const toast = {
  success: (title: string, description?: string) =>
    show({ variant: "success", title, description }),
  error: (title: string, description?: string) =>
    show({ variant: "error", title, description }),
  warning: (title: string, description?: string) =>
    show({ variant: "warning", title, description }),
  info: (title: string, description?: string) =>
    show({ variant: "info", title, description }),
}
