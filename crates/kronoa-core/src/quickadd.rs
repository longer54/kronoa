//! Quick add: one sentence (Polish or English) → an event draft.
//!
//! The parser cuts recognised pieces (date, time, duration, recurrence, place, calendar,
//! attendees) out of the sentence; what is left is the title. Everything is relative to a
//! reference "now" so the result is testable.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use regex::{Captures, Regex};
use serde::Serialize;
use std::sync::OnceLock;

use crate::recur::weekday_code;

/// The result of parsing a sentence.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickAdd {
    pub title: String,
    /// `YYYY-MM-DD`
    pub date: String,
    /// `HH:MM`, or "" for an all-day event.
    pub start: String,
    /// Minutes (0 for all-day events).
    pub duration: i64,
    pub all_day: bool,
    /// RRULE value ("" = single).
    pub rrule: String,
    /// daily | weekdays | weekly | monthly | yearly | "" (for the preview).
    pub repeat: String,
    pub location: String,
    /// `#name` hint for the calendar ("" = default).
    pub calendar: String,
    pub attendees: Vec<String>,
    /// True when a date or time was recognised (the preview shows it as parsed, not guessed).
    pub has_date: bool,
    pub has_time: bool,
}

impl QuickAdd {
    pub fn date(&self) -> NaiveDate {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").unwrap_or_default()
    }
    pub fn start_time(&self) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(&self.start, "%H:%M").ok()
    }
}

const DEFAULT_MINUTES: i64 = 60;

struct Res {
    recur: Regex,
    recur_day: Regex,
    all_day: Regex,
    range: Regex,
    from_to: Regex,
    duration: Regex,
    duration_words: Regex,
    time_hm: Regex,
    time_at: Regex,
    time_ampm: Regex,
    part_of_day: Regex,
    rel_day: Regex,
    in_days: Regex,
    in_weeks: Regex,
    weekday: Regex,
    iso: Regex,
    dmy: Regex,
    d_month: Regex,
    month_d: Regex,
    calendar: Regex,
    attendee: Regex,
    with_person: Regex,
    location: Regex,
    strip_edges: Regex,
}

const WD: &str = r"poniedziałek|poniedzialek|pon|pn|wtorek|wt|środę|środa|srode|sroda|śr|sr|czwartek|czw|piątek|piatek|pt|sobotę|sobota|sobote|sob|niedzielę|niedziela|niedziele|niedz|nd|monday|mon|tuesday|tue|tues|wednesday|wed|thursday|thu|thurs|friday|fri|saturday|sat|sunday|sun";
const MONTHS: &str = r"stycznia|styczeń|styczen|sty|lutego|luty|lut|marca|marzec|mar|kwietnia|kwiecień|kwiecien|kwi|maja|maj|czerwca|czerwiec|cze|lipca|lipiec|lip|sierpnia|sierpień|sierpien|sie|września|wrzesnia|wrzesień|wrzesien|wrz|października|pazdziernika|październik|pazdziernik|paź|paz|listopada|listopad|lis|grudnia|grudzień|grudzien|gru|january|jan|february|feb|march|april|apr|may|june|jun|july|jul|august|aug|september|sept|sep|october|oct|november|nov|december|dec";

