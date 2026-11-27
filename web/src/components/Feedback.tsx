import type { ReactNode } from 'react'

/** A failed action, announced to screen readers. */
export function ErrorText({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="rounded-lg bg-sf-loss-soft px-3 py-2 text-sm text-sf-loss">
      {children}
    </p>
  )
}

/** A completed action, announced to screen readers. */
export function NoticeText({ children }: { children: ReactNode }) {
  return (
    <p role="status" className="rounded-lg bg-sf-gain-soft px-3 py-2 text-sm text-sf-gain">
      {children}
    </p>
  )
}
