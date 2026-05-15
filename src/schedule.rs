use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, Utc};
use cron::Schedule;
use std::str::FromStr;

/// Returns true iff there is at least one scheduled fire time `T` in the
/// interval `(last_run_at, now - grace]` — i.e. the schedule should have
/// fired at least once since we last saw it run, ignoring the grace window
/// that protects against racing cron's own imminent fire.
pub fn is_missed(
    expr: &str,
    last_run_at: DateTime<Utc>,
    now: DateTime<Utc>,
    grace: Duration,
) -> Result<bool> {
    let schedule = parse(expr)?;
    let deadline = now - grace;
    if deadline <= last_run_at {
        return Ok(false);
    }
    let after_local = last_run_at.with_timezone(&Local);
    let deadline_local = deadline.with_timezone(&Local);
    Ok(schedule
        .after(&after_local)
        .next()
        .is_some_and(|fire| fire <= deadline_local))
}

fn parse(expr: &str) -> Result<Schedule> {
    // POSIX cron is 5-field (minute hour dom month dow). The `cron` crate
    // wants 6 fields with leading seconds — prepend `0 `.
    let full = format!("0 {}", expr.trim());
    Schedule::from_str(&full).with_context(|| format!("invalid cron expression: {expr}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(s: &str) -> DateTime<Utc> {
        // Parse a local-time wall clock and convert to UTC, so tests read
        // naturally regardless of where they run.
        let naive = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .expect("bad test timestamp");
        Local
            .from_local_datetime(&naive)
            .single()
            .expect("ambiguous local time in test fixture")
            .with_timezone(&Utc)
    }

    #[test]
    fn missed_when_fire_passed_outside_grace() {
        // Every hour on the hour. Last run was at 08:30, now is 10:05.
        // 09:00 is a scheduled fire that we missed.
        let last = utc("2026-05-15 08:30:00");
        let now = utc("2026-05-15 10:05:00");
        assert!(is_missed("0 * * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn not_missed_when_within_grace() {
        // 9am daily; now is 09:00:30 (30s after the fire). Grace 90s →
        // deadline is 08:59:00, which is before the 09:00 fire, so no miss.
        let last = utc("2026-05-14 09:00:00");
        let now = utc("2026-05-15 09:00:30");
        assert!(!is_missed("0 9 * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn missed_when_just_past_grace() {
        // 9am daily; now is 09:02 (120s after the fire). Grace 90s →
        // deadline is 09:00:30, which is after the 09:00 fire, so missed.
        let last = utc("2026-05-14 09:00:00");
        let now = utc("2026-05-15 09:02:00");
        assert!(is_missed("0 9 * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn not_missed_when_last_run_after_fire() {
        // Daily at 9am. Last run was at 09:01 today, now is 10:00. The 09:00
        // fire is before last_run, so not missed.
        let last = utc("2026-05-15 09:01:00");
        let now = utc("2026-05-15 10:00:00");
        assert!(!is_missed("0 9 * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn not_missed_when_no_fire_in_interval() {
        // Daily at 9am. Last run yesterday at 09:01, now is today 08:00 —
        // no 9am fire has occurred yet today.
        let last = utc("2026-05-14 09:01:00");
        let now = utc("2026-05-15 08:00:00");
        assert!(!is_missed("0 9 * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn missed_when_long_offline() {
        // Daily at 9am. Last run 5 days ago. Should detect a miss
        // immediately (any one of the intervening fires).
        let last = utc("2026-05-10 09:01:00");
        let now = utc("2026-05-15 10:00:00");
        assert!(is_missed("0 9 * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn every_minute_schedule() {
        // `* * * * *` — every minute. Last run 5 minutes ago, now is now.
        let last = utc("2026-05-15 10:00:00");
        let now = utc("2026-05-15 10:05:00");
        assert!(is_missed("* * * * *", last, now, Duration::seconds(90)).unwrap());
    }

    #[test]
    fn invalid_expression_errors() {
        let last = utc("2026-05-15 10:00:00");
        let now = utc("2026-05-15 11:00:00");
        assert!(is_missed("not a cron expr", last, now, Duration::seconds(90)).is_err());
    }
}
