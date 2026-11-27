import { useId, type ReactNode } from 'react'

interface CardProps {
  title: string
  description?: string
  children: ReactNode
}

/** One settings block: a surface card with a heading and an optional lead line. */
export function Card({ title, description, children }: CardProps) {
  const headingId = useId()

  return (
    <section
      aria-labelledby={headingId}
      className="rounded-2xl border border-border bg-sf-surface p-5 sm:p-6"
    >
      <h2 id={headingId} className="font-heading text-lg font-semibold tracking-tight">
        {title}
      </h2>
      {description ? <p className="mt-1 text-sm text-sf-text-muted">{description}</p> : null}
      <div className="mt-4">{children}</div>
    </section>
  )
}
