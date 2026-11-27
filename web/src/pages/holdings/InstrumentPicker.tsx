import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ErrorText } from '@/components/Feedback'
import { Field } from '@/components/Field'
import { Button } from '@/components/ui/button'
import { api } from '@/lib/api'
import type { Instrument, Lookup, LookupHit } from '@/lib/book'
import { useErrorMessage } from '@/lib/errors'

export interface PickedInstrument {
  id: number
  name: string
  currency: string
}

interface InstrumentPickerProps {
  value: PickedInstrument | null
  onPick: (instrument: PickedInstrument | null) => void
}

/** Finds an instrument by ticker, ISIN, or name, adding it to Saffron if new. */
export function InstrumentPicker({ value, onPick }: InstrumentPickerProps) {
  const { t } = useTranslation()
  const messageOf = useErrorMessage()
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<LookupHit[] | null>(null)
  const [isin, setIsin] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function search() {
    setBusy(true)
    setError(null)
    try {
      const answer = await api.get<Lookup>(
        `/instruments/lookup?q=${encodeURIComponent(query.trim())}`,
      )
      setResults(answer.results.slice(0, 8))
      setIsin(answer.isin)
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  async function choose(hit: LookupHit) {
    setBusy(true)
    setError(null)
    try {
      // A hit Saffron does not know yet is added first, so the transaction can name it.
      const view = hit.instrument_id
        ? await api.get<Instrument>(`/instruments/${hit.instrument_id}`)
        : await api.post<Instrument>('/instruments', {
            symbol: hit.symbol,
            ...(isin ? { isin } : {}),
          })
      onPick({ id: view.id, name: view.name, currency: view.currency })
      setResults(null)
      setQuery('')
    } catch (failure) {
      setError(messageOf(failure))
    } finally {
      setBusy(false)
    }
  }

  if (value) {
    return (
      <div className="rounded-lg border border-sf-border bg-sf-surface-sunken px-3 py-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <p className="min-w-0 text-sm break-words">
            {value.name} <span className="text-xs text-sf-text-muted">({value.currency})</span>
          </p>
          <Button type="button" variant="ghost" onClick={() => onPick(null)}>
            {t('holdings.instrument.change')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-0 flex-1">
          <Field
            label={t('holdings.instrument.search')}
            name="instrument_search"
            hint={t('holdings.instrument.searchHint')}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                // The picker sits inside the transaction form; Enter must search here.
                event.preventDefault()
                void search()
              }
            }}
          />
        </div>
        <Button type="button" onClick={() => void search()} disabled={busy || query.trim() === ''}>
          {busy ? t('holdings.instrument.searching') : t('holdings.instrument.find')}
        </Button>
      </div>
      {error ? <ErrorText>{error}</ErrorText> : null}
      {results?.length === 0 ? (
        <p className="text-sm text-sf-text-muted">{t('holdings.instrument.none')}</p>
      ) : null}
      {results && results.length > 0 ? (
        <ul
          aria-live="polite"
          className="divide-y divide-sf-divider rounded-lg border border-sf-border"
        >
          {results.map((hit) => (
            <li key={`${hit.provider}:${hit.symbol}`}>
              <button
                type="button"
                disabled={busy}
                onClick={() => choose(hit)}
                className="flex min-h-11 w-full flex-wrap items-baseline justify-between gap-2 px-3 py-2 text-left text-sm hover:bg-sf-surface-sunken"
              >
                <span className="min-w-0 break-words">{hit.name}</span>
                <span className="text-xs text-sf-text-muted">
                  {hit.symbol}
                  {hit.exchange ? ` · ${hit.exchange}` : ''} · {t(`instrumentKind.${hit.kind}`)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  )
}
