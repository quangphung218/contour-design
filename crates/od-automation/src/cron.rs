//! Five-field cron schedules in local time: `minute hour day-of-month
//! month day-of-week`, with `*`, lists, ranges and steps, plus `@hourly`,
//! `@daily`, `@weekly` and `@monthly`. Day-of-month and day-of-week follow
//! Vixie cron: when both are restricted, either one matching is enough.

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDateTime, TimeZone, Timelike};
use std::fmt;
use std::str::FromStr;

/// Longest accepted expression.
const MAX_LEN: usize = 200;
/// `next_after` gives up after this many days (covers Feb 29 schedules).
const SEARCH_DAYS: i64 = 366 * 5;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid schedule: {0}")]
pub struct CronError(String);

/// A parsed schedule.
#[derive(Clone, PartialEq, Eq)]
pub struct Schedule {
    source: String,
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    days_any: bool,
    weekdays_any: bool,
}

impl fmt::Debug for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Schedule").field(&self.source).finish()
    }
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

/// Parses one field into a bit set over `min..=max`. `*` reports true.
fn field(raw: &str, min: u32, max: u32, name: &str) -> Result<(u64, bool), CronError> {
    let bad = || CronError(format!("{name} field {raw:?}"));
    let mut bits = 0u64;
    for part in raw.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().map_err(|_| bad())?),
            None => (part, 1),
        };
        if step == 0 || step > max {
            return Err(bad());
        }
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let a = a.parse::<u32>().map_err(|_| bad())?;
            let b = b.parse::<u32>().map_err(|_| bad())?;
            (a, b)
        } else {
            let v = range.parse::<u32>().map_err(|_| bad())?;
            // `5/15` means 5, 20, 35, 50.
            (v, if part.contains('/') { max } else { v })
        };
        if lo < min || hi > max || lo > hi {
            return Err(bad());
        }
        let mut v = lo;
        while v <= hi {
            bits |= 1 << v;
            v += step;
        }
    }
    Ok((bits, raw == "*"))
}

impl FromStr for Schedule {
    type Err = CronError;

    fn from_str(raw: &str) -> Result<Self, CronError> {
        let source = raw.trim();
        if source.is_empty() || source.len() > MAX_LEN {
            return Err(CronError("empty or too long".into()));
        }
        let expanded = match source {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            "@monthly" => "0 0 1 * *",
            other if other.starts_with('@') => return Err(CronError(format!("unknown {other}"))),
            other => other,
        };
        let parts: Vec<&str> = expanded.split_whitespace().collect();
        let [mi, h, dom, mo, dow] = parts[..] else {
            return Err(CronError(format!(
                "expected 5 fields (minute hour day month weekday), got {}",
                parts.len()
            )));
        };
        let (minutes, _) = field(mi, 0, 59, "minute")?;
        let (hours, _) = field(h, 0, 23, "hour")?;
        let (days, days_any) = field(dom, 1, 31, "day-of-month")?;
        let (months, _) = field(mo, 1, 12, "month")?;
        let (mut weekdays, weekdays_any) = field(dow, 0, 7, "day-of-week")?;
        // 7 is Sunday too.
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays | 1) & 0x7f;
        }
        let narrow = |v: u64| u32::try_from(v).map_err(|_| CronError("field overflow".into()));
        Ok(Self {
            source: source.to_owned(),
            minutes,
            hours: narrow(hours)?,
            days: narrow(days)?,
            months: u16::try_from(months).map_err(|_| CronError("field overflow".into()))?,
            weekdays: u8::try_from(weekdays).map_err(|_| CronError("field overflow".into()))?,
            days_any,
            weekdays_any,
        })
    }
}

impl Schedule {
    fn day_matches(&self, t: &NaiveDateTime) -> bool {
        let dom = self.days & (1 << t.day()) != 0;
        let dow = self.weekdays & (1 << t.weekday().num_days_from_sunday()) != 0;
        match (self.days_any, self.weekdays_any) {
            (true, true) => true,
            (false, true) => dom,
            (true, false) => dow,
            (false, false) => dom || dow,
        }
    }

