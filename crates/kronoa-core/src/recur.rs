//! A practical subset of RFC 5545 recurrence rules.
//!
//! Supported: FREQ=DAILY|WEEKLY|MONTHLY|YEARLY, INTERVAL, COUNT, UNTIL, BYDAY (plain weekdays
//! for WEEKLY/DAILY, plus `1MO`/`-1FR` style ordinals for MONTHLY) and BYMONTHDAY for MONTHLY.
//! Expansion happens in the event's wall-clock zone so 9:00 stays 9:00 across DST changes.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};

use crate::model::Event;
use crate::time::{self, Zone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    /// Inclusive end as an instant (UTC seconds).
    pub until: Option<i64>,
    /// (ordinal, weekday); ordinal 0 = every such weekday.
    pub byday: Vec<(i32, Weekday)>,
    pub bymonthday: Vec<i32>,
}

pub fn weekday_code(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// Parse an iCalendar date or date-time (`20261113`, `20261113T090000`, `…Z`) to an instant.
pub fn parse_ical_instant(v: &str) -> Option<i64> {
    let v = v.trim();
    if v.len() == 8 {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(|d| time::date_ts(d) + 86_399);
    }
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    let n = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    Some(if utc { n.and_utc().timestamp() } else { Zone::Local.to_ts(n) })
}

impl Rule {
    pub fn parse(s: &str) -> Option<Rule> {
        let s = s.trim().trim_start_matches("RRULE:");
        let mut rule = Rule { freq: Freq::Daily, interval: 1, count: None, until: None, byday: vec![], bymonthday: vec![] };
        let mut have_freq = false;
        for part in s.split(';') {
            let (k, v) = part.split_once('=')?;
            match k.to_ascii_uppercase().as_str() {
                "FREQ" => {
                    rule.freq = match v.to_ascii_uppercase().as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        _ => return None,
                    };
                    have_freq = true;
                }
                "INTERVAL" => rule.interval = v.parse::<u32>().ok().filter(|n| *n > 0).unwrap_or(1),
                "COUNT" => rule.count = v.parse().ok(),
                "UNTIL" => rule.until = parse_ical_instant(v),
                "BYDAY" => {
                    for d in v.split(',') {
                        let d = d.trim().to_ascii_uppercase();
                        if d.len() < 2 {
                            continue;
                        }
                        let (num, code) = d.split_at(d.len() - 2);
                        if let Some(w) = parse_weekday(code) {
                            rule.byday.push((num.parse().unwrap_or(0), w));
                        }
                    }
                }
                "BYMONTHDAY" => rule.bymonthday = v.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
                _ => {}
            }
        }
        have_freq.then_some(rule)
    }

    /// A short machine name for the UI: daily | weekdays | weekly | monthly | yearly | custom.
    pub fn kind(&self) -> &'static str {
        let weekdays = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri];
        if self.interval != 1 || self.count.is_some() || self.until.is_some() {
            return "custom";
        }
        match self.freq {
            Freq::Daily if self.byday.is_empty() => "daily",
            Freq::Weekly if self.byday.len() == 5 && weekdays.iter().all(|w| self.byday.contains(&(0, *w))) => "weekdays",
            Freq::Weekly if self.byday.len() <= 1 => "weekly",
            Freq::Monthly if self.byday.is_empty() && self.bymonthday.is_empty() => "monthly",
            Freq::Yearly => "yearly",
            _ => "custom",
        }
    }
}

/// The RRULE for a UI choice (`none` gives "").
pub fn rule_for_kind(kind: &str, start_weekday: Weekday) -> String {
    match kind {
        "daily" => "FREQ=DAILY".into(),
        "weekdays" => "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".into(),
        "weekly" => format!("FREQ=WEEKLY;BYDAY={}", weekday_code(start_weekday)),
        "monthly" => "FREQ=MONTHLY".into(),
        "yearly" => "FREQ=YEARLY".into(),
        _ => String::new(),
    }
}

fn nth_weekday(y: i32, m: u32, n: i32, w: Weekday) -> Option<NaiveDate> {
    if n > 0 {
        NaiveDate::from_weekday_of_month_opt(y, m, w, n as u8)
    } else {
        let last = NaiveDate::from_ymd_opt(y, m, time::days_in_month(y, m))?;
        let back = (last.weekday().num_days_from_monday() + 7 - w.num_days_from_monday()) % 7;
        let d = last - Duration::days(back as i64) - Duration::weeks((-n - 1) as i64);
        (d.month() == m).then_some(d)
    }
}

