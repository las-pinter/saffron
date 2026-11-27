import { useTranslation } from 'react-i18next'

/** The saffron mark: a crocus with its threads, as in docs/design/DESIGN.md. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" aria-hidden="true">
      <circle cx="14" cy="14" r="13" className="fill-sf-accent-soft" />
      <path
        d="M14 22C12 16 9 12 7 8M14 22C14 15 14 10 14 6M14 22C16 16 19 12 21 8"
        className="stroke-sf-crimson"
        strokeWidth="2"
        fill="none"
        strokeLinecap="round"
      />
      <circle cx="7" cy="8" r="1.8" className="fill-sf-accent" />
      <circle cx="14" cy="6" r="1.8" className="fill-sf-accent" />
      <circle cx="21" cy="8" r="1.8" className="fill-sf-accent" />
    </svg>
  )
}

/** A heading block for the public screens, which carry no navigation. */
export function PublicHeader({ title, subtitle }: { title: string; subtitle: string }) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-col items-center gap-2 text-center">
      <Logo size={40} />
      <p className="font-heading text-sm font-semibold tracking-wide text-sf-text-muted uppercase">
        {t('app.name')}
      </p>
      <h1 className="font-heading text-2xl font-semibold tracking-tight">{title}</h1>
      <p className="text-sm text-sf-text-muted">{subtitle}</p>
    </div>
  )
}
