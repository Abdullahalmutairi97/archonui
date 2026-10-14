//! Sidebar grouping and relative times (layout.md §2.1). Pure functions over a `now` the caller
//! passes in, so the sidebar can re-render on a timer and tests can use fixed times.

use crate::store::Session;
use std::cmp::Reverse;

const DAY: u64 = 86_400;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// Last used on the current local calendar day.
    Today,
    Earlier,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Today => "TODAY",
            Section::Earlier => "EARLIER",
        }
    }
}

/// Sessions grouped as TODAY then EARLIER, most recently used first within each group. Empty
/// groups are dropped.
pub fn sections(sessions: &[Session], now: u64) -> Vec<(Section, Vec<&Session>)> {
    sections_at(sessions, now, local_offset(now))
}

/// Relative time of `ts` as seen at `now`: `now`, `4m`, `1h`, `1d`, a weekday for 2–6 days ago,
/// then `Nd`.
pub fn ago(ts: u64, now: u64) -> String {
    ago_at(ts, now, local_offset(now))
}

/// `sections` with the UTC offset given, in seconds.
pub fn sections_at(sessions: &[Session], now: u64, offset: i64) -> Vec<(Section, Vec<&Session>)> {
    let today = day_index(now, offset);
    let mut recent: Vec<&Session> = sessions.iter().collect();
    recent.sort_by_key(|s| Reverse(s.updated));
    let (mut section_today, mut earlier) = (Vec::new(), Vec::new());
    for s in recent {
        if day_index(s.updated, offset) == today {
            section_today.push(s);
        } else {
            earlier.push(s);
        }
    }
    [(Section::Today, section_today), (Section::Earlier, earlier)]
        .into_iter()
        .filter(|(_, items)| !items.is_empty())
        .collect()
}

/// `ago` with the UTC offset given, in seconds.
pub fn ago_at(ts: u64, now: u64, offset: i64) -> String {
    let elapsed = now.saturating_sub(ts);
    if elapsed < 60 {
        return "now".into();
    }
    if elapsed < 3600 {
        return format!("{}m", elapsed / 60);
    }
    match day_index(now, offset) - day_index(ts, offset) {
        0 => format!("{}h", elapsed / 3600),
        1 => "1d".into(),
        2..=6 => WEEKDAYS[weekday(ts, offset)].into(),
        days => format!("{days}d"),
    }
}

/// Days since the epoch on the local calendar, for a UTC offset in seconds.
fn day_index(ts: u64, offset: i64) -> i64 {
    (ts as i64 + offset).div_euclid(DAY as i64)
}

/// Sunday is 0. 1970-01-01 was a Thursday.
fn weekday(ts: u64, offset: i64) -> usize {
    (day_index(ts, offset) + 4).rem_euclid(7) as usize
}

/// The local UTC offset at `now`, in seconds. UTC where the platform offers no `tm_gmtoff`.
fn local_offset(now: u64) -> i64 {
    #[cfg(unix)]
    {
        let t = now as libc::time_t;
        // SAFETY: `localtime_r` writes into `tm`, which is zeroed and lives for the call.
        unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                return 0;
            }
            tm.tm_gmtoff as i64
        }
    }
    #[cfg(not(unix))]
    {
        let _ = now;
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Day 10 since the epoch is a Sunday; now is noon that day.
    const NOW: u64 = 10 * DAY + 12 * 3600;

    fn session(id: &str, updated: u64) -> Session {
        let mut s = Session::new("pi", "/tmp".into());
        s.id = id.into();
        s.updated = updated;
        s
    }

    #[test]
    fn relative_times() {
        let at = |secs_ago: u64| ago_at(NOW - secs_ago, NOW, 0);
        assert_eq!(at(0), "now");
        assert_eq!(at(59), "now");
        assert_eq!(at(4 * 60), "4m");
        assert_eq!(at(3600), "1h");
        assert_eq!(at(11 * 3600), "11h", "same day, in hours");
        assert_eq!(ago_at(9 * DAY + 23 * 3600, NOW, 0), "1d", "yesterday");
        assert_eq!(ago_at(8 * DAY + 12 * 3600, NOW, 0), "Fri", "two days ago is a weekday");
        assert_eq!(ago_at(4 * DAY, NOW, 0), "Mon", "six days ago");
        assert_eq!(ago_at(3 * DAY, NOW, 0), "7d", "a week and more is a day count");
    }

    #[test]
    fn future_timestamps_read_as_now() {
        assert_eq!(ago_at(NOW + 500, NOW, 0), "now");
    }

    #[test]
    fn today_follows_the_local_day() {
        // 22:00 UTC on day 9 is 01:00 on day 10 at UTC+3.
        let ts = 9 * DAY + 22 * 3600;
        assert_eq!(ago_at(ts, NOW, 0), "1d");
        assert_eq!(ago_at(ts, NOW, 3 * 3600), "14h");
        let sessions = [session("late", ts), session("now", NOW)];
        let groups = sections_at(&sessions, NOW, 3 * 3600);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, Section::Today);
        let groups = sections_at(&sessions, NOW, 0);
        assert_eq!(groups[0].0, Section::Today);
        assert_eq!(groups[0].1[0].id, "now");
        assert_eq!(groups[1].0, Section::Earlier);
    }

    #[test]
    fn grouping_sorts_by_last_use_and_drops_empty_groups() {
        let sessions = [session("b", NOW - 3600), session("c", DAY), session("a", NOW - 60)];
        let groups = sections_at(&sessions, NOW, 0);
        let names: Vec<_> = groups.iter().map(|(s, v)| (s.label(), v.iter().map(|x| x.id.as_str()).collect::<Vec<_>>())).collect();
        assert_eq!(names, [("TODAY", vec!["a", "b"]), ("EARLIER", vec!["c"])]);

        let only_today = [session("a", NOW - 60)];
        assert_eq!(sections_at(&only_today, NOW, 0).len(), 1);
        assert!(sections_at(&[], NOW, 0).is_empty());
    }
}
