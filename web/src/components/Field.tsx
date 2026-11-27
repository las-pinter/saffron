import { useId, type ComponentProps } from 'react'

interface FieldProps extends ComponentProps<'input'> {
  label: string
  hint?: string
}

/** A labelled text input; the hint is tied to the input for screen readers. */
export function Field({ label, hint, id, className = '', ...props }: FieldProps) {
  const generated = useId()
  const inputId = id ?? generated
  const hintId = hint ? `${inputId}-hint` : undefined

  return (
    <div>
      <label htmlFor={inputId} className="block text-sm font-medium text-sf-text-secondary">
        {label}
      </label>
      <input
        id={inputId}
        aria-describedby={hintId}
        className={`mt-1.5 h-11 w-full rounded-lg border border-sf-border bg-sf-surface-sunken px-3 text-foreground placeholder:text-sf-text-faint ${className}`}
        {...props}
      />
      {hint ? (
        <p id={hintId} className="mt-1 text-xs text-sf-text-muted">
          {hint}
        </p>
      ) : null}
    </div>
  )
}

interface CheckFieldProps extends ComponentProps<'input'> {
  label: string
}

/** A checkbox with its label read as one control. */
export function CheckField({ label, id, className = '', ...props }: CheckFieldProps) {
  const generated = useId()
  const inputId = id ?? generated

  return (
    <label
      htmlFor={inputId}
      className="flex min-h-11 items-center gap-2 text-sm text-sf-text-secondary sm:min-h-0"
    >
      <input
        id={inputId}
        type="checkbox"
        className={`size-4 accent-sf-accent ${className}`}
        {...props}
      />
      {label}
    </label>
  )
}
