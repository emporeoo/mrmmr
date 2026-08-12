import type { ReactNode } from "react"

interface EmptyStateProps {
  icon: ReactNode
  title: string
  description: string
  action?: ReactNode
}

export function EmptyState({ icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex min-h-64 flex-col items-center justify-center border border-dashed border-border bg-[#181818] px-6 py-12 text-center">
      <div className="mb-3 grid size-10 place-items-center rounded-sm border border-border bg-muted text-muted-foreground">
        {icon}
      </div>
      <h2 className="text-sm font-semibold">{title}</h2>
      <p className="mt-1 max-w-sm text-xs leading-relaxed text-muted-foreground">{description}</p>
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  )
}
