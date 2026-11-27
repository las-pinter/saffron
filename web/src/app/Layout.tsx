import { useRef } from 'react'
import { NavLink, Outlet } from 'react-router'
import { useTranslation } from 'react-i18next'
import { Compass, House, Sparkles, Wallet } from 'lucide-react'
import { useAuth } from '@/auth/context'
import { Logo } from '@/components/Logo'
import { PortfolioProvider } from '@/portfolio/PortfolioProvider'
import { PortfolioSwitcher } from '@/portfolio/PortfolioSwitcher'

const NAV_ITEMS = [
  { to: '/', key: 'nav.overview', icon: House },
  { to: '/holdings', key: 'nav.holdings', icon: Wallet },
  { to: '/discover', key: 'nav.discover', icon: Compass },
  { to: '/ai', key: 'nav.aiDesk', icon: Sparkles },
] as const

/** The navigation links, as pills in the header and as tabs on a phone. */
function NavLinks({ place }: { place: 'header' | 'tabs' }) {
  const { t } = useTranslation()

  return NAV_ITEMS.map(({ to, key, icon: Icon }) => (
    <NavLink
      key={to}
      to={to}
      end={to === '/'}
      className={({ isActive }) =>
        place === 'header'
          ? `flex min-h-11 items-center rounded-lg px-3.5 no-underline ${
              isActive
                ? 'bg-sf-accent-soft font-semibold text-sf-accent-on-soft'
                : 'text-sf-text-secondary hover:text-foreground'
            }`
          : `flex min-h-14 flex-col items-center justify-center gap-0.5 rounded-lg text-xs no-underline ${
              isActive
                ? 'bg-sf-accent-soft font-semibold text-sf-accent-on-soft'
                : 'text-sf-text-secondary'
            }`
      }
    >
      <Icon size={20} aria-hidden="true" className={place === 'tabs' ? '' : 'mr-2'} />
      {t(key)}
    </NavLink>
  ))
}

/** The account menu holds the signed-in name, Settings, and Sign out. */
function AccountMenu() {
  const { t } = useTranslation()
  const { user, signOut } = useAuth()
  const menu = useRef<HTMLDetailsElement>(null)

  if (!user) return null

  function close() {
    menu.current?.removeAttribute('open')
  }

  return (
    <details ref={menu} className="relative ml-auto">
      <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-lg px-3 py-2 text-sm text-sf-text-secondary hover:text-foreground [&::-webkit-details-marker]:hidden">
        <span className="max-w-40 truncate">{user.display_name}</span>
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <path d="M2 4.5 6 8.5 10 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      </summary>
      <div className="absolute right-0 z-20 mt-2 w-56 rounded-xl border border-border bg-sf-surface p-2 shadow-lg">
        <p className="px-3 py-1 text-xs break-all text-sf-text-muted">@{user.username}</p>
        <NavLink
          to="/settings"
          onClick={close}
          className={({ isActive }) =>
            `block min-h-11 rounded-lg px-3 py-2 text-sm no-underline ${
              isActive ? 'bg-sf-accent-soft text-sf-accent-on-soft' : 'text-foreground'
            }`
          }
        >
          {t('account.settings')}
        </NavLink>
        <button
          type="button"
          className="min-h-11 w-full rounded-lg px-3 py-2 text-left text-sm text-foreground hover:bg-sf-accent-soft"
          onClick={() => {
            close()
            void signOut()
          }}
        >
          {t('account.signOut')}
        </button>
      </div>
    </details>
  )
}

export function Layout() {
  return (
    <PortfolioProvider>
      <Shell />
    </PortfolioProvider>
  )
}

function Shell() {
  const { t } = useTranslation()

  return (
    <div className="min-h-screen bg-background text-foreground">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-30 focus:rounded-lg focus:bg-sf-surface focus:px-3 focus:py-2"
      >
        {t('nav.skip')}
      </a>
      <header className="border-b border-border bg-sf-surface">
        <div className="mx-auto flex max-w-[1320px] flex-wrap items-center gap-x-4 gap-y-1 px-5 py-2 sm:gap-x-6 sm:px-8">
          <NavLink to="/" className="flex items-center gap-2.5 py-2 text-foreground no-underline">
            <Logo />
            <span className="font-heading text-[19px] font-bold">{t('app.name')}</span>
          </NavLink>
          <PortfolioSwitcher />
          <nav aria-label={t('nav.label')} className="hidden flex-wrap gap-1 sm:flex">
            <NavLinks place="header" />
          </nav>
          <AccountMenu />
        </div>
      </header>
      <main id="main" className="mx-auto max-w-[1320px] px-5 pt-6 pb-24 sm:p-8">
        <Outlet />
      </main>
      <nav
        aria-label={t('nav.phone')}
        className="fixed inset-x-0 bottom-0 z-10 border-t border-border bg-sf-surface px-2 pb-[env(safe-area-inset-bottom)] sm:hidden"
      >
        <div className="grid grid-cols-4 gap-1 py-1">
          <NavLinks place="tabs" />
        </div>
      </nav>
    </div>
  )
}
