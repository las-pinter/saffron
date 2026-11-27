import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

interface ConfirmButtonProps {
  label: string
  confirmLabel: string
  /** Accessible names that name the row, when one label would serve several rows. */
  labelFor?: string
  confirmLabelFor?: string
  busy?: boolean
  onConfirm: () => void | Promise<void>
}

/** A button that asks once before doing something that cannot be undone here. */
export function ConfirmButton({
  label,
  confirmLabel,
  labelFor,
  confirmLabelFor,
  busy = false,
  onConfirm,
}: ConfirmButtonProps) {
  const { t } = useTranslation()
  const [asking, setAsking] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const confirm = useRef<HTMLButtonElement>(null)
  const wasAsking = useRef(false)

  // The button that had focus unmounts when the question appears, so move focus to the
  // confirm control. On cancel, give it back unless the caller moved focus elsewhere.
  useEffect(() => {
    if (asking) {
      confirm.current?.focus()
    } else if (wasAsking.current && document.activeElement === document.body) {
      trigger.current?.focus()
    }
    wasAsking.current = asking
  }, [asking])

  if (!asking) {
    return (
      <Button
        ref={trigger}
        type="button"
        variant="ghost"
        aria-label={labelFor ?? label}
        onClick={() => setAsking(true)}
      >
        {label}
      </Button>
    )
  }

  return (
    <span className="inline-flex items-center gap-1">
      <Button
        ref={confirm}
        type="button"
        variant="destructive"
        aria-label={confirmLabelFor ?? confirmLabel}
        disabled={busy}
        onClick={async () => {
          try {
            await onConfirm()
          } finally {
            setAsking(false)
          }
        }}
      >
        {confirmLabel}
      </Button>
      <Button type="button" variant="ghost" onClick={() => setAsking(false)}>
        {t('common.cancel')}
      </Button>
    </span>
  )
}
