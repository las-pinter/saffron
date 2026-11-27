import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { formatDate } from '@/lib/format'
import {
  ACCOUNT,
  BUY,
  DIVIDEND,
  HIT,
  NOKIA,
  amount,
  book,
  dividendDate,
  editDate,
} from '@/test/bookFixtures'
import { fakeApi } from '@/test/fakeApi'
import { renderApp } from '@/test/renderApp'

beforeEach(() => {
  window.localStorage.clear()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

/** The page's transaction list, once the book has arrived. */
async function list() {
  return within(await screen.findByRole('region', { name: 'Transactions' }))
}

describe('TransactionsSection', () => {
  it('records a buy with an instrument found by search', async () => {
    const calls = fakeApi({
      ...book(),
      'GET /instruments/lookup': { body: { isin: null, results: [HIT] } },
      'POST /portfolios/7/transactions': { body: { ...BUY, id: 22 } },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(transactions.getByRole('button', { name: 'Add transaction' }))
    await userEvent.type(await screen.findByLabelText('Find an instrument'), 'Nokia')
    await userEvent.click(screen.getByRole('button', { name: 'Search' }))
    await userEvent.click(await screen.findByRole('button', { name: /Nokia Oyj/ }))
    expect(await screen.findByRole('button', { name: 'Change' })).toBeInTheDocument()

    await userEvent.type(screen.getByLabelText('Units'), '10')
    await userEvent.type(screen.getByLabelText('Price'), '4.50')
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Transaction recorded.')).toBeInTheDocument()
    const sent = amount(calls, '/portfolios/7/transactions', 'POST')[0]?.body
    expect(sent).toEqual({
      account_id: 11,
      kind: 'buy',
      date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
      instrument_id: 3,
      quantity: '10',
      price: '4.50',
    })
    expect(amount(calls, '/instruments/lookup')[0]?.path).toBe('/instruments/lookup')
  })

  it('records an unpaired transfer in with its price', async () => {
    const calls = fakeApi({
      ...book(),
      'GET /portfolios/7/transactions': { body: [] },
      'GET /portfolios/7/holdings': { body: [] },
      'GET /instruments/lookup': { body: { isin: null, results: [HIT] } },
      'POST /portfolios/7/transactions': { body: { ...DIVIDEND, id: 23 } },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(transactions.getByRole('button', { name: 'Add transaction' }))
    const form = within(await screen.findByRole('form', { name: 'Add transaction' }))
    await userEvent.selectOptions(await form.findByLabelText('Kind'), 'transfer_in')
    await userEvent.type(form.getByLabelText('Find an instrument'), 'Nokia')
    await userEvent.click(form.getByRole('button', { name: 'Search' }))
    await userEvent.click(await form.findByRole('button', { name: /Nokia Oyj/ }))
    await userEvent.type(form.getByLabelText('Units'), '5')
    await userEvent.type(await form.findByLabelText('Price'), '4.00')
    await userEvent.click(form.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Transaction recorded.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/transactions', 'POST')[0]?.body).toEqual({
      account_id: 11,
      kind: 'transfer_in',
      date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
      instrument_id: 3,
      quantity: '5',
      price: '4.00',
    })
  })

  it('searches on Enter without submitting the transaction form', async () => {
    const calls = fakeApi({
      ...book(),
      'GET /instruments/lookup': { body: { isin: null, results: [HIT] } },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(transactions.getByRole('button', { name: 'Add transaction' }))
    await userEvent.type(await screen.findByLabelText('Find an instrument'), 'Nokia{Enter}')

    expect(await screen.findByRole('button', { name: /Nokia Oyj/ })).toBeInTheDocument()
    expect(amount(calls, '/instruments/lookup')).toHaveLength(1)
    expect(amount(calls, '/portfolios/7/transactions', 'POST')).toHaveLength(0)
  })

  it('edits a stored transaction in place', async () => {
    const calls = fakeApi({
      ...book(),
      'PATCH /portfolios/7/transactions/21': { body: { ...BUY, price: '4.60' } },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(
      await transactions.findByRole('button', { name: `Edit buy of Nokia on ${editDate}` }),
    )
    // Moving a stored row to another account would reinterpret its amounts.
    expect(await screen.findByLabelText('Account')).toBeDisabled()
    const price = await screen.findByLabelText('Price')
    expect(price).toHaveValue('4.00')
    await userEvent.clear(price)
    await userEvent.type(price, '4.60')
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Transaction updated.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/transactions/21', 'PATCH')[0]?.body).toEqual({
      account_id: 11,
      kind: 'buy',
      date: '2026-11-20',
      instrument_id: 3,
      quantity: '10',
      price: '4.60',
      fee: '0',
      note: 'first',
    })
  })

  it('fills the form afresh when another row is chosen for editing', async () => {
    const calls = fakeApi({
      ...book(),
      'GET /portfolios/7/transactions': { body: [BUY, DIVIDEND] },
      'PATCH /portfolios/7/transactions/22': { body: DIVIDEND },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(
      await transactions.findByRole('button', { name: `Edit buy of Nokia on ${editDate}` }),
    )
    expect(await screen.findByLabelText('Price')).toHaveValue('4.00')

    await userEvent.click(
      await transactions.findByRole('button', {
        name: `Edit dividend of Nokia on ${dividendDate}`,
      }),
    )
    await screen.findByRole('form', { name: 'Edit transaction' })
    expect(await screen.findByLabelText('Amount')).toHaveValue('12')
    expect(screen.queryByLabelText('Price')).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(amount(calls, '/portfolios/7/transactions/22', 'PATCH')).toHaveLength(1)
    expect(amount(calls, '/portfolios/7/transactions/21', 'PATCH')).toHaveLength(0)
  })

  it('adds an instrument Saffron does not know yet', async () => {
    const calls = fakeApi({
      ...book(),
      'GET /instruments/lookup': {
        body: { isin: 'FI0009000681', results: [{ ...HIT, instrument_id: null }] },
      },
      'POST /instruments': { body: NOKIA },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(transactions.getByRole('button', { name: 'Add transaction' }))
    await userEvent.type(await screen.findByLabelText('Find an instrument'), 'Nokia')
    await userEvent.click(screen.getByRole('button', { name: 'Search' }))
    await userEvent.click(await screen.findByRole('button', { name: /Nokia Oyj/ }))

    expect(await screen.findByRole('button', { name: 'Change' })).toBeInTheDocument()
    expect(amount(calls, '/instruments', 'POST')[0]?.body).toEqual({
      symbol: 'NOKIA.HE',
      isin: 'FI0009000681',
    })
  })

  it('drops the FX rate when the transaction stays in the account currency', async () => {
    const calls = fakeApi({
      ...book(),
      'POST /portfolios/7/transactions': { body: { ...DIVIDEND, id: 24 } },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(transactions.getByRole('button', { name: 'Add transaction' }))
    const form = within(await screen.findByRole('form', { name: 'Add transaction' }))
    await userEvent.selectOptions(await form.findByLabelText('Kind'), 'deposit')
    await userEvent.type(form.getByLabelText('Amount'), '5')
    await userEvent.type(form.getByLabelText('Currency'), 'USD')
    await userEvent.type(await form.findByLabelText('FX rate'), '0.9')
    await userEvent.clear(form.getByLabelText('Currency'))
    await userEvent.click(form.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Transaction recorded.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/transactions', 'POST')[0]?.body).toEqual({
      account_id: 11,
      kind: 'deposit',
      date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/),
      amount: '5',
    })
  })

  it('deletes a transaction after a confirmation', async () => {
    const calls = fakeApi({
      ...book(),
      'DELETE /portfolios/7/transactions/21': { status: 204 },
    })
    renderApp('/holdings')

    const transactions = await list()
    await userEvent.click(
      await transactions.findByRole('button', { name: `Delete buy of Nokia on ${editDate}` }),
    )
    await userEvent.click(
      screen.getByRole('button', { name: `Delete buy of Nokia on ${editDate}` }),
    )

    expect(await screen.findByText('Transaction deleted.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/transactions/21', 'DELETE')).toHaveLength(1)
  })

  it('hides the row actions of a closed account', async () => {
    fakeApi({
      ...book(),
      'GET /portfolios/7/accounts': { body: [{ ...ACCOUNT, closed_at: '2026-11-27T12:00:00Z' }] },
    })
    renderApp('/holdings')

    const transactions = await list()
    expect(await transactions.findByText(formatDate('2026-11-20'))).toBeInTheDocument()
    expect(transactions.queryByRole('button', { name: /^Edit / })).toBeNull()
    expect(transactions.queryByRole('button', { name: /^Delete / })).toBeNull()
  })
})