fn res() -> &'static Res {
    static R: OnceLock<Res> = OnceLock::new();
    R.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("quick-add regex");
        Res {
            recur: r(r"(?i)\b(codziennie|każdego dnia|kazdego dnia|co dzień|co dzien|daily|every day|w dni robocze|w dni powszednie|every weekday|weekdays|co tydzień|co tydzien|cotygodniowo|weekly|every week|co miesiąc|co miesiac|monthly|every month|co roku|co rok|yearly|annually|every year)\b"),
            recur_day: r(&format!(r"(?i)\b(?:co|każdy|każdą|kazdy|kazda|every)\s+({WD})\b")),
            all_day: r(r"(?i)\b(cały dzień|caly dzien|całodniowe|całodniowy|all[- ]day)\b"),
            range: r(r"(?i)(?:\b(?:o|od|at|from|godz\.?)\s+)?\b(\d{1,2})(?:[:.](\d{2}))?\s*(am|pm)?\s*(?:-|–|—)\s*(\d{1,2})(?:[:.](\d{2}))?\s*(am|pm)?\b"),
            from_to: r(r"(?i)\b(?:od|from)\s+(\d{1,2})(?:[:.](\d{2}))?\s*(am|pm)?\s+(?:do|to|until|till)\s+(\d{1,2})(?:[:.](\d{2}))?\s*(am|pm)?\b"),
            duration: r(r"(?i)(?:\b(?:przez|na|for)\s+)?\b(\d+(?:[.,]\d+)?)\s*(minut[ay]?|min\.?|m|godzin[ya]?|godz\.?|g|hours?|hrs?|h)(?:\b|$)"),
            duration_words: r(r"(?i)\b(przez godzinę|przez godzine|na godzinę|na godzine|godzinę|for an hour|an hour|one hour|pół godziny|pol godziny|przez pół godziny|half an hour|kwadrans|półtorej godziny|poltorej godziny|an hour and a half)\b"),
            time_hm: r(r"(?i)(?:\b(?:o|at|godz\.?|o godz\.?|o godzinie)\s+)?\b([01]?\d|2[0-3])[:.]([0-5]\d)\s*(am|pm)?\b"),
            time_at: r(r"(?i)\b(?:o|at|godz\.?|o godz\.?|o godzinie)\s+([01]?\d|2[0-3])\s*(am|pm)?\b"),
            time_ampm: r(r"(?i)\b(1[0-2]|0?[1-9])\s*(am|pm)\b"),
            part_of_day: r(r"(?i)\b(rano|wieczorem|wieczór|wieczor|po południu|po poludniu|w nocy|in the morning|in the evening|in the afternoon|tonight|at night)\b"),
            rel_day: r(r"(?i)\b(dzisiaj|dziś|dzis|today|tonight|jutro|tomorrow|tmrw|pojutrze|day after tomorrow)\b"),
            in_days: r(r"(?i)\b(?:za|in)\s+(\d+|jeden|dwa|trzy|cztery|pięć|piec|one|two|three|four|five)\s+(dni|dzień|dzien|days?)\b"),
            in_weeks: r(r"(?i)\b(?:za|in)\s+(tydzień|tydzien|a week|one week|(\d+)\s+(?:tygodnie|tygodni|weeks?))\b"),
            weekday: r(&format!(r"(?i)\b(?:(?:w|we|na|on|this|next|w przyszły|w przyszłą|w przyszly|w przyszla|w następny|w następną|w nastepny|w nastepna|w najbliższy|w najbliższą|przyszły|przyszłą|następny|następną)\s+)?({WD})\b\.?")),
            iso: r(r"\b(\d{4})-(\d{2})-(\d{2})\b"),
            dmy: r(r"\b(\d{1,2})[./](\d{1,2})(?:[./](\d{4}))?\b"),
            d_month: r(&format!(r"(?i)\b(\d{{1,2}})(?:\.|st|nd|rd|th)?\s+({MONTHS})\b\.?(?:\s+(\d{{4}}))?")),
            month_d: r(&format!(r"(?i)\b({MONTHS})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?\b(?:,?\s+(\d{{4}}))?")),
            calendar: r(r"#([\p{L}\p{N}_\-/]+)"),
            attendee: r(r"(?:^|\s)@([\w.+\-]+(?:@[\w.\-]+\.\w+)?)"),
            with_person: r(r"\b(?:z|ze|with)\s+(\p{Lu}\p{Ll}+)\b"),
            location: r(r"\b(?:w|we|at|in|na|u)\s+(\p{Lu}[\p{L}\p{N}\-']*(?:\s+(?:\p{Lu}|\d)[\p{L}\p{N}\-']*)*)"),
            strip_edges: r(r"(?i)^(?:\s|,|-|–|o|w|we|na|at|on|in|od|do|i|and)+\b|(?:\s|,|-|–|\bo|\bw|\bwe|\bna|\bat|\bon|\bin|\bod|\bdo|\bi|\band)+$"),
        }
    })
}

