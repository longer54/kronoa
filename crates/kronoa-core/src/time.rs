//! Time helpers: the local zone, wall-clock ↔ instant conversion and all-day dates.

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// The zone used for drawing and for new events.
#[derive(Clone, Copy, Debug)]
pub enum Zone {
    Local,
    Named(Tz),
}

impl Zone {
    /// `tzid` as an IANA zone; anything unknown (or empty) is the system zone.
    pub fn from_tzid(tzid: &str) -> Zone {
        tzid.trim().parse::<Tz>().map(Zone::Named).unwrap_or(Zone::Local)
    }

    /// The instant of a wall-clock time in this zone (gaps move forward, overlaps take the first).
    pub fn to_ts(&self, naive: NaiveDateTime) -> i64 {
        fn pick<T: TimeZone>(r: LocalResult<DateTime<T>>, z: &T, naive: NaiveDateTime) -> i64 {
            match r {
                LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.timestamp(),
                LocalResult::None => match z.from_local_datetime(&(naive + Duration::hours(1))) {
                    LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.timestamp(),
                    LocalResult::None => naive.and_utc().timestamp(),
                },
            }
        }
        match self {
            Zone::Local => pick(Local.from_local_datetime(&naive), &Local, naive),
            Zone::Named(tz) => pick(tz.from_local_datetime(&naive), tz, naive),
        }
    }

    /// The wall-clock time of an instant in this zone.
    pub fn naive(&self, ts: i64) -> NaiveDateTime {
        let utc = DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default();
        match self {
            Zone::Local => utc.with_timezone(&Local).naive_local(),
            Zone::Named(tz) => utc.with_timezone(tz).naive_local(),
        }
    }
}

/// The system zone's IANA name, when known.
pub fn local_tzid() -> String {
    iana_time_zone::get_timezone().unwrap_or_default()
}

/// Midnight UTC of a date: how all-day events are stored.
pub fn date_ts(d: NaiveDate) -> i64 {
    d.and_time(NaiveTime::MIN).and_utc().timestamp()
}

/// The date of an all-day timestamp.
pub fn ts_date(ts: i64) -> NaiveDate {
    DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default().date_naive()
}

/// Local midnight of a date as an instant.
pub fn local_midnight(d: NaiveDate) -> i64 {
    Zone::Local.to_ts(d.and_time(NaiveTime::MIN))
}

/// `YYYY-MM-DD`.
pub fn ymd(d: NaiveDate) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

/// Now, in Unix seconds.
pub fn now() -> i64 {
    Utc::now().timestamp()
}

/// Today in the local zone.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Days in a month.
pub fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}
