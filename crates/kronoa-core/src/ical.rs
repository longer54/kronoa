//! iCalendar (RFC 5545) reading and writing for VEVENTs.

use chrono::{NaiveDate, NaiveDateTime};

use crate::model::Event;
use crate::time::{self, Zone};

/// One content line: name, parameters, value.
#[derive(Debug, Clone)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

fn parse_line(line: &str) -> Option<Prop> {
    // split at the first ':' that is not inside a quoted parameter value
    let mut in_q = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_q = !in_q,
            ':' if !in_q => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string())))
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

pub fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace("\r\n", "\\n").replace('\n', "\\n")
}

/// A date or date-time property → (instant, all_day, tzid).
fn instant(p: &Prop) -> Option<(i64, bool, String)> {
    let v = p.value.trim();
    if p.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || v.len() == 8 {
        let d = NaiveDate::parse_from_str(&v[..8.min(v.len())], "%Y%m%d").ok()?;
        return Some((time::date_ts(d), true, String::new()));
    }
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    let n = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    if utc {
        return Some((n.and_utc().timestamp(), false, String::new()));
    }
    let tzid = p.param("TZID").unwrap_or("").trim_start_matches('/').to_string();
    Some((Zone::from_tzid(&tzid).to_ts(n), false, tzid))
}