/// Cut the first match of `re` out of `text`, returning the captured groups.
fn take(text: &mut String, re: &Regex) -> Option<Vec<Option<String>>> {
    let (range, groups) = {
        let c: Captures = re.captures(text)?;
        let m = c.get(0)?;
        (m.range(), (0..c.len()).map(|i| c.get(i).map(|g| g.as_str().to_string())).collect::<Vec<_>>())
    };
    text.replace_range(range, " ");
    Some(groups)
}

fn g(v: &[Option<String>], i: usize) -> &str {
    v.get(i).and_then(|x| x.as_deref()).unwrap_or("")
}

fn weekday_of(word: &str) -> Option<Weekday> {
    let w = word.to_lowercase();
    let w = w.trim_end_matches('.');
    Some(match w {
        "poniedziałek" | "poniedzialek" | "pon" | "pn" | "monday" | "mon" => Weekday::Mon,
        "wtorek" | "wt" | "tuesday" | "tue" | "tues" => Weekday::Tue,
        "środę" | "środa" | "srode" | "sroda" | "śr" | "sr" | "wednesday" | "wed" => Weekday::Wed,
        "czwartek" | "czw" | "thursday" | "thu" | "thurs" => Weekday::Thu,
        "piątek" | "piatek" | "pt" | "friday" | "fri" => Weekday::Fri,
        "sobotę" | "sobota" | "sobote" | "sob" | "saturday" | "sat" => Weekday::Sat,
        "niedzielę" | "niedziela" | "niedziele" | "niedz" | "nd" | "sunday" | "sun" => Weekday::Sun,
        _ => return None,
    })
}

fn month_of(word: &str) -> Option<u32> {
    let w = word.to_lowercase();
    let w = w.trim_end_matches('.');
    let table: [(&[&str], u32); 12] = [
        (&["sty", "jan"], 1),
        (&["lut", "feb"], 2),
        (&["mar"], 3),
        (&["kwi", "apr"], 4),
        (&["maj", "may"], 5),
        (&["cze", "jun"], 6),
        (&["lip", "jul"], 7),
        (&["sie", "aug"], 8),
        (&["wrz", "sep"], 9),
        (&["paź", "paz", "oct"], 10),
        (&["lis", "nov"], 11),
        (&["gru", "dec"], 12),
    ];
    table.iter().find(|(p, _)| p.iter().any(|p| w.starts_with(p))).map(|(_, m)| *m)
}

fn small_number(s: &str) -> Option<i64> {
    s.parse().ok().or(match s.to_lowercase().as_str() {
        "jeden" | "one" => Some(1),
        "dwa" | "two" => Some(2),
        "trzy" | "three" => Some(3),
        "cztery" | "four" => Some(4),
        "pięć" | "piec" | "five" => Some(5),
        _ => None,
    })
}

fn hour_with_ampm(h: u32, ampm: &str) -> u32 {
    match ampm.to_lowercase().as_str() {
        "pm" if h < 12 => h + 12,
        "am" if h == 12 => 0,
        _ => h,
    }
}

fn hm(h: &str, m: &str, ampm: &str) -> Option<NaiveTime> {
    let h: u32 = h.parse().ok()?;
    let m: u32 = if m.is_empty() { 0 } else { m.parse().ok()? };
    NaiveTime::from_hms_opt(hour_with_ampm(h, ampm), m, 0)
}

