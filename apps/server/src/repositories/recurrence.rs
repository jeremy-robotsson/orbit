//! Recurring tasks. A task's `recurrence` is an RFC 5545 RRULE subset:
//! `FREQ=DAILY|WEEKLY|MONTHLY|YEARLY` with an optional `INTERVAL=n` (1 to 999).
//!
//! When a recurring task moves into a `completed` status, [`spawn_next_in_tx`] creates the next
//! occurrence in the same transaction: same project, title, description, priority, assignees and
//! labels, back in the unstarted status it was completed from (else the project's first open
//! status), with the due dates moved to the next date of
//! the series that is still in the future. The rule moves to the new task (the completed one keeps
//! none), so reopening and completing the old task again never makes a second copy. A cancelled
//! task ends its series.
//!
//! Dates are moved in UTC calendar steps; a monthly rule anchored on the 31st lands on the last
//! day of shorter months without drifting (each step is counted from the original due date).

use chrono::{DateTime, Days, Months, Utc};
use orbit_platform::{Id, TimestampMillis};
use sqlx::{Any, Transaction};

use super::tasks::{TaskError, TaskRecord, parse_id};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recurrence {
    pub frequency: Frequency,
    pub interval: u32,
}

const MAX_INTERVAL: u32 = 999;

impl Recurrence {
    /// Parses a rule such as `FREQ=WEEKLY;INTERVAL=2` (parts in any order, `INTERVAL` optional).
    /// Returns `None` for anything outside the supported subset.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let value = match value.get(..6) {
            Some(prefix) if prefix.eq_ignore_ascii_case("RRULE:") => &value[6..],
            _ => value,
        };
        let mut frequency = None;
        let mut interval = None;
        for part in value.split(';').filter(|part| !part.is_empty()) {
            let (key, value) = part.split_once('=')?;
            match key.trim().to_ascii_uppercase().as_str() {
                "FREQ" if frequency.is_none() => {
                    frequency = Some(match value.trim().to_ascii_uppercase().as_str() {
                        "DAILY" => Frequency::Daily,
                        "WEEKLY" => Frequency::Weekly,
                        "MONTHLY" => Frequency::Monthly,
                        "YEARLY" => Frequency::Yearly,
                        _ => return None,
                    });
                }
                "INTERVAL" if interval.is_none() => {
                    let value = value.trim();
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return None;
                    }
                    interval = Some(value.parse::<u32>().ok()?);
                }
                _ => return None,
            }
        }
        let interval = interval.unwrap_or(1);
        (1..=MAX_INTERVAL).contains(&interval).then_some(Self {
            frequency: frequency?,
            interval,
        })
    }

    /// The canonical rule text stored in `tasks.recurrence`.
    #[must_use]
    pub fn to_rule(self) -> String {
        let frequency = match self.frequency {
            Frequency::Daily => "DAILY",
            Frequency::Weekly => "WEEKLY",
            Frequency::Monthly => "MONTHLY",
            Frequency::Yearly => "YEARLY",
        };
        format!("FREQ={frequency};INTERVAL={}", self.interval)
    }

    /// `anchor` moved forward by `steps` intervals; `None` past the calendar's range.
    fn advance(self, anchor: TimestampMillis, steps: u32) -> Option<TimestampMillis> {
        let date = DateTime::<Utc>::from_timestamp_millis(anchor.as_millis())?;
        let count = steps.checked_mul(self.interval)?;
        let moved = match self.frequency {
            Frequency::Daily => date.checked_add_days(Days::new(u64::from(count)))?,
            Frequency::Weekly => date.checked_add_days(Days::new(u64::from(count) * 7))?,
            Frequency::Monthly => date.checked_add_months(Months::new(count))?,
            Frequency::Yearly => date.checked_add_months(Months::new(count.checked_mul(12)?))?,
        };
        Some(TimestampMillis::from_millis(moved.timestamp_millis()))
    }

    /// The first date of the series that starts at `anchor` and lies after `now` (at least one
    /// step after `anchor`). A task completed late therefore skips the dates it missed.
    #[must_use]
    pub fn next_after(
        self,
        anchor: TimestampMillis,
        now: TimestampMillis,
    ) -> Option<TimestampMillis> {
        // Daily and weekly steps have a fixed length: jump close to `now` first.
        let mut steps = match self.frequency {
            Frequency::Daily | Frequency::Weekly => {
                let unit = if self.frequency == Frequency::Daily {
                    1
                } else {
                    7
                };
                let period = i64::from(self.interval) * unit * 86_400_000;
                let behind = now.as_millis().saturating_sub(anchor.as_millis()).max(0);
                u32::try_from(behind / period).ok()?.max(1)
            }
            Frequency::Monthly | Frequency::Yearly => 1,
        };
        // Bounded: a monthly series 1000 years behind still ends here.
        for _ in 0..12_000 {
            let next = self.advance(anchor, steps)?;
            if next.as_millis() > now.as_millis() {
                return Some(next);
            }
            steps = steps.checked_add(1)?;
        }
        None
    }
}