fn add_months(d: NaiveDate, months: i64) -> (i32, u32) {
    let total = d.year() as i64 * 12 + d.month0() as i64 + months;
    ((total.div_euclid(12)) as i32, (total.rem_euclid(12) + 1) as u32)
}

/// Candidate dates of one period (in order) for the rule, starting at period `k` from `first`.
fn period_dates(rule: &Rule, first: NaiveDate, k: i64) -> Vec<NaiveDate> {
    match rule.freq {
        Freq::Daily => {
            let d = first + Duration::days(k * rule.interval as i64);
            if rule.byday.is_empty() || rule.byday.iter().any(|(_, w)| *w == d.weekday()) {
                vec![d]
            } else {
                vec![]
            }
        }
        Freq::Weekly => {
            let week0 = first - Duration::days(first.weekday().num_days_from_monday() as i64);
            let week = week0 + Duration::weeks(k * rule.interval as i64);
            if rule.byday.is_empty() {
                vec![week + Duration::days(first.weekday().num_days_from_monday() as i64)]
            } else {
                let mut v: Vec<NaiveDate> =
                    rule.byday.iter().map(|(_, w)| week + Duration::days(w.num_days_from_monday() as i64)).collect();
                v.sort();
                v.dedup();
                v
            }
        }
        Freq::Monthly => {
            let (y, m) = add_months(first, k * rule.interval as i64);
            let mut v: Vec<NaiveDate> = if !rule.byday.is_empty() {
                rule.byday
                    .iter()
                    .flat_map(|(n, w)| {
                        if *n == 0 {
                            (1..=5).filter_map(|i| NaiveDate::from_weekday_of_month_opt(y, m, *w, i)).collect::<Vec<_>>()
                        } else {
                            nth_weekday(y, m, *n, *w).into_iter().collect()
                        }
                    })
                    .collect()
            } else {
                let days = if rule.bymonthday.is_empty() { vec![first.day() as i32] } else { rule.bymonthday.clone() };
                let dim = time::days_in_month(y, m) as i32;
                days.iter()
                    .filter_map(|d| {
                        let d = if *d < 0 { dim + 1 + d } else { *d };
                        NaiveDate::from_ymd_opt(y, m, d as u32)
                    })
                    .collect()
            };
            v.sort();
            v.dedup();
            v
        }
        Freq::Yearly => {
            let y = first.year() + (k * rule.interval as i64) as i32;
            NaiveDate::from_ymd_opt(y, first.month(), first.day()).into_iter().collect()
        }
    }
}

