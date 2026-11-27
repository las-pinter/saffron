import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import { ErrorText } from '@/components/Feedback'
import type { Portfolio } from '@/lib/book'
import { usePortfolio } from '@/portfolio/context'
import { CreatePortfolioForm } from '@/portfolio/CreatePortfolioForm'
import { AccountsSection } from './holdings/AccountsSection'
import { PositionsSection } from './holdings/PositionsSection'
import { TransactionsSection } from './holdings/TransactionsSection'
import { useBook } from './holdings/useBook'

/** One portfolio's book; the page keys it by portfolio id so a switch starts clean. */
function BookView({ portfolio }: { portfolio: Portfolio }) {
  const book = useBook(portfolio.id)
  const readOnly = portfolio.role === 'viewer'

  if (book.error) return <ErrorText>{book.error}</ErrorText>

  return (
    <>
      <AccountsSection
        portfolioId={portfolio.id}
        baseCurrency={portfolio.base_currency}
        accounts={book.accounts}
        readOnly={readOnly}
        reload={book.reload}
      />
      <PositionsSection
        accounts={book.accounts}
        holdings={book.holdings}
        instruments={book.instruments}
      />
      <TransactionsSection
        portfolioId={portfolio.id}
        accounts={book.accounts}
        transactions={book.transactions}
        instruments={book.instruments}
        readOnly={readOnly}
        reload={book.reload}
      />
    </>
  )
}

/** The portfolio book: accounts, positions, and transactions in one place. */
export function HoldingsPage() {
  const { t } = useTranslation()
  const { portfolios, selected, loading, error: portfolioError } = usePortfolio()

  return (
    <div className="space-y-6">
      <header>
        <h1 className="font-heading text-3xl font-semibold tracking-tight">{t('nav.holdings')}</h1>
        <p className="mt-1 text-sm text-sf-text-muted">
          {selected
            ? t('holdings.subtitle', {
                name: selected.name,
                currency: selected.base_currency,
              })
            : t('holdings.subtitleEmpty')}
        </p>
      </header>
      {loading ? (
        <p role="status" className="text-sm text-sf-text-muted">
          {t('common.loading')}
        </p>
      ) : null}
      {portfolioError ? <ErrorText>{portfolioError}</ErrorText> : null}
      {portfolios !== null && portfolios.every((portfolio) => portfolio.archived_at !== null) ? (
        <Card
          title={t('holdings.noPortfolio.title')}
          description={t('holdings.noPortfolio.description')}
        >
          <CreatePortfolioForm />
        </Card>
      ) : null}
      {selected ? <BookView key={selected.id} portfolio={selected} /> : null}
    </div>
  )
}
