import { expect, test } from '@playwright/test'
import { expectNoSeriousViolations, signedIn, stubApi } from './support'

const PORTFOLIO = {
  id: 9,
  name: 'Household',
  base_currency: 'EUR',
  benchmark_instrument_id: null,
  role: 'owner',
  owner: { id: 1, username: 'las', display_name: 'Laszlo' },
  created_at: '2026-11-20T09:00:00Z',
  archived_at: null,
}

const ACCOUNT = {
  id: 11,
  name: 'OP book-entry',
  kind: 'book_entry',
  currency: 'EUR',
  institution: 'OP',
  created_at: '2026-11-20T09:00:00Z',
  closed_at: null,
}

const NOKIA = {
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

const BUY = {
  id: 31,
  account_id: 11,
  instrument_id: 3,
  kind: 'buy',
  date: '2026-11-27',
  quantity: '10',
  price: '4.50',
  amount: null,
  fee: '0',
  currency: 'EUR',
  fx_rate: null,
  note: null,
}

const EMPTY = { account_id: 11, currency: 'EUR', cash: '0', realized: '0', holdings: [] }

const FILLED = {
  account_id: 11,
  currency: 'EUR',
  cash: '0',
  realized: '0',
  holdings: [
    { instrument_id: 3, quantity: '10', cost_basis: '45', average_cost: '4.50', lots: [] },
  ],
}

test('a buy is recorded and its holding shows the right value', async ({ page }) => {
  let bought = false
  await stubApi(page, {
    ...signedIn(),
    '/portfolios': { body: [PORTFOLIO] },
    '/portfolios/9/accounts': { body: [ACCOUNT] },
    'GET /portfolios/9/transactions': () => ({ body: bought ? [BUY] : [] }),
    'GET /portfolios/9/holdings': () => ({ body: [bought ? FILLED : EMPTY] }),
    'POST /portfolios/9/transactions': () => {
      bought = true
      return { status: 204 }
    },
    '/instruments/lookup': {
      body: {
        isin: null,
        results: [
          {
            provider: 'yahoo',
            symbol: 'NOKIA.HE',
            name: 'Nokia Oyj',
            kind: 'stock',
            exchange: 'Helsinki',
            instrument_id: 3,
          },
        ],
      },
    },
    '/instruments/3': { body: NOKIA },
  })
  await page.goto('/holdings')

  const positions = page.getByRole('region', { name: 'Positions' })
  await expect(positions.getByText('Nothing recorded yet')).toBeVisible()

  const transactions = page.getByRole('region', { name: 'Transactions' })
  await transactions.getByRole('button', { name: 'Add transaction' }).click()
  await page.getByLabel('Find an instrument').fill('Nokia')
  await page.getByRole('button', { name: 'Search' }).click()
  await page.getByRole('button', { name: /Nokia Oyj/ }).click()
  await page.getByLabel('Units').fill('10')
  await page.getByLabel('Price').fill('4.50')
  await page.getByRole('button', { name: 'Save' }).click()

  await expect(page.getByText('Transaction recorded.')).toBeVisible()
  await expect(positions.getByText('Nokia')).toBeVisible()
  await expect(positions.getByText('10')).toBeVisible()
  // Ten units at the latest close of 4.50.
  await expect(positions.getByText(/€\s?45[.,]00/)).toBeVisible()
  await expectNoSeriousViolations(page)
})
