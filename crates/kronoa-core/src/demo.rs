//! Sample data for screenshots (`--demo`). Never used unless asked for.

use anyhow::Result;
use chrono::{Duration, NaiveDate};

use crate::model::Event;
use crate::store::Store;
use crate::time::{self, Zone};

/// Fill an empty store with a week of events around today.
pub fn seed(store: &Store, local_source: i64, pl: bool) -> Result<()> {
    let t = |en: &'static str, pl_: &'static str| if pl { pl_ } else { en };
    let personal = store.calendars_of(local_source)?[0].clone();
    let mut p = personal.clone();
    p.name = t("Work", "Praca").into();
    p.color = "#2383e2".into();
    store.update_calendar(&p)?;
    let work = personal.id;
    let home = store.add_calendar(local_source, "", t("Home", "Dom"), "#0f9d58", false)?;
    let sport = store.add_calendar(local_source, "", t("Sport", "Sport"), "#e16259", false)?;
    let proj = store.add_calendar(local_source, "", t("Projects", "Projekty"), "#9065b0", false)?;
    let hol = store.add_calendar(local_source, "", t("Holidays", "Święta"), "#d9730d", false)?;

    let today = time::today();
    let monday = crate::engine::week_start(today, 1);
    let ts = |d: i64, h: f64| -> i64 {
        let day: NaiveDate = monday + Duration::days(d);
        Zone::Local.to_ts(day.and_hms_opt(0, 0, 0).unwrap()) + (h * 3600.0).round() as i64
    };
    let mut n = 0;
    let mut add = |cal: i64, d: i64, s: f64, e: f64, title: &str, loc: &str, att: i32, tent: bool, rr: &str| {
        n += 1;
        let ev = Event {
            calendar_id: cal,
            uid: format!("demo-{n}@kronoa"),
            title: title.into(),
            location: loc.into(),
            start: ts(d, s),
            end: ts(d, e),
            tzid: time::local_tzid(),
            reminder: Some(10),
            tentative: tent,
            attendees: att,
            rrule: rr.into(),
            ..Default::default()
        };
        store.save_event(&ev).map(|_| ())
    };
    add(work, 0, 9.0, 10.0, t("Morning planning", "Poranne planowanie"), "", 0, false, "")?;
    add(work, 0, 12.0, 13.5, t("Team meeting", "Spotkanie z zespołem"), "https://meet.example.org/team", 4, false, "")?;
    add(sport, 1, 7.5, 8.5, t("Gym", "Siłownia"), "", 0, false, "")?;
    add(work, 1, 10.0, 11.0, t("Call with the supplier", "Rozmowa z dostawcą"), "", 0, true, "")?;
    add(proj, 2, 14.0, 16.0, t("Product workshop", "Warsztat produktowy"), t("Room B", "Sala B"), 6, false, "")?;
    add(work, 3, 9.0, 9.5, "Stand-up", "", 0, false, "FREQ=WEEKLY;BYDAY=TH")?;
    add(sport, 3, 17.0, 18.0, t("Swimming", "Basen"), "", 0, false, "")?;
    add(home, 4, 8.0, 9.0, t("Check-up", "Badania"), "", 0, false, "")?;
    add(proj, 4, 11.0, 12.5, t("Project review", "Przegląd projektu"), "https://meet.example.org/review", 3, false, "")?;
    add(work, 4, 15.0, 15.75, t("Call the client", "Telefon do klienta"), "", 0, false, "")?;
    add(sport, 5, 10.0, 12.0, t("Bike ride to the lake", "Rowerem nad zalew"), "", 0, false, "")?;
    add(home, 6, 13.0, 15.0, t("Family lunch", "Obiad rodzinny"), "", 0, false, "")?;
    // all-day holiday on Wednesday
    let ad = Event {
        calendar_id: hol,
        uid: "demo-holiday@kronoa".into(),
        title: t("Public holiday", "Święto państwowe").into(),
        start: time::date_ts(monday + Duration::days(2)),
        end: time::date_ts(monday + Duration::days(3)),
        all_day: true,
        ..Default::default()
    };
    store.save_event(&ad)?;
    Ok(())
}
