import { expect, test } from 'bun:test'

import { parseRecurrence, RECURRENCE_CHOICES, recurrenceLabel } from './recurrence'

test('recurrenceLabel names the rules the server returns', () => {
  expect(recurrenceLabel(null)).toBe("Doesn't repeat")
  expect(recurrenceLabel('FREQ=WEEKLY;INTERVAL=1')).toBe('Every week')
  expect(recurrenceLabel('FREQ=DAILY;INTERVAL=3')).toBe('Every 3 days')
  expect(recurrenceLabel('FREQ=MONTHLY')).toBe('Every month')
})

test('parseRecurrence rejects rules outside the supported subset', () => {
  expect(parseRecurrence('FREQ=HOURLY;INTERVAL=1')).toBeNull()
  expect(parseRecurrence('FREQ=DAILY;INTERVAL=0')).toBeNull()
  expect(parseRecurrence('')).toBeNull()
})

test('every choice has a matching label', () => {
  for (const choice of RECURRENCE_CHOICES) expect(recurrenceLabel(choice.rule)).toBe(choice.label)
})
