import { NavLink, Outlet } from 'react-router'
import { useTranslation } from 'react-i18next'

const NAV_ITEMS = [
  { to: '/', key: 'nav.overview' },
  { to: '/holdings', key: 'nav.holdings' },
  { to: '/discover', key: 'nav.discover' },
  { to: '/ai', key: 'nav.aiDesk' },
] as const

function Logo() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden="true">
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

export function Layout() {
  const { t } = useTranslation()

  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b border-border bg-sf-surface">
        <div className="mx-auto flex min-h-16 max-w-[1320px] flex-wrap items-center gap-6 px-8">
          <NavLink to="/" className="flex items-center gap-2.5 text-foreground no-underline">
            <Logo />
            <span className="font-heading text-[19px] font-bold">{t('app.name')}</span>
          </NavLink>
          <nav aria-label={t('nav.label')} className="flex flex-wrap gap-1">
            {NAV_ITEMS.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.to === '/'}
                className={({ isActive }) =>
                  `rounded-lg px-3.5 py-2.5 no-underline ${
                    isActive
                      ? 'bg-sf-accent-soft font-semibold text-sf-accent-on-soft'
                      : 'text-sf-text-secondary hover:text-foreground'
                  }`
                }
              >
                {t(item.key)}
              </NavLink>
            ))}
          </nav>
        </div>
      </header>
      <main className="mx-auto max-w-[1320px] p-8">
        <Outlet />
      </main>
    </div>
  )
}
