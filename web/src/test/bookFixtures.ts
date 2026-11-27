/** The book the Holdings page tests read: one portfolio, one account, one buy. */

import {
  type Account,
  type AccountHoldings,
  type Instrument,
  type Portfolio,
  type Transaction,
} from '@/lib/book'
import { formatDate } from '@/lib/format'
import { fakeApi, type Routes } from '@/test/fakeApi'
import { LAS, signedIn } from '@/test/fixtures'

export const PORTFOLIO: Portfolio = {
  id: 7,
  name: 'Household',
  base_currency: 'EUR',
  benchmark_instrument_id: null,
  role: 'owner',
  owner: { id: 1, username: 'las', display_name: 'Laszlo' },
  created_at: '2026-11-20T09:00:00Z',
  archived_at: null,
}

export const ACCOUNT: Account = {
  id: 11,
  name: 'OP book-entry',
  kind: 'book_entry',
  currency: 'EUR',
  institution: 'OP',
  created_at: '2026-11-20T09:00:00Z',
  closed_at: null,
}

export const NOKIA: Instrument = {
  id: 3,
  kind: 'stock',
  name: 'Nokia',
  isin: 'FI0009000681',
  currency: 'EUR',
  asset_class: 'equity',
  region: 'europe',
  created_at: '2026-11-20T09:00:00Z',
  listings: [],
  latest_price: {
    date: '2026-11-27',
    close: '4.50',
    currency: 'EUR',
    source: 'yahoo',
    as_of: '2026-11-27T16:30:00Z',
    checked_at: '2026-11-27T16:31:00Z',
    stale: false,
  },
}

export const BUY: Transaction = {
  id: 21,
  account_id: 11,
  instrument_id: 3,
  kind: 'buy',
  date: '2026-11-20',
  quantity: '10',
  price: '4.00',
  amount: null,
  fee: '0',
  currency: 'EUR',
  fx_rate: null,
  note: 'first',
}

export const DIVIDEND: Transaction = {
  id: 22,
  account_id: 11,
  instrument_id: 3,
  kind: 'dividend',
  date: '2026-11-21',
  quantity: null,
  price: null,
  amount: '12',
  fee: '0',
  currency: 'EUR',
  fx_rate: null,
  note: null,
}

export const STATE: AccountHoldings = {
  account_id: 11,
  currency: 'EUR',
  cash: '0',
  realized: '0',
  holdings: [
    { instrument_id: 3, quantity: '10', cost_basis: '40', average_cost: '4.00', lots: [] },
  ],
}

/** A search hit for Nokia that Saffron already knows. */
export const HIT = {
  provider: 'yahoo',
  symbol: 'NOKIA.HE',
  name: 'Nokia Oyj',
  kind: 'stock',
  exchange: 'Helsinki',
  instrument_id: 3,
}

export const editDate = formatDate('2026-11-20')
export const dividendDate = formatDate('2026-11-21')

/** The usual signed-in book: one portfolio, one account, one buy of ten Nokias. */
export function book(portfolio: Portfolio = PORTFOLIO): Routes {
  return {
    'GET /auth/state': signedIn(LAS),
    'GET /portfolios': { body: [portfolio] },
    'GET /portfolios/7/accounts': { body: [ACCOUNT] },
    'GET /portfolios/7/transactions': { body: [BUY] },
    'GET /portfolios/7/holdings': { body: [STATE] },
    'GET /instruments/3': { body: NOKIA },
  }
}

/** The calls `fakeApi` recorded for one path and method. */
export function amount(calls: ReturnType<typeof fakeApi>, path: string, method = 'GET') {
  return calls.filter((call) => call.method === method && call.path === path)
}
