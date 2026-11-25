import { useTranslation } from 'react-i18next'

export function Placeholder() {
  const { t } = useTranslation()

  return (
    <section className="rounded-2xl border border-border bg-sf-surface p-7">
      <h1 className="font-heading text-3xl font-semibold tracking-tight">
        {t('placeholder.title')}
      </h1>
      <p className="mt-2 text-sf-text-muted">{t('placeholder.body')}</p>
    </section>
  )
}