/// The next date with weekday `w` on or after `from` (`strict`: after).
fn next_weekday(from: NaiveDate, w: Weekday, strict: bool) -> NaiveDate {
    let mut diff = (w.num_days_from_monday() as i64 - from.weekday().num_days_from_monday() as i64).rem_euclid(7);
    if diff == 0 && strict {
        diff = 7;
    }
    from + Duration::days(diff)
}

/// Pick the year for a day/month without a year: this year, or next year if already past.
fn infer_year(today: NaiveDate, m: u32, d: u32) -> Option<NaiveDate> {
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this < today - Duration::days(7) {
        NaiveDate::from_ymd_opt(today.year() + 1, m, d)
    } else {
        Some(this)
    }
}

/// Parse `text` relative to `now` (local wall-clock time).
pub fn parse(text: &str, now: NaiveDateTime) -> QuickAdd {
    let r = res();
    let today = now.date();
    let mut rest = format!(" {} ", text.trim());
    let mut date: Option<NaiveDate> = None;
    let mut start: Option<NaiveTime> = None;
    let mut end: Option<NaiveTime> = None;
    let mut minutes: Option<i64> = None;
    let mut rrule = String::new();
    let mut repeat = String::new();
    let mut all_day = false;
    let mut attendees = vec![];
    let mut calendar = String::new();

    // recurrence ("co poniedziałek" also sets the first date)
    if let Some(v) = take(&mut rest, &r.recur_day) {
        if let Some(w) = weekday_of(g(&v, 1)) {
            rrule = format!("FREQ=WEEKLY;BYDAY={}", weekday_code(w));
            repeat = "weekly".into();
            date = Some(next_weekday(today, w, false));
        }
    }
    if rrule.is_empty() {
        if let Some(v) = take(&mut rest, &r.recur) {
            let w = g(&v, 1).to_lowercase();
            (rrule, repeat) = if w.contains("robocze") || w.contains("powszednie") || w.contains("weekday") {
                ("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR".into(), "weekdays".into())
            } else if (w.contains("dzie") && !w.contains("tydzie")) || w.contains("dnia") || w.contains("daily") || w.contains("day") {
                ("FREQ=DAILY".into(), "daily".into())
            } else if w.contains("tydz") || w.contains("tygod") || w.contains("week") {
                ("FREQ=WEEKLY".into(), "weekly".into())
            } else if w.contains("miesi") || w.contains("month") {
                ("FREQ=MONTHLY".into(), "monthly".into())
            } else {
                ("FREQ=YEARLY".into(), "yearly".into())
            };
        }
    }
    if take(&mut rest, &r.all_day).is_some() {
        all_day = true;
    }

    // calendar, attendees
    if let Some(v) = take(&mut rest, &r.calendar) {
        calendar = g(&v, 1).to_string();
    }
    while let Some(v) = take(&mut rest, &r.attendee) {
        attendees.push(g(&v, 1).to_string());
    }

    // dates (before times so "13.11" is not read as 13:11)
    if let Some(v) = take(&mut rest, &r.iso) {
        date = NaiveDate::from_ymd_opt(g(&v, 1).parse().unwrap_or(0), g(&v, 2).parse().unwrap_or(0), g(&v, 3).parse().unwrap_or(0));
    }
    if date.is_none() {
        if let Some(v) = take(&mut rest, &r.d_month) {
            let d: u32 = g(&v, 1).parse().unwrap_or(1);
            if let Some(m) = month_of(g(&v, 2)) {
                date = match g(&v, 3).parse::<i32>() {
                    Ok(y) => NaiveDate::from_ymd_opt(y, m, d),
                    Err(_) => infer_year(today, m, d),
                };
            }
        }
    }
    if date.is_none() {
        if let Some(v) = take(&mut rest, &r.month_d) {
            let d: u32 = g(&v, 2).parse().unwrap_or(1);
            if let Some(m) = month_of(g(&v, 1)) {
                date = match g(&v, 3).parse::<i32>() {
                    Ok(y) => NaiveDate::from_ymd_opt(y, m, d),
                    Err(_) => infer_year(today, m, d),
                };
            }
        }
    }
    if date.is_none() {
        // 13.11 / 13.11.2026 / 13/11 — a date only when it can be one and is not "o 14.30"
        let candidate = r.dmy.captures(&rest).and_then(|c| {
            let m0 = c.get(0)?;
            let before = rest[..m0.start()].trim_end().to_lowercase();
            let is_time_prefix = before.ends_with(" o") || before.ends_with(" at") || before.ends_with("godz.") || before.ends_with("godz");
            let d: u32 = c.get(1)?.as_str().parse().ok()?;
            let m: u32 = c.get(2)?.as_str().parse().ok()?;
            let y = c.get(3).and_then(|y| y.as_str().parse::<i32>().ok());
            if is_time_prefix || m == 0 || m > 12 || d == 0 || d > 31 || (y.is_none() && c.get(0)?.as_str().contains(':')) {
                return None;
            }
            Some((m0.range(), d, m, y))
        });
        if let Some((range, d, m, y)) = candidate {
            date = match y {
                Some(y) => NaiveDate::from_ymd_opt(y, m, d),
                None => infer_year(today, m, d),
            };
            rest.replace_range(range, " ");
        }
    }
    if date.is_none() {
        if let Some(v) = take(&mut rest, &r.rel_day) {
            let w = g(&v, 1).to_lowercase();
            date = Some(if w.starts_with("poj") || w.starts_with("day after") {
                today + Duration::days(2)
            } else if w.starts_with("jut") || w.starts_with("tom") || w == "tmrw" {
                today + Duration::days(1)
            } else {
                today
            });
        }
    }
    if date.is_none() {
        if let Some(v) = take(&mut rest, &r.in_days) {
            date = small_number(g(&v, 1)).map(|n| today + Duration::days(n));
        }
    }
    if date.is_none() {
        if let Some(v) = take(&mut rest, &r.in_weeks) {
            let n = small_number(g(&v, 2)).unwrap_or(1);
            date = Some(today + Duration::weeks(n));
        }
    }
    if date.is_none() {
        if let Some(v) = take(&mut rest, &r.weekday) {
            if let Some(w) = weekday_of(g(&v, 1)) {
                let lead = g(&v, 0).to_lowercase();
                let strict = lead.contains("next") || lead.contains("przysz") || lead.contains("nast");
                let mut d = next_weekday(today, w, strict);
                if lead.contains("next") && d - today < Duration::days(7) && lead.trim_start().starts_with("next") && strict {
                    // "next friday" = the coming one (common usage); keep it
                    d = next_weekday(today, w, true);
                }
                date = Some(d);
                if repeat == "weekly" && rrule == "FREQ=WEEKLY" {
                    rrule = format!("FREQ=WEEKLY;BYDAY={}", weekday_code(w));
                }
            }
        }
    }

    // times
    if let Some(v) = take(&mut rest, &r.from_to).or_else(|| take(&mut rest, &r.range)) {
        let (ap1, ap2) = (g(&v, 3), g(&v, 6));
        let ap1 = if ap1.is_empty() { ap2 } else { ap1 };
        start = hm(g(&v, 1), g(&v, 2), ap1);
        end = hm(g(&v, 4), g(&v, 5), ap2);
    }
    if start.is_none() {
        if let Some(v) = take(&mut rest, &r.time_hm) {
            start = hm(g(&v, 1), g(&v, 2), g(&v, 3));
        }
    }
    if start.is_none() {
        if let Some(v) = take(&mut rest, &r.time_ampm) {
            start = hm(g(&v, 1), "", g(&v, 2));
        }
    }
    if start.is_none() {
        if let Some(v) = take(&mut rest, &r.time_at) {
            start = hm(g(&v, 1), "", g(&v, 2));
        }
    }
    if let Some(v) = take(&mut rest, &r.part_of_day) {
        let w = g(&v, 1).to_lowercase();
        let evening = w.contains("wiecz") || w.contains("evening") || w.contains("night") || w.contains("tonight") || w.contains("nocy") || w.contains("południu") || w.contains("poludniu") || w.contains("afternoon");
        if let Some(s) = start {
            use chrono::Timelike;
            if evening && s.hour() < 12 {
                start = NaiveTime::from_hms_opt(s.hour() + 12, s.minute(), 0);
                end = end.and_then(|e| if e.hour() < 12 { NaiveTime::from_hms_opt(e.hour() + 12, e.minute(), 0) } else { Some(e) });
            }
        } else if w.contains("tonight") {
            date = date.or(Some(today));
        }
    }

    // duration
    if let Some(v) = take(&mut rest, &r.duration) {
        let n: f64 = g(&v, 1).replace(',', ".").parse().unwrap_or(0.0);
        let unit = g(&v, 2).to_lowercase();
        let mins = if unit.starts_with('m') { n } else { n * 60.0 };
        if mins > 0.0 {
            minutes = Some(mins.round() as i64);
        }
    } else if let Some(v) = take(&mut rest, &r.duration_words) {
        let w = g(&v, 1).to_lowercase();
        minutes = Some(if w.contains("półtorej") || w.contains("poltorej") || w.contains("and a half") {
            90
        } else if w.contains("pół") || w.contains("pol ") || w.contains("half") {
            30
        } else if w.contains("kwadrans") {
            15
        } else {
            60
        });
    }

    // people and place (case-sensitive; after dates so "w Piątek" is already gone)
    if let Some(v) = take(&mut rest, &r.with_person) {
        attendees.push(g(&v, 1).to_string());
    }
    let mut location = String::new();
    if let Some(v) = take(&mut rest, &r.location) {
        location = g(&v, 1).trim().to_string();
    }

    // the title: what is left
    let mut title = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    for _ in 0..3 {
        title = r.strip_edges.replace_all(&title, "").trim().to_string();
    }
    if let Some(p) = attendees.iter().find(|a| !a.contains('@') && a.chars().next().is_some_and(|c| c.is_uppercase())) {
        if title.is_empty() {
            title = p.clone();
        }
    }
    let mut chars = title.chars();
    title = match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };

    let has_time = start.is_some();
    let has_date = date.is_some();
    let all_day = all_day || (!has_time && minutes.is_none());
    let start = if all_day { None } else { start.or(NaiveTime::from_hms_opt(10, 0, 0)) };
    let duration = if all_day {
        0
    } else if let (Some(s), Some(e)) = (start, end) {
        let d = (e - s).num_minutes();
        if d > 0 { d } else { d + 24 * 60 }
    } else {
        minutes.unwrap_or(DEFAULT_MINUTES)
    };
    let date = date.unwrap_or(today);
    if rrule == "FREQ=WEEKLY" {
        rrule = format!("FREQ=WEEKLY;BYDAY={}", weekday_code(date.weekday()));
    }
    QuickAdd {
        title,
        date: crate::time::ymd(date),
        start: start.map(|t| t.format("%H:%M").to_string()).unwrap_or_default(),
        duration,
        all_day,
        rrule,
        repeat,
        location,
        calendar,
        attendees,
        has_date,
        has_time,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Friday, 13 November 2026, 12:00.
    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 13).unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    #[track_caller]
    fn check(text: &str, title: &str, date: &str, start: &str, dur: i64, rrule: &str) {
        let q = parse(text, now());
        assert_eq!((q.title.as_str(), q.date.as_str(), q.start.as_str(), q.duration, q.rrule.as_str()), (title, date, start, dur, rrule), "input: {text:?} → {q:?}");
    }

    #[test] fn pl_tomorrow_time_hour() { check("spotkanie jutro 14:00 1h", "Spotkanie", "2026-11-14", "14:00", 60, ""); }
    #[test] fn pl_today_dot_time() { check("dentysta dziś 14.30", "Dentysta", "2026-11-13", "14:30", 60, ""); }
    #[test] fn pl_dzisiaj() { check("dzisiaj o 18 kino", "Kino", "2026-11-13", "18:00", 60, ""); }
    #[test] fn pl_pojutrze_o_9() { check("pojutrze o 9 przegląd auta", "Przegląd auta", "2026-11-15", "09:00", 60, ""); }
    #[test] fn pl_w_piatek_today() { check("Przegląd projektu w piątek 10:30 45 min co tydzień", "Przegląd projektu", "2026-11-13", "10:30", 45, "FREQ=WEEKLY;BYDAY=FR"); }
    #[test] fn pl_we_wtorek() { check("we wtorek o 9 stand-up 15 min", "Stand-up", "2026-11-17", "09:00", 15, ""); }
    #[test] fn pl_w_srode() { check("w środę 16:00 trening 2 godz", "Trening", "2026-11-18", "16:00", 120, ""); }
    #[test] fn pl_w_sobote() { check("w sobotę rower 10:00 3h", "Rower", "2026-11-14", "10:00", 180, ""); }
    #[test] fn pl_w_niedziele() { check("obiad u mamy w niedzielę o 13", "Obiad u mamy", "2026-11-15", "13:00", 60, ""); }
    #[test] fn pl_w_poniedzialek() { check("w poniedziałek 8:30 lekarz 30 min", "Lekarz", "2026-11-16", "08:30", 30, ""); }
    #[test] fn pl_w_czwartek() { check("w czwartek basen o 17 1,5 h", "Basen", "2026-11-19", "17:00", 90, ""); }
    #[test] fn pl_codziennie() { check("codziennie 7:00 bieganie 30 min", "Bieganie", "2026-11-13", "07:00", 30, "FREQ=DAILY"); }
    #[test] fn pl_co_poniedzialek() { check("co poniedziałek o 9 standup", "Standup", "2026-11-16", "09:00", 60, "FREQ=WEEKLY;BYDAY=MO"); }
    #[test] fn pl_dni_robocze() { check("w dni robocze 8:00 poczta 15 min", "Poczta", "2026-11-13", "08:00", 15, "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"); }
    #[test] fn pl_co_miesiac() { check("czynsz co miesiąc 10 grudnia", "Czynsz", "2026-12-10", "", 0, "FREQ=MONTHLY"); }
    #[test] fn pl_date_month_name() { check("urodziny Ani 13 listopada", "Urodziny Ani", "2026-11-13", "", 0, ""); }
    #[test] fn pl_date_dots() { check("badania 20.11 o 8:15", "Badania", "2026-11-20", "08:15", 60, ""); }
    #[test] fn pl_date_full() { check("konferencja 3.02.2027 9:00 8h", "Konferencja", "2027-02-03", "09:00", 480, ""); }
    #[test] fn pl_range() { check("warsztaty jutro 14-16:30", "Warsztaty", "2026-11-14", "14:00", 150, ""); }
    #[test] fn pl_od_do() { check("szkolenie od 9 do 11 w poniedziałek", "Szkolenie", "2026-11-16", "09:00", 120, ""); }
    #[test] fn pl_przez_godzine() { check("jutro 15:00 dentysta przez godzinę", "Dentysta", "2026-11-14", "15:00", 60, ""); }
    #[test] fn pl_pol_godziny() { check("telefon do klienta 15:00 pół godziny", "Telefon do klienta", "2026-11-13", "15:00", 30, ""); }
    #[test] fn pl_za_dni() { check("za 3 dni o 10 serwis", "Serwis", "2026-11-16", "10:00", 60, ""); }
    #[test] fn pl_godz() { check("kolacja jutro godz. 19", "Kolacja", "2026-11-14", "19:00", 60, ""); }
    #[test] fn pl_wieczorem() { check("jutro o 8 wieczorem film", "Film", "2026-11-14", "20:00", 60, ""); }
    #[test] fn pl_next_week_day() { check("w przyszły piątek 10:00 przegląd", "Przegląd", "2026-11-20", "10:00", 60, ""); }
    #[test] fn pl_minutes_word() { check("jutro 9:00 rozmowa 20 minut", "Rozmowa", "2026-11-14", "09:00", 20, ""); }
    #[test] fn pl_title_only() { check("kupić prezent", "Kupić prezent", "2026-11-13", "", 0, ""); }
    #[test] fn en_tomorrow_pm() { check("lunch with team tomorrow 1pm", "Lunch with team", "2026-11-14", "13:00", 60, ""); }
    #[test] fn en_at_time() { check("dentist today at 3:30pm 45 min", "Dentist", "2026-11-13", "15:30", 45, ""); }
    #[test] fn en_next_friday() { check("review next friday 10:00", "Review", "2026-11-20", "10:00", 60, ""); }
    #[test] fn en_on_monday() { check("call on monday at 9 30 min", "Call", "2026-11-16", "09:00", 30, ""); }
    #[test] fn en_every_monday() { check("every monday 9:00 standup", "Standup", "2026-11-16", "09:00", 60, "FREQ=WEEKLY;BYDAY=MO"); }
    #[test] fn en_daily() { check("daily 8am workout 1h", "Workout", "2026-11-13", "08:00", 60, "FREQ=DAILY"); }
    #[test] fn en_weekly() { check("team sync tuesday 11:00 weekly", "Team sync", "2026-11-17", "11:00", 60, "FREQ=WEEKLY;BYDAY=TU"); }
    #[test] fn en_month_day() { check("trip Nov 20", "Trip", "2026-11-20", "", 0, ""); }
    #[test] fn en_day_month() { check("party 5 December 8pm", "Party", "2026-12-05", "20:00", 60, ""); }
    #[test] fn en_in_days() { check("in 2 days 10am haircut", "Haircut", "2026-11-15", "10:00", 60, ""); }
    #[test] fn en_from_to() { check("workshop from 9 to 12 tomorrow", "Workshop", "2026-11-14", "09:00", 180, ""); }
    #[test] fn en_hours() { check("hike saturday 9:00 4 hours", "Hike", "2026-11-14", "09:00", 240, ""); }
    #[test] fn en_all_day() { check("conference all day tomorrow", "Conference", "2026-11-14", "", 0, ""); }
    #[test] fn en_yearly() { check("anniversary 2 March yearly", "Anniversary", "2027-03-02", "", 0, "FREQ=YEARLY"); }

    #[test]
    fn location_calendar_people() {
        let q = parse("przegląd w Biurze jutro 10:00 #praca @anna@example.org", now());
        assert_eq!(q.title, "Przegląd");
        assert_eq!(q.location, "Biurze");
        assert_eq!(q.calendar, "praca");
        assert_eq!(q.attendees, vec!["anna@example.org".to_string()]);
        let q = parse("meeting at Office tomorrow 9am", now());
        assert_eq!((q.title.as_str(), q.location.as_str()), ("Meeting", "Office"));
        let q = parse("kawa z Anną jutro 9:00", now());
        assert_eq!(q.title, "Kawa");
        assert_eq!(q.attendees, vec!["Anną".to_string()]);
    }

    #[test]
    fn flags() {
        let q = parse("jutro 15:00 dentysta 30 min", now());
        assert!(q.has_date && q.has_time && !q.all_day);
        let q = parse("dentysta", now());
        assert!(!q.has_date && !q.has_time && q.all_day);
        assert_eq!(q.repeat, "");
        assert_eq!(parse("codziennie 7:00 x", now()).repeat, "daily");
    }
}