/// The first open status of the project for a new occurrence: the first `unstarted` one, else the
/// first `started` one.
async fn open_status_in_tx(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    project_id: Id,
) -> Result<Option<Id>, TaskError> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM task_statuses WHERE workspace_id = $1 AND project_id = $2 \
         AND category IN ('unstarted', 'started') \
         ORDER BY CASE WHEN category = 'unstarted' THEN 0 ELSE 1 END, position, id LIMIT 1",
    )
    .bind(workspace_id.to_string())
    .bind(project_id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    id.map(parse_id).transpose()
}

/// What the next occurrence of `task` (just completed) looks like, or `None` when it has no rule
/// or its project has no open status.
pub(super) struct NextOccurrence {
    pub status_id: Id,
    pub recurrence: Recurrence,
    pub due_start_at: Option<TimestampMillis>,
    pub due_at: Option<TimestampMillis>,
}

/// `status_id`: the open status to use, else the project's first open one.
pub(super) async fn next_occurrence_in_tx(
    tx: &mut Transaction<'_, Any>,
    task: &TaskRecord,
    status_id: Option<Id>,
    now: TimestampMillis,
) -> Result<Option<NextOccurrence>, TaskError> {
    let Some(recurrence) = task.recurrence.as_deref().and_then(Recurrence::parse) else {
        return Ok(None);
    };
    let status_id = match status_id {
        Some(status_id) => status_id,
        None => match open_status_in_tx(tx, task.workspace_id, task.project_id).await? {
            Some(status_id) => status_id,
            None => return Ok(None),
        },
    };
    // Without a due date the series counts from the completion.
    let anchor = task.due_at.unwrap_or(now);
    let Some(due_at) = recurrence.next_after(anchor, now) else {
        return Ok(None);
    };
    let shift = due_at.as_millis() - anchor.as_millis();
    Ok(Some(NextOccurrence {
        status_id,
        recurrence,
        due_start_at: task
            .due_start_at
            .map(|start| TimestampMillis::from_millis(start.as_millis() + shift)),
        due_at: Some(due_at),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> TimestampMillis {
        TimestampMillis::from_millis(
            DateTime::parse_from_rfc3339(value)
                .unwrap()
                .timestamp_millis(),
        )
    }

    #[test]
    fn parses_the_supported_subset_only() {
        let weekly = Recurrence::parse("FREQ=WEEKLY;INTERVAL=2").unwrap();
        assert_eq!(weekly.frequency, Frequency::Weekly);
        assert_eq!(weekly.interval, 2);
        assert_eq!(Recurrence::parse("rrule:freq=daily").unwrap().interval, 1);
        assert_eq!(
            Recurrence::parse("INTERVAL=3;FREQ=MONTHLY")
                .unwrap()
                .to_rule(),
            "FREQ=MONTHLY;INTERVAL=3"
        );
        for invalid in [
            "",
            "FREQ=HOURLY",
            "INTERVAL=2",
            "FREQ=DAILY;INTERVAL=0",
            "FREQ=DAILY;INTERVAL=1000",
            "FREQ=DAILY;INTERVAL=-1",
            "FREQ=DAILY;BYDAY=MO",
            "FREQ=DAILY;FREQ=WEEKLY",
        ] {
            assert_eq!(Recurrence::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn next_date_is_one_step_ahead_when_done_on_time() {
        let weekly = Recurrence::parse("FREQ=WEEKLY").unwrap();
        assert_eq!(
            weekly.next_after(at("2026-11-10T09:00:00Z"), at("2026-11-09T20:00:00Z")),
            Some(at("2026-11-17T09:00:00Z"))
        );
    }

    #[test]
    fn late_completion_skips_missed_dates() {
        let every_two_days = Recurrence::parse("FREQ=DAILY;INTERVAL=2").unwrap();
        assert_eq!(
            every_two_days.next_after(at("2026-11-01T08:00:00Z"), at("2026-11-10T12:00:00Z")),
            Some(at("2026-11-11T08:00:00Z"))
        );
    }

    #[test]
    fn monthly_series_keep_the_anchor_day() {
        let monthly = Recurrence::parse("FREQ=MONTHLY").unwrap();
        let anchor = at("2027-01-31T10:00:00Z");
        assert_eq!(
            monthly.next_after(anchor, at("2027-01-31T11:00:00Z")),
            Some(at("2027-02-28T10:00:00Z"))
        );
        assert_eq!(
            monthly.next_after(anchor, at("2027-03-01T00:00:00Z")),
            Some(at("2027-03-31T10:00:00Z"))
        );
        let yearly = Recurrence::parse("FREQ=YEARLY").unwrap();
        assert_eq!(
            yearly.next_after(at("2028-02-29T00:00:00Z"), at("2028-03-01T00:00:00Z")),
            Some(at("2029-02-28T00:00:00Z"))
        );
    }
}