/// `-PT15M`, `PT1H30M`, `P1D`, `P1W` → seconds (sign kept).
pub fn parse_duration(v: &str) -> Option<i64> {
    let v = v.trim();
    let (neg, v) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', _) => 7 * 86400,
                    ('D', _) => 86400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

/// All VEVENTs of a VCALENDAR document. `me` (an email, may be empty) selects the attendee whose
/// PARTSTAT is the RSVP answer.
pub fn parse_events(text: &str, me: &str) -> Vec<Event> {
    let mut out = vec![];
    let mut cur: Option<(Event, Option<i64>, Vec<String>)> = None;
    let mut depth_alarm = false;
    let me = me.to_lowercase();
    for line in unfold(text) {
        let Some(p) = parse_line(&line) else { continue };
        match (p.name.as_str(), p.value.trim().to_ascii_uppercase().as_str()) {
            ("BEGIN", "VEVENT") => cur = Some((Event::default(), None, vec![line.clone()])),
            ("END", "VEVENT") => {
                if let Some((mut ev, dur, mut raw)) = cur.take() {
                    raw.push(line.clone());
                    if ev.end == 0 {
                        ev.end = match dur {
                            Some(d) => ev.start + d,
                            None if ev.all_day => ev.start + 86400,
                            None => ev.start,
                        };
                    }
                    ev.raw = raw.join("\r\n");
                    if !ev.uid.is_empty() || ev.start != 0 {
                        out.push(ev);
                    }
                }
            }
            ("BEGIN", "VALARM") => {
                depth_alarm = true;
                if let Some(c) = cur.as_mut() {
                    c.2.push(line.clone());
                }
            }
            ("END", "VALARM") => {
                depth_alarm = false;
                if let Some(c) = cur.as_mut() {
                    c.2.push(line.clone());
                }
            }
            _ => {
                let Some((ev, dur, raw)) = cur.as_mut() else { continue };
                raw.push(line.clone());
                if depth_alarm {
                    if p.name == "TRIGGER" && p.param("VALUE").is_none_or(|v| !v.eq_ignore_ascii_case("DATE-TIME")) {
                        if let Some(s) = parse_duration(&p.value) {
                            let m = (-s / 60) as i32;
                            if m >= 0 && ev.reminder.is_none_or(|r| m < r) {
                                ev.reminder = Some(m);
                            }
                        }
                    }
                    continue;
                }
                match p.name.as_str() {
                    "UID" => ev.uid = p.value.trim().to_string(),
                    "SUMMARY" => ev.title = unescape(&p.value),
                    "LOCATION" => ev.location = unescape(&p.value),
                    "DESCRIPTION" => ev.description = unescape(&p.value),
                    "DTSTART" => {
                        if let Some((ts, ad, tz)) = instant(&p) {
                            ev.start = ts;
                            ev.all_day = ad;
                            ev.tzid = tz;
                        }
                    }
                    "DTEND" => {
                        if let Some((ts, _, _)) = instant(&p) {
                            ev.end = ts;
                        }
                    }
                    "DURATION" => *dur = parse_duration(&p.value),
                    "RRULE" => ev.rrule = p.value.trim().to_string(),
                    "EXDATE" => {
                        for v in p.value.split(',') {
                            let q = Prop { value: v.to_string(), ..p.clone() };
                            if let Some((ts, _, _)) = instant(&q) {
                                ev.exdates.push(ts);
                            }
                        }
                    }
                    "RECURRENCE-ID" => {
                        if let Some((ts, _, _)) = instant(&p) {
                            ev.recurrence_id = ts;
                        }
                    }
                    "STATUS" => ev.tentative = p.value.trim().eq_ignore_ascii_case("TENTATIVE"),
                    "ATTENDEE" => {
                        ev.attendees += 1;
                        let addr = p.value.trim().trim_start_matches("mailto:").trim_start_matches("MAILTO:").to_lowercase();
                        if !me.is_empty() && addr == me {
                            ev.rsvp = match p.param("PARTSTAT").unwrap_or("").to_ascii_uppercase().as_str() {
                                "ACCEPTED" => "accepted",
                                "TENTATIVE" => "tentative",
                                "DECLINED" => "declined",
                                _ => "",
                            }
                            .into();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // the stored instant of a DTSTART-relative EXDATE in a floating zone matches occurrences
    out
}

fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in line.chars() {
        let l = c.len_utf8();
        if n + l > 74 {
            out.push_str("\r\n ");
            n = 1;
        }
        out.push(c);
        n += l;
    }
    out.push_str("\r\n");
    out
}

fn utc_stamp(ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0).unwrap_or_default().format("%Y%m%dT%H%M%SZ").to_string()
}

fn dt_line(name: &str, ts: i64, all_day: bool, tzid: &str) -> String {
    if all_day {
        format!("{name};VALUE=DATE:{}", time::ts_date(ts).format("%Y%m%d"))
    } else if !tzid.is_empty() && tzid.parse::<chrono_tz::Tz>().is_ok() {
        format!("{name};TZID={tzid}:{}", Zone::from_tzid(tzid).naive(ts).format("%Y%m%dT%H%M%S"))
    } else {
        format!("{name}:{}", utc_stamp(ts))
    }
}

/// Properties Kronoa writes itself; everything else of the original VEVENT is kept.
const OWN: [&str; 14] = [
    "UID", "SUMMARY", "LOCATION", "DESCRIPTION", "DTSTART", "DTEND", "DURATION", "RRULE", "EXDATE", "RECURRENCE-ID", "STATUS",
    "DTSTAMP", "LAST-MODIFIED", "SEQUENCE",
];

/// The VEVENT lines of `ev` (without BEGIN/END).
fn vevent_body(ev: &Event, seq: i64) -> Vec<String> {
    let mut l = vec![
        format!("UID:{}", ev.uid),
        format!("DTSTAMP:{}", utc_stamp(time::now())),
        format!("LAST-MODIFIED:{}", utc_stamp(time::now())),
        format!("SEQUENCE:{seq}"),
        dt_line("DTSTART", ev.start, ev.all_day, &ev.tzid),
        dt_line("DTEND", ev.end.max(ev.start + if ev.all_day { 86400 } else { 0 }), ev.all_day, &ev.tzid),
        format!("SUMMARY:{}", escape(&ev.title)),
    ];
    if ev.recurrence_id != 0 {
        l.push(dt_line("RECURRENCE-ID", ev.recurrence_id, ev.all_day, &ev.tzid));
    }
    if !ev.location.is_empty() {
        l.push(format!("LOCATION:{}", escape(&ev.location)));
    }
    if !ev.description.is_empty() {
        l.push(format!("DESCRIPTION:{}", escape(&ev.description)));
    }
    if !ev.rrule.is_empty() {
        l.push(format!("RRULE:{}", ev.rrule));
    }
    for x in &ev.exdates {
        l.push(dt_line("EXDATE", *x, ev.all_day, &ev.tzid));
    }
    if ev.tentative {
        l.push("STATUS:TENTATIVE".into());
    }
    l
}

/// Serialise events sharing one UID (master + overrides) as a VCALENDAR. Unknown properties of
/// the originals (`raw`) are preserved; reminders are rewritten from `reminder`.
pub fn write_calendar(events: &[Event]) -> String {
    let mut s = String::new();
    s.push_str("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Kronoa//Kronoa 0.1//EN\r\nCALSCALE:GREGORIAN\r\n");
    for ev in events {
        let seq = ev
            .raw
            .lines()
            .find_map(|l| l.trim_end().strip_prefix("SEQUENCE:").and_then(|v| v.parse::<i64>().ok()))
            .map(|n| n + 1)
            .unwrap_or(0);
        s.push_str("BEGIN:VEVENT\r\n");
        for line in vevent_body(ev, seq) {
            s.push_str(&fold(&line));
        }
        // keep foreign properties (attendees, organizer, conference data, X-…)
        let mut in_alarm = false;
        for line in unfold(&ev.raw) {
            let Some(p) = parse_line(&line) else { continue };
            let upper = p.value.trim().to_ascii_uppercase();
            if p.name == "BEGIN" && upper == "VALARM" {
                in_alarm = true;
                continue;
            }
            if p.name == "END" && upper == "VALARM" {
                in_alarm = false;
                continue;
            }
            if in_alarm || p.name == "BEGIN" || p.name == "END" || OWN.contains(&p.name.as_str()) {
                continue;
            }
            s.push_str(&fold(&line));
        }
        if let Some(m) = ev.reminder {
            s.push_str("BEGIN:VALARM\r\nACTION:DISPLAY\r\n");
            s.push_str(&fold(&format!("DESCRIPTION:{}", escape(&ev.title))));
            s.push_str(&format!("TRIGGER:-PT{}M\r\nEND:VALARM\r\n", m.max(0)));
        }
        s.push_str("END:VEVENT\r\n");
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

/// Change my PARTSTAT in a raw VEVENT (used for RSVP answers).
pub fn set_partstat(raw: &str, me: &str, answer: &str) -> String {
    let stat = match answer {
        "accepted" => "ACCEPTED",
        "tentative" => "TENTATIVE",
        "declined" => "DECLINED",
        _ => "NEEDS-ACTION",
    };
    let me = me.to_lowercase();
    unfold(raw)
        .into_iter()
        .map(|line| match parse_line(&line) {
            Some(p) if p.name == "ATTENDEE" && p.value.to_lowercase().trim_start_matches("mailto:") == me => {
                let params: Vec<String> = p
                    .params
                    .iter()
                    .filter(|(k, _)| k != "PARTSTAT")
                    .map(|(k, v)| if v.contains([':', ';', ',']) { format!("{k}=\"{v}\"") } else { format!("{k}={v}") })
                    .collect();
                format!("ATTENDEE;{};PARTSTAT={stat}:{}", params.join(";"), p.value).replace("ATTENDEE;;", "ATTENDEE;")
            }
            _ => line,
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:abc-1\r\nDTSTART;TZID=Europe/Warsaw:20261113T090000\r\nDTEND;TZID=Europe/Warsaw:20261113T093000\r\nSUMMARY:Stand-up\\, daily\r\nLOCATION:Room 1\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\r\nSTATUS:TENTATIVE\r\nATTENDEE;CN=Alice;PARTSTAT=NEEDS-ACTION:mailto:alice@example.org\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:bob@example.org\r\nX-CUSTOM:keep me\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:allday\r\nDTSTART;VALUE=DATE:20261111\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parse_sample() {
        let v = parse_events(SAMPLE, "alice@example.org");
        assert_eq!(v.len(), 2);
        let e = &v[0];
        assert_eq!(e.title, "Stand-up, daily");
        assert_eq!(e.end - e.start, 1800);
        assert_eq!(e.tzid, "Europe/Warsaw");
        assert!(e.tentative);
        assert_eq!(e.reminder, Some(15));
        assert_eq!(e.attendees, 2);
        assert_eq!(e.rsvp, "");
        assert!(v[1].all_day);
        assert_eq!(v[1].end - v[1].start, 86400);
    }

    #[test]
    fn roundtrip_keeps_foreign_props() {
        let mut e = parse_events(SAMPLE, "").remove(0);
        e.title = "Renamed; ok".into();
        e.reminder = Some(5);
        let out = write_calendar(std::slice::from_ref(&e));
        assert!(out.contains("X-CUSTOM:keep me"));
        assert!(out.contains("SUMMARY:Renamed\\; ok"));
        assert!(out.contains("TRIGGER:-PT5M"));
        assert!(!out.contains("PT15M"));
        let back = parse_events(&out, "").remove(0);
        assert_eq!((back.start, back.end, back.title.as_str()), (e.start, e.end, "Renamed; ok"));
        assert_eq!(back.attendees, 2);
    }

    #[test]
    fn partstat() {
        let e = parse_events(SAMPLE, "").remove(0);
        let raw = set_partstat(&e.raw, "alice@example.org", "accepted");
        let back = parse_events(&format!("BEGIN:VCALENDAR\r\n{raw}\r\nEND:VCALENDAR"), "alice@example.org");
        assert_eq!(back[0].rsvp, "accepted");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("-PT15M"), Some(-900));
        assert_eq!(parse_duration("PT1H30M"), Some(5400));
        assert_eq!(parse_duration("P1D"), Some(86400));
        assert_eq!(parse_duration("P1W"), Some(604800));
    }

    #[test]
    fn folding_utf8() {
        let long = "Zażółć gęślą jaźń ".repeat(10);
        let e = Event { uid: "x".into(), title: long.clone(), start: 0, end: 3600, ..Default::default() };
        let back = parse_events(&write_calendar(&[e]), "");
        assert_eq!(back[0].title, long);
    }
}
