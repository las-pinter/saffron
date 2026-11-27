import { useTranslation } from 'react-i18next'
import { Card } from '@/components/Card'
import {
  accountName,
  formatMoney,
  formatQuantity,
  holdingValue,
  type Account,
  type AccountHoldings,
  type Instrument,
} from '@/lib/book'
import { formatDate } from '@/lib/format'

interface PositionsSectionProps {
  accounts: Account[] | null
  holdings: AccountHoldings[] | null
  instruments: Map<number, Instrument>
}

/** What each account holds, with the value at the latest known price. */
export function PositionsSection({ accounts, holdings, instruments }: PositionsSectionProps) {
  const { t } = useTranslation()
  const nonEmpty = (holdings ?? []).filter(
    (state) => state.holdings.length > 0 || Number(state.cash) !== 0,
  )

  return (
    <Card title={t('holdings.positions.title')} description={t('holdings.positions.description')}>
      {holdings === null ? (
        <p role="status" className="text-sm text-sf-text-muted">
          {t('common.loading')}
        </p>
      ) : null}
      {holdings && nonEmpty.length === 0 ? (
        <p className="text-sm text-sf-text-muted">{t('holdings.positions.empty')}</p>
      ) : null}
      {nonEmpty.map((state) => (
        <section key={state.account_id} className="mt-5 first:mt-0">
          <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
            <h3 className="font-heading text-base font-semibold">
              {accountName(accounts, state.account_id)}
            </h3>
            <p className="text-sm text-sf-text-muted">
              {t('holdings.positions.cashValue', {
                cash: formatMoney(state.cash, state.currency),
                realized: formatMoney(state.realized, state.currency),
              })}
            </p>
          </div>
          {state.holdings.length === 0 ? (
            <p className="mt-1 text-sm text-sf-text-muted">{t('holdings.positions.noHoldings')}</p>
          ) : (
            <div className="mt-2 overflow-x-auto">
              <table className="w-full border-collapse text-sm">
                <thead>
                  <tr className="text-left text-xs text-sf-text-muted">
                    <th scope="col" className="py-2 pr-4 font-medium">
                      {t('holdings.positions.instrument')}
                    </th>
                    <th scope="col" className="py-2 pr-4 text-right font-medium">
                      {t('holdings.positions.quantity')}
                    </th>
                    <th scope="col" className="py-2 pr-4 text-right font-medium">
                      {t('holdings.positions.averageCost')}
                    </th>
                    <th scope="col" className="py-2 pr-4 text-right font-medium">
                      {t('holdings.positions.price')}
                    </th>
                    <th scope="col" className="py-2 text-right font-medium">
                      {t('holdings.positions.value')}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {state.holdings.map((holding) => {
                    const instrument = instruments.get(holding.instrument_id)
                    const price = instrument?.latest_price
                    const value = holdingValue(holding, instrument)
                    return (
                      <tr
                        key={holding.instrument_id}
                        className="border-t border-sf-divider align-top"
                      >
                        <td className="max-w-56 py-2 pr-4 break-words">
                          {instrument?.name ?? `#${holding.instrument_id}`}
                        </td>
                        <td className="py-2 pr-4 text-right whitespace-nowrap">
                          {formatQuantity(holding.quantity)}
                        </td>
                        <td className="py-2 pr-4 text-right whitespace-nowrap">
                          {formatMoney(holding.average_cost, state.currency)}
                        </td>
                        <td className="py-2 pr-4 text-right whitespace-nowrap">
                          {price ? (
                            <>
                              {formatMoney(price.close, price.currency)}
                              <span className="block text-xs text-sf-text-muted">
                                {t('holdings.positions.priceDate', {
                                  date: formatDate(price.date),
                                })}
                                {price.stale ? (
                                  <span className="text-sf-loss">
                                    {` · ${t('holdings.positions.stale')}`}
                                  </span>
                                ) : null}
                              </span>
                            </>
                          ) : (
                            t('holdings.positions.noPrice')
                          )}
                        </td>
                        <td className="py-2 text-right whitespace-nowrap">
                          {value === null || !instrument
                            ? t('holdings.positions.noPrice')
                            : formatMoney(String(value), instrument.currency)}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          )}
        </section>
      ))}
    </Card>
  )
}
