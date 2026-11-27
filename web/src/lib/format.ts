/** Dates as the interface shows them, through Intl so every locale works. */

/** A calendar day (YYYY-MM-DD) in the interface's locale. */
export function formatDate(day: string): string {
  const [year = Number.NaN, month = Number.NaN, date = Number.NaN] = day.split('-').map(Number)
  if (!Number.isInteger(year) || !Number.isInteger(month) || !Number.isInteger(date)) return day
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(
    new Date(year, month - 1, date),
  )
}

export function formatDateTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(
    date,
  )
}
