/** Repeat rules the server accepts: `FREQ=DAILY|WEEKLY|MONTHLY|YEARLY;INTERVAL=n` (the web's `features/tasks/recurrence.ts`). */
export type RecurrenceFrequency = 'DAILY' | 'WEEKLY' | 'MONTHLY' | 'YEARLY'

/** The choices of the task's Repeat sheet, in order; `null` stops the task from repeating. */
export const RECURRENCE_CHOICES: readonly { rule: string | null; label: string }[] = [
  { rule: null, label: "Doesn't repeat" },
  { rule: 'FREQ=DAILY;INTERVAL=1', label: 'Every day' },
  { rule: 'FREQ=WEEKLY;INTERVAL=1', label: 'Every week' },
  { rule: 'FREQ=WEEKLY;INTERVAL=2', label: 'Every 2 weeks' },
  { rule: 'FREQ=MONTHLY;INTERVAL=1', label: 'Every month' },
  { rule: 'FREQ=MONTHLY;INTERVAL=3', label: 'Every 3 months' },
  { rule: 'FREQ=YEARLY;INTERVAL=1', label: 'Every year' },
]

const UNIT: Record<RecurrenceFrequency, string> = { DAILY: 'day', WEEKLY: 'week', MONTHLY: 'month', YEARLY: 'year' }

export function parseRecurrence(rule: string | null | undefined): { frequency: RecurrenceFrequency; interval: number } | null {
  if (!rule) return null
  const parts = Object.fromEntries(rule.split(';').map((part) => part.split('=') as [string, string]))
  const frequency = parts.FREQ as RecurrenceFrequency | undefined
  const interval = Number(parts.INTERVAL ?? 1)
  if (!frequency || !(frequency in UNIT) || !Number.isInteger(interval) || interval < 1) return null
  return { frequency, interval }
}

/** "Every week", "Every 2 weeks"; "Doesn't repeat" without a rule. */
export function recurrenceLabel(rule: string | null | undefined): string {
  const parsed = parseRecurrence(rule)
  if (!parsed) return "Doesn't repeat"
  const unit = UNIT[parsed.frequency]
  return parsed.interval === 1 ? `Every ${unit}` : `Every ${parsed.interval} ${unit}s`
}
