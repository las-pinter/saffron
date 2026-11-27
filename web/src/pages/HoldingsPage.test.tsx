import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { formatMoney, type Portfolio } from '@/lib/book'
import { formatDate } from '@/lib/format'
import { ACCOUNT, BUY, NOKIA, PORTFOLIO, STATE, amount, book, editDate } from '@/test/bookFixtures'
import { fakeApi } from '@/test/fakeApi'
import { LAS, signedIn } from '@/test/fixtures'
import { renderApp } from '@/test/renderApp'

beforeEach(() => {
  window.localStorage.clear()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('HoldingsPage', () => {
  it('shows a holding with its units and market value', async () => {
    fakeApi(book())
    renderApp('/holdings')

    const positions = within(await screen.findByRole('region', { name: 'Positions' }))
    expect(await positions.findByText('Nokia')).toBeInTheDocument()
    expect(positions.getByText('10')).toBeInTheDocument()
    // Ten units at the latest close of 4.50.
    expect(positions.getByText(formatMoney('45', 'EUR'))).toBeInTheDocument()
    expect(positions.getByText(formatMoney('4.50', 'EUR'))).toBeInTheDocument()
    expect(positions.getByText(`as of ${formatDate('2026-11-27')}`)).toBeInTheDocument()
    expect(screen.getByText(formatDate('2026-11-20'))).toBeInTheDocument()
  })

  it('creates the first portfolio from the empty state', async () => {
    const calls = fakeApi({
      'GET /auth/state': signedIn(LAS),
      'GET /portfolios': { body: [] },
      'POST /portfolios': { body: PORTFOLIO },
      'GET /portfolios/7/accounts': { body: [ACCOUNT] },
      'GET /portfolios/7/transactions': { body: [BUY] },
      'GET /portfolios/7/holdings': { body: [STATE] },
      'GET /instruments/3': { body: NOKIA },
    })
    renderApp('/holdings')

    const card = within(await screen.findByRole('region', { name: 'Create a portfolio' }))
    await userEvent.type(card.getByLabelText('Portfolio name'), 'Household')
    await userEvent.click(card.getByRole('button', { name: 'Create portfolio' }))

    expect(amount(calls, '/portfolios', 'POST')[0]?.body).toEqual({ name: 'Household' })
    expect(await screen.findByText('Household · EUR')).toBeInTheDocument()
    const positions = within(await screen.findByRole('region', { name: 'Positions' }))
    expect(await positions.findByText('Nokia')).toBeInTheDocument()
  })

  it('adds an account', async () => {
    const calls = fakeApi({ ...book(), 'POST /portfolios/7/accounts': { status: 204 } })
    renderApp('/holdings')

    const accounts = within(await screen.findByRole('region', { name: 'Accounts' }))
    await userEvent.click(await accounts.findByRole('button', { name: 'Add account' }))
    await userEvent.type(await accounts.findByLabelText('Name'), 'Nordnet')
    await userEvent.click(accounts.getByRole('button', { name: 'Create account' }))

    expect(await screen.findByText('Account created.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/accounts', 'POST')[0]?.body).toEqual({
      name: 'Nordnet',
      kind: 'book_entry',
    })
  })

  it('renames an account', async () => {
    const calls = fakeApi({ ...book(), 'PATCH /portfolios/7/accounts/11': { status: 204 } })
    renderApp('/holdings')

    const accounts = within(await screen.findByRole('region', { name: 'Accounts' }))
    await userEvent.click(await accounts.findByRole('button', { name: 'Rename OP book-entry' }))
    const field = await accounts.findByLabelText('New name')
    await userEvent.clear(field)
    await userEvent.type(field, 'OP AOT')
    await userEvent.click(accounts.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Account renamed.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/accounts/11', 'PATCH')[0]?.body).toEqual({
      name: 'OP AOT',
    })
  })

  it('closes an account and shows it behind the closed toggle', async () => {
    let closed = false
    const calls = fakeApi({
      ...book(),
      'GET /portfolios/7/accounts': () => ({
        body: [{ ...ACCOUNT, closed_at: closed ? '2026-11-27T12:00:00Z' : null }],
      }),
      'POST /portfolios/7/accounts/11/close': () => {
        closed = true
        return { status: 204 }
      },
    })
    renderApp('/holdings')

    const accounts = within(await screen.findByRole('region', { name: 'Accounts' }))
    await userEvent.click(await accounts.findByRole('button', { name: 'Close OP book-entry' }))

    expect(await screen.findByText('Account closed.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/accounts/11/close', 'POST')).toHaveLength(1)
    const toggle = await accounts.findByLabelText('Show closed accounts (1)')
    expect(accounts.queryByRole('button', { name: 'Reopen OP book-entry' })).toBeNull()
    await userEvent.click(toggle)
    expect(
      await accounts.findByRole('button', { name: 'Reopen OP book-entry' }),
    ).toBeInTheDocument()
  })

  it('reopens a closed account', async () => {
    const calls = fakeApi({
      ...book(),
      'GET /portfolios/7/accounts': { body: [{ ...ACCOUNT, closed_at: '2026-11-27T12:00:00Z' }] },
      'POST /portfolios/7/accounts/11/reopen': { status: 204 },
    })
    renderApp('/holdings')

    const accounts = within(await screen.findByRole('region', { name: 'Accounts' }))
    await userEvent.click(await accounts.findByLabelText('Show closed accounts (1)'))
    await userEvent.click(await accounts.findByRole('button', { name: 'Reopen OP book-entry' }))

    expect(await screen.findByText('Account reopened.')).toBeInTheDocument()
    expect(amount(calls, '/portfolios/7/accounts/11/reopen', 'POST')).toHaveLength(1)
  })

  it('shows a viewer a read-only page', async () => {
    fakeApi(book({ ...PORTFOLIO, role: 'viewer' }))
    renderApp('/holdings')

    const positions = within(await screen.findByRole('region', { name: 'Positions' }))
    expect(await positions.findByText('Nokia')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add transaction' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Add account' })).toBeNull()
    expect(screen.queryByRole('button', { name: `Edit buy of Nokia on ${editDate}` })).toBeNull()
  })

  it('does not offer an archived portfolio', async () => {
    fakeApi(book({ ...PORTFOLIO, archived_at: '2026-11-27T12:00:00Z' }))
    renderApp('/holdings')

    expect(await screen.findByRole('region', { name: 'Create a portfolio' })).toBeInTheDocument()
    expect(screen.queryByText('Household · EUR')).toBeNull()
  })

  it('marks a stale price', async () => {
    fakeApi({
      ...book(),
      'GET /instruments/3': {
        body: { ...NOKIA, latest_price: { ...NOKIA.latest_price!, stale: true } },
      },
    })
    renderApp('/holdings')

    const positions = within(await screen.findByRole('region', { name: 'Positions' }))
    expect(await positions.findByText(/stale/)).toBeInTheDocument()
  })

  it('shows a failed portfolio read without offering a new one', async () => {
    fakeApi({
      'GET /auth/state': signedIn(LAS),
      'GET /portfolios': {
        status: 500,
        body: { error: 'internal', message: 'the portfolio list is unavailable' },
      },
    })
    renderApp('/holdings')

    expect(
      (await screen.findAllByText('the portfolio list is unavailable')).length,
    ).toBeGreaterThan(0)
    expect(screen.queryByRole('region', { name: 'Create a portfolio' })).toBeNull()
  })

  it('keeps the remembered portfolio when the list fails', async () => {
    window.localStorage.setItem('saffron.portfolio', '8')
    fakeApi({
      'GET /auth/state': signedIn(LAS),
      'GET /portfolios': { status: 500, body: { error: 'internal', message: 'no list' } },
    })
    renderApp('/holdings')

    await screen.findAllByText('no list')
    expect(window.localStorage.getItem('saffron.portfolio')).toBe('8')
  })

  it('clears the list failure once a portfolio is created', async () => {
    fakeApi({
      'GET /auth/state': signedIn(LAS),
      'GET /portfolios': () => ({
        status: 500,
        body: { error: 'internal', message: 'the list is down' },
      }),
      'POST /portfolios': { body: PORTFOLIO },
      'GET /portfolios/7/accounts': { body: [] },
      'GET /portfolios/7/transactions': { body: [] },
      'GET /portfolios/7/holdings': { body: [] },
    })
    renderApp('/holdings')

    await screen.findAllByText('the list is down')
    await userEvent.type(screen.getByLabelText('Portfolio name'), 'Household')
    await userEvent.click(screen.getByRole('button', { name: 'Create portfolio' }))

    expect(await screen.findByText('Household · EUR')).toBeInTheDocument()
    expect(screen.queryByText('the list is down')).toBeNull()
  })

  it('shows one error when the book fails to load', async () => {
    fakeApi({
      ...book(),
      'GET /portfolios/7/accounts': {
        status: 500,
        body: { error: 'internal', message: 'the book is unavailable' },
      },
    })
    renderApp('/holdings')

    expect(await screen.findByText('the book is unavailable')).toBeInTheDocument()
    expect(screen.queryAllByText('Loading…')).toHaveLength(0)
  })

  it('starts clean when the portfolio switches', async () => {
    const other: Portfolio = { ...PORTFOLIO, id: 8, name: 'Nordnet' }
    fakeApi({
      ...book(),
      'GET /portfolios': { body: [PORTFOLIO, other] },
      'GET /portfolios/8/accounts': { body: [{ ...ACCOUNT, id: 12, name: 'Nordnet account' }] },
      'GET /portfolios/8/transactions': { body: [] },
      'GET /portfolios/8/holdings': { body: [] },
    })
    renderApp('/holdings')

    const transactions = within(await screen.findByRole('region', { name: 'Transactions' }))
    await userEvent.click(
      await transactions.findByRole('button', { name: `Edit buy of Nokia on ${editDate}` }),
    )
    expect(await screen.findByRole('form', { name: 'Edit transaction' })).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: /Nordnet/ }))

    expect(await screen.findByText('Nordnet account')).toBeInTheDocument()
    expect(screen.queryByRole('form', { name: 'Edit transaction' })).toBeNull()
    expect(screen.queryByText('OP book-entry')).toBeNull()
  })
})
