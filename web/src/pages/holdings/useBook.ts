/** Everything the Holdings page reads about one portfolio, in one place. */

import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '@/lib/api'
import type { Account, AccountHoldings, Instrument, Transaction } from '@/lib/book'
import { useErrorMessage } from '@/lib/errors'

export interface BookData {
  accounts: Account[] | null
  transactions: Transaction[] | null
  holdings: AccountHoldings[] | null
  /** The instruments the holdings and rows name, by id. */
  instruments: Map<number, Instrument>
  error: string | null
  /** Re-reads everything; call it after any write. */
  reload: () => Promise<void>
}

interface Read {
  accounts: Account[]
  transactions: Transaction[]
  holdings: AccountHoldings[]
  instruments: Map<number, Instrument>
}

async function readBook(portfolioId: number): Promise<Read> {
  const base = `/portfolios/${portfolioId}`
  const [accounts, transactions, holdings] = await Promise.all([
    api.get<Account[]>(`${base}/accounts`),
    api.get<Transaction[]>(`${base}/transactions`),
    api.get<AccountHoldings[]>(`${base}/holdings`),
  ])
  const ids = [
    ...new Set([
      ...holdings.flatMap((state) => state.holdings.map((row) => row.instrument_id)),
      ...transactions.flatMap((row) => (row.instrument_id === null ? [] : [row.instrument_id])),
    ]),
  ]
  const views = await Promise.all(ids.map((id) => api.get<Instrument>(`/instruments/${id}`)))
  return {
    accounts,
    transactions,
    holdings,
    instruments: new Map(views.map((view) => [view.id, view])),
  }
}

export function useBook(portfolioId: number | null): BookData {
  const messageOf = useErrorMessage()
  const [accounts, setAccounts] = useState<Account[] | null>(null)
  const [transactions, setTransactions] = useState<Transaction[] | null>(null)
  const [holdings, setHoldings] = useState<AccountHoldings[] | null>(null)
  const [instruments, setInstruments] = useState<Map<number, Instrument>>(new Map())
  const [error, setError] = useState<string | null>(null)
  // A read that started before a change must not overwrite the newer state.
  const latest = useRef(0)

  const apply = useCallback((read: Read) => {
    setAccounts(read.accounts)
    setTransactions(read.transactions)
    setHoldings(read.holdings)
    setInstruments(read.instruments)
    setError(null)
  }, [])

  const reload = useCallback(async () => {
    // Nothing is shown without a portfolio, so there is nothing to read.
    if (portfolioId === null) return
    const generation = ++latest.current
    try {
      const read = await readBook(portfolioId)
      if (latest.current === generation) apply(read)
    } catch (failure) {
      if (latest.current === generation) setError(messageOf(failure))
    }
  }, [portfolioId, apply, messageOf])

  useEffect(() => {
    if (portfolioId === null) return
    let live = true
    const generation = ++latest.current
    readBook(portfolioId)
      .then((read) => {
        if (live && latest.current === generation) apply(read)
      })
      .catch((failure: unknown) => {
        if (live && latest.current === generation) setError(messageOf(failure))
      })
    return () => {
      live = false
    }
  }, [portfolioId, apply, messageOf])

  return { accounts, transactions, holdings, instruments, error, reload }
}
