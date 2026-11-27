import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { usePortfolio } from './context'
import { CreatePortfolioForm } from './CreatePortfolioForm'

/** The header's portfolio choice, with creating one in the same menu. */
export function PortfolioSwitcher() {
  const { t } = useTranslation()
  const { portfolios, selected, select, loading, error } = usePortfolio()
  const offered = portfolios?.filter((portfolio) => portfolio.archived_at === null) ?? null
  const menu = useRef<HTMLDetailsElement>(null)

  function close() {
    menu.current?.removeAttribute('open')
  }

  return (
    <details ref={menu} className="relative">
      <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-lg border border-sf-border px-3 py-2 text-sm text-sf-text-secondary hover:text-foreground [&::-webkit-details-marker]:hidden">
        <span className="max-w-40 truncate">{selected ? selected.name : t('portfolio.none')}</span>
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <path d="M2 4.5 6 8.5 10 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      </summary>
      <div className="absolute right-0 z-20 mt-2 w-64 max-w-[calc(100vw-2rem)] rounded-xl border border-border bg-sf-surface p-2 shadow-lg">
        {loading ? (
          <p role="status" className="px-3 py-2 text-sm text-sf-text-muted">
            {t('common.loading')}
          </p>
        ) : null}
        {error ? <p className="px-3 py-2 text-sm text-sf-loss">{error}</p> : null}
        {offered?.length === 0 ? (
          <p className="px-3 py-2 text-sm text-sf-text-muted">{t('portfolio.empty')}</p>
        ) : null}
        {offered && offered.length > 0 ? (
          <ul>
            {offered.map((portfolio) => (
              <li key={portfolio.id}>
                <button
                  type="button"
                  aria-current={portfolio.id === selected?.id ? 'true' : undefined}
                  onClick={() => {
                    select(portfolio.id)
                    close()
                  }}
                  className={`flex min-h-11 w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-sm ${
                    portfolio.id === selected?.id
                      ? 'bg-sf-accent-soft text-sf-accent-on-soft'
                      : 'text-foreground hover:bg-sf-surface-sunken'
                  }`}
                >
                  <span className="truncate">{portfolio.name}</span>
                  <span className="text-xs text-sf-text-muted">{portfolio.base_currency}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : null}
        <div className="mt-2 border-t border-sf-divider p-1 pt-3">
          <CreatePortfolioForm onDone={close} />
        </div>
      </div>
    </details>
  )
}
