//! The engine the UI talks to: calendars, occurrences, editing with undo, quick add,
//! reminders and accounts. Network work lives in `sync` and runs on its own store connection.

use anyhow::{anyhow, bail, Result};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use std::collections::HashMap;

use crate::config::{Account, Config, Paths};
use crate::model::*;
use crate::quickadd::{self, QuickAdd};
use crate::recur::{self, Rule};
use crate::store::Store;
use crate::time::{self, Zone};
use crate::{secrets, sync};

/// One undoable step: put `before` back, delete `created`.
struct Undo {
    label: String,
    before: Vec<Event>,
    created: Vec<i64>,
}

/// A reminder that is due now.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub event_id: i64,
    pub title: String,
    pub location: String,
    pub start_ms: i64,
    pub minutes: i32,
    pub link: String,
}

pub struct Core {
    pub paths: Paths,
    pub config: Config,
    pub store: Store,
    undo: Vec<Undo>,
    /// `true` when the interface language is Polish (names of the first calendar, demo data).
    pub pl: bool,
}

impl Core {
    /// Open (and create) the store of a profile. `demo` fills an empty store with sample events.
    pub fn open(paths: Paths, demo: bool, pl: bool) -> Result<Core> {
        let config = Config::load(&paths);
        let store = Store::open(&paths.db_file())?;
        Core::with_store(paths, config, store, demo, pl)
    }

    pub fn with_store(paths: Paths, config: Config, store: Store, demo: bool, pl: bool) -> Result<Core> {
        let mut core = Core { paths, config, store, undo: vec![], pl };
        core.init(demo)?;
        Ok(core)
    }

    fn init(&mut self, demo: bool) -> Result<()> {
        let local = self.store.upsert_source("local", SourceKind::Local, if self.pl { "Ten komputer" } else { "This computer" }, "", "", "")?;
        if self.store.calendars_of(local)?.is_empty() {
            self.store.add_calendar(local, "", if self.pl { "Osobisty" } else { "Personal" }, PALETTE[0], false)?;
        }
        for a in self.config.accounts.clone() {
            let kind = SourceKind::parse(&a.kind);
            let sid = self.store.upsert_source(&a.id, kind, &a.name, &a.url, &a.username, &a.secret_ref)?;
            if kind == SourceKind::Ics && self.store.calendars_of(sid)?.is_empty() {
                let color = if a.color.is_empty() { PALETTE[self.store.calendars()?.len() % PALETTE.len()].to_string() } else { a.color.clone() };
                self.store.add_calendar(sid, &a.url, &a.name, &color, true)?;
            }
        }
        // accounts removed from config.toml by hand disappear with their events
        for (key, s) in self.store.sources()? {
            if key != "local" && !self.config.accounts.iter().any(|a| a.id == key) {
                self.store.delete_source(s.id)?;
            }
        }
        if demo && self.store.calendars()?.iter().all(|c| self.store.events_of_calendar(c.id).map(|e| e.is_empty()).unwrap_or(true)) {
            crate::demo::seed(&self.store, local, self.pl)?;
        }
        Ok(())
    }

