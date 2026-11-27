import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api } from '@/lib/api'
import type { Portfolio } from '@/lib/book'
import { useErrorMessage } from '@/lib/errors'
import { PortfolioContext, type PortfolioValue } from './context'

/** Where the browser remembers the last chosen portfolio. */
const STORAGE_KEY = 'saffron.portfolio'

function remembered(): number | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    const id = raw === null ? Number.NaN : Number(raw)
    return Number.isInteger(id) ? id : null
  } catch {
    return null
  }
}

function remember(id: number | null) {
  try {
    if (id === null) window.localStorage.removeItem(STORAGE_KEY)
    else window.localStorage.setItem(STORAGE_KEY, String(id))
  } catch {
    // A browser without storage just forgets the choice.
  }
}

/** The first portfolio the switcher offers; archived ones are not offered. */
function firstOf(list: Portfolio[]): Portfolio | null {
  return list.find((portfolio) => portfolio.archived_at === null) ?? null
}

export function PortfolioProvider({ children }: { children: ReactNode }) {
  const messageOf = useErrorMessage()
  const [portfolios, setPortfolios] = useState<Portfolio[] | null>(null)
  const [selectedId, setSelectedId] = useState<number | null>(remembered)
  const [error, setError] = useState<string | null>(null)

  const apply = useCallback((list: Portfolio[]) => {
    setPortfolios(list)
    setError(null)
    setSelectedId((current) => {
      const kept = list.find(
        (portfolio) => portfolio.id === current && portfolio.archived_at === null,
      )
      return kept ? kept.id : (firstOf(list)?.id ?? null)
    })
  }, [])

  useEffect(() => {
    let live = true
    api
      .get<Portfolio[]>('/portfolios')
      .then((list) => {
        if (live) apply(list)
      })
      .catch((failure: unknown) => {
        if (live) setError(messageOf(failure))
      })
    return () => {
      live = false
    }
  }, [apply, messageOf])

  const select = useCallback((id: number) => {
    setSelectedId(id)
    remember(id)
  }, [])

  const create = useCallback(
    async (name: string) => {
      const created = await api.post<Portfolio>('/portfolios', { name })
      setError(null)
      setPortfolios((list) => [...(list ?? []), created])
      select(created.id)
      return created
    },
    [select],
  )

  const selected = useMemo(
    () => portfolios?.find((portfolio) => portfolio.id === selectedId) ?? null,
    [portfolios, selectedId],
  )

  // Keep the remembered choice in step with the live one; a failed first read must
  // not erase it.
  useEffect(() => {
    if (portfolios !== null) remember(selected?.id ?? null)
  }, [portfolios, selected])

  const value = useMemo<PortfolioValue>(
    () => ({
      portfolios,
      selected,
      loading: portfolios === null && error === null,
      error,
      select,
      create,
    }),
    [portfolios, selected, error, select, create],
  )

  return <PortfolioContext.Provider value={value}>{children}</PortfolioContext.Provider>
}
