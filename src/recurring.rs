use chrono::NaiveDate;

use crate::model::RecurringTask;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RecurringSchedule {
    pub(crate) elapsed_days: i64,
    pub(crate) available_after_days: i64,
    pub(crate) due_after_days: i64,
    pub(crate) eligible: bool,
    pub(crate) wheel_weight: f64,
}

impl RecurringSchedule {
    pub(crate) fn days_until_available(self) -> i64 {
        self.available_after_days - self.elapsed_days
    }

    pub(crate) fn days_until_due(self) -> i64 {
        self.due_after_days - self.elapsed_days
    }
}

pub(crate) fn recurring_schedule(task: &RecurringTask, today: &str) -> RecurringSchedule {
    let start = parse_iso_date(&task.cycle_started_on);
    let current = parse_iso_date(today).or(start);
    let elapsed_days = match (start, current) {
        (Some(start), Some(current)) => (current - start).num_days().max(0),
        _ => 0,
    };
    let available_after_days = i64::from(task.available_after_days);
    let due_after_days = i64::from(task.due_after_days.max(task.available_after_days));
    let has_been_completed = task.completed_count > 0 || task.last_completed_at.is_some();
    let eligible = !has_been_completed || elapsed_days >= available_after_days;
    let wheel_weight = if !eligible {
        0.0
    } else if elapsed_days >= due_after_days {
        100.0
    } else {
        let window_days = (due_after_days - available_after_days).max(1) as f64;
        let progress = ((elapsed_days - available_after_days) as f64 / window_days).clamp(0.0, 1.0);
        1.0 + 24.0 * progress * progress
    };

    RecurringSchedule {
        elapsed_days,
        available_after_days,
        due_after_days,
        eligible,
        wheel_weight,
    }
}

pub(crate) fn recurring_schedule_label(schedule: RecurringSchedule) -> String {
    let due_in = schedule.days_until_due();
    if !schedule.eligible {
        return format!(
            "Appears in {} | due in {}",
            format_days(schedule.days_until_available()),
            format_days(due_in.max(0))
        );
    }
    if due_in < 0 {
        return format!(
            "Overdue by {} | very high wheel chance",
            format_days(-due_in)
        );
    }
    if due_in == 0 {
        return "Due today | very high wheel chance".to_owned();
    }

    let chance = if schedule.wheel_weight >= 12.0 {
        "high"
    } else if schedule.wheel_weight >= 4.0 {
        "rising"
    } else {
        "normal"
    };
    format!(
        "In wheel | due in {} | {chance} chance",
        format_days(due_in)
    )
}

pub(crate) fn format_days(days: i64) -> String {
    if days == 1 {
        "1 day".to_owned()
    } else {
        format!("{days} days")
    }
}

pub(crate) fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recurring_task_for_test() -> RecurringTask {
        RecurringTask {
            id: 1,
            title: "Cut grass".to_owned(),
            available_after_days: 7,
            due_after_days: 14,
            cycle_started_on: "2026-01-01".to_owned(),
            created_at: "2026-01-01 09:00:00".to_owned(),
            last_completed_at: None,
            completed_count: 0,
            picked_count: 0,
        }
    }

    #[test]
    fn new_recurring_task_enters_wheel_immediately() {
        let task = recurring_task_for_test();
        let created = recurring_schedule(&task, "2026-01-01");
        let early = recurring_schedule(&task, "2026-01-07");

        assert!(created.eligible);
        assert_eq!(created.wheel_weight, 1.0);
        assert!(early.eligible);
        assert_eq!(early.wheel_weight, 1.0);
        assert_eq!(
            recurring_schedule_label(created),
            "In wheel | due in 14 days | normal chance"
        );
    }

    #[test]
    fn completed_recurring_task_waits_then_gains_deadline_weight() {
        let mut task = recurring_task_for_test();
        task.last_completed_at = Some("2026-01-01 09:00:00".to_owned());
        task.completed_count = 1;

        let early = recurring_schedule(&task, "2026-01-07");
        let available = recurring_schedule(&task, "2026-01-08");
        let approaching = recurring_schedule(&task, "2026-01-12");
        let due = recurring_schedule(&task, "2026-01-15");

        assert!(!early.eligible);
        assert_eq!(early.wheel_weight, 0.0);
        assert!(available.eligible);
        assert_eq!(available.wheel_weight, 1.0);
        assert!(approaching.wheel_weight > available.wheel_weight);
        assert!(approaching.wheel_weight < due.wheel_weight);
        assert_eq!(due.wheel_weight, 100.0);
    }
}
