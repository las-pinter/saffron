/** The chosen portfolio and the list the header switcher shows. */

import { createContext, useContext } from 'react'
import type { Portfolio } from '@/lib/book'

export interface PortfolioValue {
  /** Every portfolio the user can see; null until the first read. */
  portfolios: Portfolio[] | null
  /** The one the pages work on; null while none exists. */
  selected: Portfolio | null
  /** Whether the first read has finished. */
  loading: boolean
  /** Why the last read failed, if it did. */
  error: string | null
  select: (id: number) => void
  /** Creates a portfolio, selects it, and returns it. */
  create: (name: string) => Promise<Portfolio>
}

export const PortfolioContext = createContext<PortfolioValue | null>(null)

export function usePortfolio(): PortfolioValue {
  const value = useContext(PortfolioContext)
  if (!value) throw new Error('usePortfolio is used outside PortfolioProvider')
  return value
}
