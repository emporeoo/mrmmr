import type { ReactNode } from "react"

interface PageHeaderProps {
  title: string
  description?: string
  icon: ReactNode
  trailing?: ReactNode
}

export function PageHeader({ title, description, icon, trailing }: PageHeaderProps) {
  return (
    <header className="flex min-h-[72px] items-center gap-3 border-b border-border bg-[#181818] px-6 py-3.5">
      <div className="grid size-8 shrink-0 place-items-center text-primary">{icon}</div>
      <div className="min-w-0">
        <h1 className="text-[15px] font-semibold tracking-[-0.01em]">{title}</h1>
        {description ? (
          <p className="mt-0.5 text-xs leading-snug text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {trailing ? <div className="ml-auto shrink-0">{trailing}</div> : null}
    </header>
  )
}