/// Start instants of every occurrence of `ev` that overlaps `[from, to)`.
///
/// Single events give their own start (when overlapping). Occurrences listed in `exdates`
/// or replaced by an override (`skip`) are left out.
pub fn occurrences(ev: &Event, from: i64, to: i64, skip: &[i64]) -> Vec<i64> {
    let len = (ev.end - ev.start).max(0);
    let rule = match (!ev.rrule.is_empty()).then(|| Rule::parse(&ev.rrule)).flatten() {
        Some(r) => r,
        None => {
            let end = if len == 0 { ev.start + 1 } else { ev.end };
            return if ev.start < to && end > from { vec![ev.start] } else { vec![] };
        }
    };
    let zone = if ev.all_day { None } else { Some(Zone::from_tzid(&ev.tzid)) };
    let start_naive = match zone {
        Some(z) => z.naive(ev.start),
        None => time::ts_date(ev.start).and_time(chrono::NaiveTime::MIN),
    };
    let first = start_naive.date();
    let tod = start_naive.time();
    let to_ts = |d: NaiveDate| match zone {
        Some(z) => z.to_ts(d.and_time(tod)),
        None => time::date_ts(d),
    };
    let mut out = vec![];
    let mut n: u32 = 0;
    'outer: for k in 0..20_000i64 {
        for d in period_dates(&rule, first, k) {
            if d < first {
                continue;
            }
            let ts = to_ts(d);
            if let Some(u) = rule.until {
                if ts > u {
                    break 'outer;
                }
            }
            n += 1;
            if let Some(c) = rule.count {
                if n > c {
                    break 'outer;
                }
            }
            if ts >= to {
                break 'outer;
            }
            if ts + len.max(1) > from && !ev.exdates.contains(&ts) && !skip.contains(&ts) {
                out.push(ts);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn ev(start: NaiveDateTime, mins: i64, rrule: &str) -> Event {
        let s = Zone::Local.to_ts(start);
        Event { start: s, end: s + mins * 60, rrule: rrule.into(), ..Default::default() }
    }
    fn dt(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_time(NaiveTime::from_hms_opt(h, 0, 0).unwrap())
    }
    fn range(a: NaiveDateTime, b: NaiveDateTime) -> (i64, i64) {
        (Zone::Local.to_ts(a), Zone::Local.to_ts(b))
    }

    #[test]
    fn daily_count() {
        let e = ev(dt(2026, 11, 2, 9), 30, "FREQ=DAILY;COUNT=5");
        let (a, b) = range(dt(2026, 11, 1, 0), dt(2026, 12, 1, 0));
        assert_eq!(occurrences(&e, a, b, &[]).len(), 5);
    }

    #[test]
    fn weekly_byday_window() {
        let e = ev(dt(2026, 11, 2, 9), 30, "FREQ=WEEKLY;BYDAY=MO,WE,FR");
        let (a, b) = range(dt(2026, 11, 9, 0), dt(2026, 11, 16, 0));
        let v = occurrences(&e, a, b, &[]);
        assert_eq!(v.len(), 3);
        assert_eq!(Zone::Local.naive(v[0]), dt(2026, 11, 9, 9));
    }

    #[test]
    fn weekdays_and_exdate() {
        let mut e = ev(dt(2026, 11, 2, 8), 60, "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR");
        e.exdates.push(Zone::Local.to_ts(dt(2026, 11, 11, 8)));
        let (a, b) = range(dt(2026, 11, 9, 0), dt(2026, 11, 16, 0));
        assert_eq!(occurrences(&e, a, b, &[]).len(), 4);
        assert_eq!(Rule::parse(&e.rrule).unwrap().kind(), "weekdays");
    }

    #[test]
    fn monthly_31st_skips_short_months() {
        let e = ev(dt(2026, 1, 31, 10), 60, "FREQ=MONTHLY;COUNT=4");
        let (a, b) = range(dt(2026, 1, 1, 0), dt(2027, 1, 1, 0));
        let v: Vec<u32> = occurrences(&e, a, b, &[]).iter().map(|t| Zone::Local.naive(*t).month()).collect();
        assert_eq!(v, vec![1, 3, 5, 7]);
    }

    #[test]
    fn monthly_last_friday() {
        let e = ev(dt(2026, 1, 30, 10), 60, "FREQ=MONTHLY;BYDAY=-1FR");
        let (a, b) = range(dt(2026, 11, 1, 0), dt(2026, 12, 1, 0));
        let v = occurrences(&e, a, b, &[]);
        assert_eq!(Zone::Local.naive(v[0]).date(), NaiveDate::from_ymd_opt(2026, 11, 27).unwrap());
    }

    #[test]
    fn until_and_interval() {
        let e = ev(dt(2026, 11, 2, 9), 30, "FREQ=WEEKLY;INTERVAL=2;UNTIL=20261201T000000Z");
        let (a, b) = range(dt(2026, 10, 1, 0), dt(2027, 1, 1, 0));
        assert_eq!(occurrences(&e, a, b, &[]).len(), 3);
    }

    #[test]
    fn dst_keeps_wall_time() {
        let mut e = ev(dt(2026, 10, 20, 9), 30, "FREQ=DAILY;COUNT=20");
        e.tzid = "Europe/Warsaw".into();
        e.start = Zone::from_tzid("Europe/Warsaw").to_ts(dt(2026, 10, 20, 9));
        e.end = e.start + 1800;
        let v = occurrences(&e, e.start, e.start + 30 * 86400, &[]);
        let z = Zone::from_tzid("Europe/Warsaw");
        assert!(v.iter().all(|t| z.naive(*t).time() == NaiveTime::from_hms_opt(9, 0, 0).unwrap()));
    }

    #[test]
    fn yearly_all_day() {
        let d = NaiveDate::from_ymd_opt(2020, 11, 11).unwrap();
        let e = Event { start: time::date_ts(d), end: time::date_ts(d) + 86400, all_day: true, rrule: "FREQ=YEARLY".into(), ..Default::default() };
        let a = time::date_ts(NaiveDate::from_ymd_opt(2026, 11, 9).unwrap());
        let v = occurrences(&e, a, a + 7 * 86400, &[]);
        assert_eq!(v, vec![time::date_ts(NaiveDate::from_ymd_opt(2026, 11, 11).unwrap())]);
    }
}