    // ---- calendars --------------------------------------------------------------------------

    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        self.store.calendars()
    }

    pub fn set_visible(&self, id: i64, visible: bool) -> Result<()> {
        if let Some(mut c) = self.store.calendar(id)? {
            c.visible = visible;
            self.store.update_calendar(&c)?;
        }
        Ok(())
    }

    /// Calendar for new events: the configured one, else the first writable one.
    pub fn default_calendar_id(&self) -> Result<i64> {
        let cals = self.store.calendars()?;
        let sources = self.store.sources()?;
        if let Some((acc, name)) = self.config.default_calendar.split_once('/') {
            if let Some((_, s)) = sources.iter().find(|(k, _)| k == acc) {
                if let Some(c) = cals.iter().find(|c| c.source_id == s.id && c.name == name && !c.read_only) {
                    return Ok(c.id);
                }
            }
        }
        cals.iter().find(|c| !c.read_only).map(|c| c.id).ok_or_else(|| anyhow!("no writable calendar"))
    }

    fn calendar_by_hint(&self, hint: &str) -> Result<Option<i64>> {
        if hint.is_empty() {
            return Ok(None);
        }
        let h = hint.trim_start_matches('#').to_lowercase();
        Ok(self.store.calendars()?.into_iter().find(|c| !c.read_only && c.name.to_lowercase().starts_with(&h)).map(|c| c.id))
    }

    // ---- reading ----------------------------------------------------------------------------

    /// Occurrences overlapping `[from_ms, to_ms)` in the visible calendars, sorted by start.
    pub fn occurrences(&self, from_ms: i64, to_ms: i64) -> Result<Vec<Occurrence>> {
        let (from, to) = (from_ms.div_euclid(1000), to_ms.div_euclid(1000));
        let cals: HashMap<i64, Calendar> = self.store.calendars()?.into_iter().map(|c| (c.id, c)).collect();
        let evs = self.store.events_for_range(from - 86_400, to + 86_400)?;
        // overridden occurrences of a series
        let mut overridden: HashMap<(i64, String), Vec<i64>> = HashMap::new();
        for e in evs.iter().filter(|e| e.recurrence_id != 0) {
            overridden.entry((e.calendar_id, e.uid.clone())).or_default().push(e.recurrence_id);
        }
        // overrides of series whose master is outside the candidate list still show on their own
        let mut out = vec![];
        for e in &evs {
            let Some(cal) = cals.get(&e.calendar_id) else { continue };
            if !cal.visible {
                continue;
            }
            let skip = overridden.get(&(e.calendar_id, e.uid.clone())).cloned().unwrap_or_default();
            let skip: &[i64] = if e.recurrence_id == 0 { &skip } else { &[] };
            let len = (e.end - e.start).max(0);
            for s in recur::occurrences(e, from, to, skip) {
                out.push(self.occurrence(e, cal, s, s + len));
            }
        }
        out.sort_by_key(|o| (o.start_ms, o.id));
        Ok(out)
    }

    fn occurrence(&self, e: &Event, cal: &Calendar, s: i64, end: i64) -> Occurrence {
        let (sd, ed) = if e.all_day {
            (time::ymd(time::ts_date(s)), time::ymd(time::ts_date(end.max(s + 86_400))))
        } else {
            (String::new(), String::new())
        };
        Occurrence {
            id: e.id,
            calendar_id: e.calendar_id,
            title: e.title.clone(),
            location: e.location.clone(),
            start_ms: s * 1000,
            end_ms: end * 1000,
            all_day: e.all_day,
            start_date: sd,
            end_date: ed,
            tentative: e.tentative,
            recurring: !e.rrule.is_empty() || e.recurrence_id != 0,
            color: cal.color.clone(),
            read_only: cal.read_only,
            rsvp: e.rsvp.clone(),
            attendees: e.attendees,
            reminder: e.reminder,
            link: if e.location.starts_with("http") { e.location.clone() } else { find_link(&format!("{} {}", e.location, e.description)) },
        }
    }

    pub fn event(&self, id: i64) -> Result<Option<Event>> {
        self.store.event(id)
    }

    /// The details panel's data: the event, its calendar and the repeat choice.
    pub fn detail(&self, id: i64, occ_ms: i64) -> Result<Option<serde_json::Value>> {
        let Some(e) = self.store.event(id)? else { return Ok(None) };
        let cal = self.store.calendar(e.calendar_id)?.unwrap_or_default();
        let kind = Rule::parse(&e.rrule).map(|r| r.kind()).unwrap_or("none");
        let (s, en) = if occ_ms > 0 {
            (occ_ms / 1000, occ_ms / 1000 + (e.end - e.start))
        } else {
            (e.start, e.end)
        };
        let o = self.occurrence(&e, &cal, s, en);
        let mut v = serde_json::to_value(&e)?;
        let m = v.as_object_mut().unwrap();
        m.insert("calendarName".into(), cal.name.clone().into());
        m.insert("color".into(), cal.color.clone().into());
        m.insert("readOnly".into(), cal.read_only.into());
        m.insert("repeat".into(), kind.into());
        m.insert("startMs".into(), (o.start_ms).into());
        m.insert("endMs".into(), (o.end_ms).into());
        m.insert("startDate".into(), o.start_date.into());
        m.insert("endDate".into(), o.end_date.into());
        m.insert("link".into(), o.link.into());
        Ok(Some(v))
    }

    // ---- editing ----------------------------------------------------------------------------

    fn source_kind_of(&self, calendar_id: i64) -> SourceKind {
        let sid = self.store.calendar(calendar_id).ok().flatten().map(|c| c.source_id).unwrap_or(0);
        self.store.sources().ok().and_then(|v| v.into_iter().find(|(_, s)| s.id == sid)).map(|(_, s)| s.kind).unwrap_or_default()
    }

    fn writable(&self, calendar_id: i64) -> Result<()> {
        match self.store.calendar(calendar_id)? {
            Some(c) if !c.read_only => Ok(()),
            Some(_) => bail!("this calendar is read-only"),
            None => bail!("no such calendar"),
        }
    }

    /// Remember the change for the server (CalDAV calendars only).
    fn queue(&self, e: &Event, op: &str) {
        if self.source_kind_of(e.calendar_id) == SourceKind::Caldav {
            let _ = self.store.queue(e.calendar_id, op, &e.uid, &e.href, &e.etag);
        }
    }

    fn push_undo(&mut self, label: &str, before: Vec<Event>, created: Vec<i64>) {
        self.undo.push(Undo { label: label.to_string(), before, created });
        if self.undo.len() > 50 {
            self.undo.remove(0);
        }
    }

    /// Create an event; `e.id` must be 0. Returns the stored event.
    pub fn create(&mut self, mut e: Event) -> Result<Event> {
        if e.calendar_id == 0 {
            e.calendar_id = self.default_calendar_id()?;
        }
        self.writable(e.calendar_id)?;
        if e.uid.is_empty() {
            e.uid = format!("{}@kronoa", uuid::Uuid::new_v4());
        }
        if e.tzid.is_empty() && !e.all_day {
            e.tzid = time::local_tzid();
        }
        if e.reminder.is_none() && self.config.default_reminder >= 0 {
            e.reminder = Some(self.config.default_reminder);
        }
        if e.end <= e.start {
            e.end = e.start + if e.all_day { 86_400 } else { 3600 };
        }
        e.id = 0;
        e.id = self.store.save_event(&e)?;
        self.queue(&e, "put");
        self.push_undo("create", vec![], vec![e.id]);
        Ok(e)
    }

    /// Save changes to an existing event (see `Undo`).
    pub fn update(&mut self, new: Event, label: &str) -> Result<()> {
        let old = self.store.event(new.id)?.ok_or_else(|| anyhow!("no such event"))?;
        self.writable(old.calendar_id)?;
        self.writable(new.calendar_id)?;
        let mut new = new;
        if new.calendar_id != old.calendar_id {
            // moving between calendars: delete over there, create here
            self.queue(&old, "delete");
            new.href.clear();
            new.etag.clear();
            new.raw.clear();
        }
        self.store.save_event(&new)?;
        self.queue(&new, "put");
        self.push_undo(label, vec![old], vec![]);
        Ok(())
    }

    /// Move/resize an occurrence. For a series the whole series shifts by the same amount.
    pub fn move_event(&mut self, id: i64, occ_start_ms: i64, new_start_ms: i64, new_end_ms: i64) -> Result<()> {
        let mut e = self.store.event(id)?.ok_or_else(|| anyhow!("no such event"))?;
        let (ns, ne) = (new_start_ms.div_euclid(1000), new_end_ms.div_euclid(1000));
        let delta = ns - occ_start_ms.div_euclid(1000);
        let len = ne - ns;
        if e.all_day {
            e.start = time::date_ts(time::ts_date(e.start + delta + 43_200));
            e.end = e.start + len.max(86_400);
        } else {
            e.start += delta;
            e.end = e.start + len.max(60);
        }
        if let Some(r) = Rule::parse(&e.rrule) {
            if r.kind() == "weekly" {
                let wd = Zone::from_tzid(&e.tzid).naive(e.start).date().weekday();
                e.rrule = recur::rule_for_kind("weekly", wd);
            }
        }
        self.update(e, "move")
    }

    pub fn delete(&mut self, id: i64) -> Result<String> {
        let e = self.store.event(id)?.ok_or_else(|| anyhow!("no such event"))?;
        self.writable(e.calendar_id)?;
        let mut all = self.store.events_by_uid(e.calendar_id, &e.uid)?;
        if e.recurrence_id != 0 || all.is_empty() {
            all = vec![e.clone()];
        }
        for x in &all {
            self.store.delete_event(x.id)?;
        }
        self.queue(&e, if e.recurrence_id != 0 { "put" } else { "delete" });
        self.push_undo("delete", all, vec![]);
        Ok(e.title)
    }

    pub fn duplicate(&mut self, id: i64) -> Result<Event> {
        let mut e = self.store.event(id)?.ok_or_else(|| anyhow!("no such event"))?;
        e.id = 0;
        e.uid.clear();
        e.href.clear();
        e.etag.clear();
        e.raw.clear();
        e.recurrence_id = 0;
        e.exdates.clear();
        e.start += 86_400;
        e.end += 86_400;
        self.create(e)
    }

    pub fn set_rsvp(&mut self, id: i64, answer: &str) -> Result<()> {
        let mut e = self.store.event(id)?.ok_or_else(|| anyhow!("no such event"))?;
        let me = self.me_for(e.calendar_id);
        e.raw = crate::ical::set_partstat(&e.raw, &me, answer);
        e.rsvp = answer.to_string();
        self.update(e, "rsvp")
    }

    fn me_for(&self, calendar_id: i64) -> String {
        let sid = self.store.calendar(calendar_id).ok().flatten().map(|c| c.source_id).unwrap_or(0);
        self.store
            .sources()
            .ok()
            .and_then(|v| v.into_iter().find(|(_, s)| s.id == sid))
            .map(|(_, s)| s.username)
            .filter(|u| u.contains('@'))
            .unwrap_or_default()
    }

    /// Undo the last change. Returns what was undone (`create`, `delete`, `move`...) or None.
    pub fn undo(&mut self) -> Result<Option<String>> {
        let Some(u) = self.undo.pop() else { return Ok(None) };
        for id in &u.created {
            if let Some(e) = self.store.event(*id)? {
                self.store.delete_event(*id)?;
                self.queue(&e, "delete");
            }
        }
        for e in &u.before {
            self.store.save_event(e)?;
            self.queue(e, "put");
        }
        Ok(Some(u.label))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    // ---- quick add --------------------------------------------------------------------------

    pub fn preview(&self, text: &str) -> QuickAdd {
        quickadd::parse(text, chrono::Local::now().naive_local())
    }

    /// Create the event described by a sentence. Returns the stored event.
    pub fn create_from_text(&mut self, text: &str) -> Result<Event> {
        let q = self.preview(text);
        if q.title.trim().is_empty() && !q.has_date && !q.has_time {
            bail!("nothing to add");
        }
        let date = q.date();
        let (start, end) = match (q.all_day, q.start_time()) {
            (false, Some(t)) => {
                let s = Zone::Local.to_ts(date.and_time(t));
                (s, s + q.duration.max(5) * 60)
            }
            _ => (time::date_ts(date), time::date_ts(date + Duration::days(1))),
        };
        let cal = self.calendar_by_hint(&q.calendar)?.unwrap_or(0);
        let all_day = q.all_day || q.start_time().is_none();
        let e = Event {
            calendar_id: cal,
            title: if q.title.is_empty() { if self.pl { "Nowe wydarzenie".into() } else { "New event".into() } } else { q.title },
            location: q.location,
            start,
            end,
            all_day,
            rrule: q.rrule,
            attendees: q.attendees.len() as i32,
            ..Default::default()
        };
        self.create(e)
    }

    /// An event for a range picked in the grid (milliseconds).
    pub fn create_range(&mut self, start_ms: i64, end_ms: i64, all_day: bool) -> Result<Event> {
        let (s, e) = (start_ms.div_euclid(1000), end_ms.div_euclid(1000));
        let ev = if all_day {
            Event { start: time::date_ts(time::ts_date(s + 43_200)), end: time::date_ts(time::ts_date(s + 43_200)) + 86_400, all_day: true, ..Default::default() }
        } else {
            Event { start: s, end: e, ..Default::default() }
        };
        self.create(ev)
    }

    // ---- reminders --------------------------------------------------------------------------

    /// Reminders that are due at `now` (Unix seconds) and have not been shown yet.
    pub fn due_reminders(&self, now: i64) -> Result<Vec<Reminder>> {
        let mut out = vec![];
        self.store.prune_notified(now - 7 * 86_400)?;
        let cals: HashMap<i64, Calendar> = self.store.calendars()?.into_iter().map(|c| (c.id, c)).collect();
        for e in self.store.events_for_range(now - 3600, now + 8 * 86_400)? {
            let Some(m) = e.reminder else { continue };
            if e.all_day || !cals.get(&e.calendar_id).map(|c| c.visible).unwrap_or(false) {
                continue;
            }
            let len = (e.end - e.start).max(0);
            for s in recur::occurrences(&e, now - 3600, now + 8 * 86_400, &[]) {
                let trigger = s - m as i64 * 60;
                if trigger <= now && s + len.min(3600) > now && self.store.mark_notified(e.id, s)? {
                    out.push(Reminder {
                        event_id: e.id,
                        title: e.title.clone(),
                        location: e.location.clone(),
                        start_ms: s * 1000,
                        minutes: m,
                        link: find_link(&format!("{} {}", e.location, e.description)),
                    });
                }
            }
        }
        Ok(out)
    }

    // ---- accounts ---------------------------------------------------------------------------

    /// Add a CalDAV account or an ICS subscription (already verified by `sync::test_account`).
    pub fn add_account(&mut self, kind: &str, name: &str, url: &str, username: &str, password: &str, color: &str) -> Result<String> {
        let k = SourceKind::parse(kind);
        if !matches!(k, SourceKind::Caldav | SourceKind::Ics) {
            bail!("unsupported account type: {kind}");
        }
        let slug: String = name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect();
        let id = format!("{}-{}", if slug.is_empty() { kind.to_string() } else { slug }, &uuid::Uuid::new_v4().simple().to_string()[..6]);
        let mut secret_ref = String::new();
        if k == SourceKind::Caldav {
            let stored = secrets::store(&self.paths, &id, password, self.config.allow_plaintext_secrets);
            if stored == secrets::Stored::Session {
                eprintln!("kronoa: no keyring available - the password is kept for this session only (see README)");
            }
            secret_ref = secrets::secret_ref(&self.paths, &id);
        }
        let name = if name.trim().is_empty() { url.to_string() } else { name.trim().to_string() };
        let acc = Account { id: id.clone(), kind: kind.into(), name, url: url.trim().into(), username: username.trim().into(), secret_ref, color: color.into() };
        self.config.accounts.push(acc);
        self.config.save(&self.paths)?;
        self.init(false)?;
        Ok(id)
    }

    pub fn remove_account(&mut self, key: &str) -> Result<()> {
        secrets::delete(&self.paths, key);
        self.config.accounts.retain(|a| a.id != key);
        self.config.save(&self.paths)?;
        self.init(false)
    }

    /// Accounts for the Settings view (no secrets).
    pub fn accounts_json(&self) -> Result<serde_json::Value> {
        let mut v = vec![];
        for (key, s) in self.store.sources()? {
            let mut j = serde_json::to_value(&s)?;
            j["key"] = key.clone().into();
            j["calendars"] = self.store.calendars_of(s.id)?.len().into();
            v.push(j);
        }
        Ok(serde_json::Value::Array(v))
    }

    pub fn sync_all(&self) -> Vec<(String, std::result::Result<String, String>)> {
        sync::sync_all(&self.paths, &self.config)
    }
}

