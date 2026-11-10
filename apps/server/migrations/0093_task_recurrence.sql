-- Recurring tasks: an RFC 5545 RRULE subset (`FREQ=DAILY|WEEKLY|MONTHLY|YEARLY;INTERVAL=n`). When a
-- task with a rule moves into a completed status, the server creates the next occurrence with the
-- due dates moved forward and hands the rule over to it (this task's rule is cleared), so reopening
-- and completing the old task again never makes a second copy. `recurrence_of` points from each
-- occurrence to the one it was created from.
ALTER TABLE tasks ADD COLUMN recurrence TEXT;
ALTER TABLE tasks ADD COLUMN recurrence_of TEXT REFERENCES tasks(id) ON DELETE SET NULL;