    /// The first wall-clock minute strictly after `after` that matches.
    #[must_use]
    pub fn next_naive_after(&self, after: NaiveDateTime) -> Option<NaiveDateTime> {
        let start = after.with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
        let limit = start + Duration::days(SEARCH_DAYS);
        let mut t = start;
        while t < limit {
            if self.months & (1 << t.month()) == 0 {
                // First minute of next month.
                let (y, m) = if t.month() == 12 {
                    (t.year() + 1, 1)
                } else {
                    (t.year(), t.month() + 1)
                };
                t = chrono::NaiveDate::from_ymd_opt(y, m, 1)?.and_hms_opt(0, 0, 0)?;
                continue;
            }
            if !self.day_matches(&t) {
                t = (t.date() + Duration::days(1)).and_hms_opt(0, 0, 0)?;
                continue;
            }
            if self.hours & (1 << t.hour()) == 0 {
                t = t.with_minute(0)? + Duration::hours(1);
                continue;
            }
            if self.minutes & (1 << t.minute()) == 0 {
                t += Duration::minutes(1);
                continue;
            }
            return Some(t);
        }
        None
    }

    /// The next matching instant after `after` in local time. A minute
    /// skipped by a DST jump is skipped; a repeated one fires once.
    #[must_use]
    pub fn next_after(&self, after: DateTime<Local>) -> Option<DateTime<Local>> {
        let mut t = after.naive_local();
        for _ in 0..1000 {
            t = self.next_naive_after(t)?;
            match Local.from_local_datetime(&t) {
                LocalResult::Single(d) | LocalResult::Ambiguous(d, _) if d > after => {
                    return Some(d);
                }
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn next(expr: &str, after: &str) -> String {
        let s: Schedule = expr.parse().unwrap();
        s.next_naive_after(at(after))
            .unwrap()
            .format("%Y-%m-%d %H:%M")
            .to_string()
    }

    #[test]
    fn common_schedules() {
        assert_eq!(next("* * * * *", "2026-10-23 07:14"), "2026-10-23 07:15");
        assert_eq!(next("*/15 * * * *", "2026-10-23 07:14"), "2026-10-23 07:15");
        assert_eq!(next("*/15 * * * *", "2026-10-23 07:15"), "2026-10-23 07:30");
        assert_eq!(next("0 9 * * 1-5", "2026-10-23 09:00"), "2026-10-26 09:00"); // Fri → Mon
        assert_eq!(next("@daily", "2026-12-31 23:59"), "2027-01-01 00:00");
        assert_eq!(next("@weekly", "2026-10-23 07:14"), "2026-10-25 00:00");
        assert_eq!(
            next("30 8 1,15 * *", "2026-10-02 00:00"),
            "2026-10-15 08:30"
        );
        assert_eq!(next("0 0 29 2 *", "2026-03-01 00:00"), "2028-02-29 00:00");
        assert_eq!(next("5/20 * * * *", "2026-10-23 07:06"), "2026-10-23 07:25");
        assert_eq!(next("0 12 * * 7", "2026-10-23 07:14"), "2026-10-25 12:00");
        assert_eq!(next("0 0 13 * 5", "2026-10-24 00:00"), "2026-10-30 00:00"); // dom OR dow
    }

    #[test]
    fn bad_expressions_are_refused() {
        for bad in [
            "",
            "* * * *",
            "* * * * * *",
            "60 * * * *",
            "* 24 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "* * * * 8",
            "*/0 * * * *",
            "5-1 * * * *",
            "a * * * *",
            "@yearly-ish",
            "1,,2 * * * *",
            &"1,".repeat(150),
        ] {
            assert!(bad.parse::<Schedule>().is_err(), "{bad:?}");
        }
        assert!("0 30 31 2 *".parse::<Schedule>().is_err(), "hour 30");
        let never: Schedule = "0 0 31 2 *".parse().unwrap();
        assert_eq!(never.next_naive_after(at("2026-01-01 00:00")), None);
    }

    #[test]
    fn local_next_is_strictly_later() {
        let s: Schedule = "* * * * *".parse().unwrap();
        let now = Local::now();
        let n = s.next_after(now).unwrap();
        assert!(n > now && n - now <= Duration::seconds(60));
    }
}
