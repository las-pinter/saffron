/** Types and rules of the portfolio book, as the API serves it. */

export interface PortfolioOwner {
  id: number
  username: string
  display_name: string
}

export interface Portfolio {
  id: number
  name: string
  base_currency: string
  benchmark_instrument_id: number | null
  role: 'owner' | 'editor' | 'viewer'
  owner: PortfolioOwner
  created_at: string
  archived_at: string | null
}

export interface Account {
  id: number
  name: string
  kind: string
  currency: string
  institution: string | null
  created_at: string
  closed_at: string | null
}

export interface Transaction {
  id: number
  account_id: number
  instrument_id: number | null
  kind: TransactionKind
  date: string
  quantity: string | null
  price: string | null
  amount: string | null
  fee: string
  currency: string
  fx_rate: string | null
  note: string | null
}

export interface Lot {
  acquired: string
  quantity: string
  unit_cost: string
}

export interface Holding {
  instrument_id: number
  quantity: string
  cost_basis: string
  average_cost: string | null
  lots: Lot[]
}

export interface AccountHoldings {
  account_id: number
  currency: string
  cash: string
  realized: string
  holdings: Holding[]
}

export interface Listing {
  provider: string
  symbol: string
  exchange: string | null
  currency: string
  is_primary: boolean
}

export interface LatestPrice {
  date: string
  close: string
  currency: string
  source: string
  as_of: string
  checked_at: string
  stale: boolean
}

export interface Instrument {
  id: number
  kind: string
  name: string
  isin: string | null
  currency: string
  asset_class: string
  region: string
  created_at: string
  listings: Listing[]
  latest_price: LatestPrice | null
}

export interface LookupHit {
  provider: string
  symbol: string
  name: string
  kind: string
  exchange: string | null
  instrument_id: number | null
}

export interface Lookup {
  isin: string | null
  results: LookupHit[]
}

export const TRANSACTION_KINDS = [
  'buy',
  'sell',
  'dividend',
  'fee',
  'deposit',
  'withdrawal',
  'interest',
  'split',
  'transfer_in',
  'transfer_out',
] as const

export type TransactionKind = (typeof TRANSACTION_KINDS)[number]

export const ACCOUNT_KINDS = [
  'book_entry',
  'fund',
  'equity_savings',
  'savings',
  'cash',
  'other',
] as const

/** Whether the engine pairs this kind with an instrument. */
export function kindNeedsInstrument(kind: TransactionKind): boolean {
  return !['fee', 'deposit', 'withdrawal', 'interest'].includes(kind)
}

/** Whether the kind carries units (and so a quantity). */
export function kindNeedsQuantity(kind: TransactionKind): boolean {
  return ['buy', 'sell', 'split', 'transfer_in', 'transfer_out'].includes(kind)
}

/** Whether the kind carries a unit price as well as a quantity. */
export function kindNeedsPrice(kind: TransactionKind): boolean {
  return kind === 'buy' || kind === 'sell'
}

/** Whether the kind takes a price; an unpaired transfer in needs one for its basis. */
export function kindAllowsPrice(kind: TransactionKind): boolean {
  return kindNeedsPrice(kind) || kind === 'transfer_in'
}

/** Whether the kind carries a cash amount instead of units. */
export function kindNeedsAmount(kind: TransactionKind): boolean {
  return !kindNeedsQuantity(kind)
}

/** Transfer and split rows take no fee. */
export function kindAllowsFee(kind: TransactionKind): boolean {
  return !['split', 'transfer_in', 'transfer_out'].includes(kind)
}

/** A money figure through Intl; `—` when there is none. */
export function formatMoney(text: string | null | undefined, currency: string): string {
  if (text === null || text === undefined || text === '') return '—'
  const value = Number(text)
  if (!Number.isFinite(value)) return text
  return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(value)
}

/** Units through Intl, without trailing zeroes. */
export function formatQuantity(text: string): string {
  const value = Number(text)
  if (!Number.isFinite(value)) return text
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 8 }).format(value)
}

/** An account's name, or its id when the list does not hold it. */
export function accountName(accounts: Account[] | null, id: number): string {
  return accounts?.find((account) => account.id === id)?.name ?? `#${id}`
}

/** The market value of a holding, in the instrument's currency. */
export function holdingValue(holding: Holding, instrument: Instrument | undefined): number | null {
  const price = instrument?.latest_price?.close
  if (price === undefined) return null
  const value = Number(holding.quantity) * Number(price)
  return Number.isFinite(value) ? value : null
}