/// Monday-based week start of the week containing `d`.
pub fn week_start(d: NaiveDate, first_day: u32) -> NaiveDate {
    // first_day: 1 = Monday … 7 = Sunday
    let wd = d.weekday().number_from_monday();
    let back = (wd + 7 - first_day) % 7;
    d - Duration::days(back as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> Core {
        Core::with_store(Paths::for_profile("test-mem"), Config::default(), Store::memory().unwrap(), false, false).unwrap()
    }

    #[test]
    fn create_move_undo() {
        let mut c = core();
        let now = chrono::Local::now().naive_local().date();
        let s = Zone::Local.to_ts(now.and_hms_opt(10, 0, 0).unwrap());
        let e = c.create(Event { title: "A".into(), start: s, end: s + 3600, ..Default::default() }).unwrap();
        let occ = c.occurrences(s * 1000 - 1000, (s + 7200) * 1000).unwrap();
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].reminder, Some(10));
        c.move_event(e.id, s * 1000, (s + 1800) * 1000, (s + 5400) * 1000).unwrap();
        assert_eq!(c.event(e.id).unwrap().unwrap().start, s + 1800);
        assert_eq!(c.undo().unwrap().as_deref(), Some("move"));
        assert_eq!(c.event(e.id).unwrap().unwrap().start, s);
        c.delete(e.id).unwrap();
        assert!(c.event(e.id).unwrap().is_none());
        assert_eq!(c.undo().unwrap().as_deref(), Some("delete"));
        assert!(c.event(e.id).unwrap().is_some());
        assert_eq!(c.undo().unwrap().as_deref(), Some("create"));
        assert!(c.event(e.id).unwrap().is_none());
        assert!(c.undo().unwrap().is_none());
    }

    #[test]
    fn quick_add_creates_series() {
        let mut c = core();
        let e = c.create_from_text("Stand-up jutro 9:30 15 min codziennie").unwrap();
        assert_eq!(e.title, "Stand-up");
        assert!(e.rrule.contains("DAILY"));
        let t = time::today();
        let from = Zone::Local.to_ts(t.and_hms_opt(0, 0, 0).unwrap()) * 1000;
        let occ = c.occurrences(from, from + 4 * 86_400_000).unwrap();
        assert!(occ.len() >= 3, "{}", occ.len());
        assert_eq!(c.detail(e.id, 0).unwrap().unwrap()["repeat"], "daily");
    }

    #[test]
    fn hidden_calendars_and_readonly() {
        let mut c = core();
        let cal = c.calendars().unwrap()[0].clone();
        let s = time::now();
        let e = c.create(Event { title: "X".into(), start: s, end: s + 60, ..Default::default() }).unwrap();
        c.set_visible(cal.id, false).unwrap();
        assert!(c.occurrences(s * 1000 - 1000, s * 1000 + 100_000).unwrap().is_empty());
        c.set_visible(cal.id, true).unwrap();
        let mut ro = cal.clone();
        ro.read_only = true;
        c.store.update_calendar(&ro).unwrap();
        assert!(c.delete(e.id).is_err());
    }

    #[test]
    fn reminders_fire_once() {
        let mut c = core();
        let s = time::now() + 5 * 60;
        c.create(Event { title: "Soon".into(), start: s, end: s + 600, ..Default::default() }).unwrap();
        assert_eq!(c.due_reminders(time::now()).unwrap().len(), 1);
        assert!(c.due_reminders(time::now()).unwrap().is_empty());
    }

    #[test]
    fn week_start_monday() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        assert_eq!(week_start(d, 1), NaiveDate::from_ymd_opt(2026, 11, 9).unwrap());
        assert_eq!(week_start(d, 7), NaiveDate::from_ymd_opt(2026, 11, 8).unwrap());
    }
}
