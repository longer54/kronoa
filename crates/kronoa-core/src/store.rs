//! The local SQLite store: sources, calendars, events, the offline outbox and reminders sent.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

use crate::model::{Calendar, Event, Source, SourceKind};

pub struct Store {
    db: Connection,
}

/// A pending change for a remote calendar.
#[derive(Clone, Debug, PartialEq)]
pub struct OutboxItem {
    pub id: i64,
    pub calendar_id: i64,
    /// put | delete
    pub op: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sources(
  id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, kind TEXT NOT NULL, name TEXT NOT NULL,
  url TEXT NOT NULL DEFAULT '', username TEXT NOT NULL DEFAULT '', secret_ref TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT '', last_sync INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS calendars(
  id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  href TEXT NOT NULL DEFAULT '', name TEXT NOT NULL, color TEXT NOT NULL, visible INTEGER NOT NULL DEFAULT 1,
  read_only INTEGER NOT NULL DEFAULT 0, ctag TEXT NOT NULL DEFAULT '', sync_token TEXT NOT NULL DEFAULT '',
  position INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS events(
  id INTEGER PRIMARY KEY, calendar_id INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
  uid TEXT NOT NULL, recurrence_id INTEGER NOT NULL DEFAULT 0, title TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '', description TEXT NOT NULL DEFAULT '', start INTEGER NOT NULL, end INTEGER NOT NULL,
  all_day INTEGER NOT NULL DEFAULT 0, tzid TEXT NOT NULL DEFAULT '', rrule TEXT NOT NULL DEFAULT '',
  exdates TEXT NOT NULL DEFAULT '[]', reminder INTEGER, tentative INTEGER NOT NULL DEFAULT 0,
  rsvp TEXT NOT NULL DEFAULT '', attendees INTEGER NOT NULL DEFAULT 0, href TEXT NOT NULL DEFAULT '',
  etag TEXT NOT NULL DEFAULT '', raw TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS events_cal ON events(calendar_id);
CREATE INDEX IF NOT EXISTS events_time ON events(start, end);
CREATE INDEX IF NOT EXISTS events_uid ON events(uid);
CREATE TABLE IF NOT EXISTS outbox(
  id INTEGER PRIMARY KEY, calendar_id INTEGER NOT NULL, op TEXT NOT NULL, uid TEXT NOT NULL,
  href TEXT NOT NULL DEFAULT '', etag TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS notified(event_id INTEGER NOT NULL, occ INTEGER NOT NULL, PRIMARY KEY(event_id, occ));
";

const EV_COLS: &str = "id, calendar_id, uid, recurrence_id, title, location, description, start, end, all_day, tzid, rrule, exdates, reminder, tentative, rsvp, attendees, href, etag, raw";

fn ev_row(r: &Row) -> rusqlite::Result<Event> {
    let ex: String = r.get(12)?;
    Ok(Event {
        id: r.get(0)?,
        calendar_id: r.get(1)?,
        uid: r.get(2)?,
        recurrence_id: r.get(3)?,
        title: r.get(4)?,
        location: r.get(5)?,
        description: r.get(6)?,
        start: r.get(7)?,
        end: r.get(8)?,
        all_day: r.get(9)?,
        tzid: r.get(10)?,
        rrule: r.get(11)?,
        exdates: serde_json::from_str(&ex).unwrap_or_default(),
        reminder: r.get(13)?,
        tentative: r.get(14)?,
        rsvp: r.get(15)?,
        attendees: r.get(16)?,
        href: r.get(17)?,
        etag: r.get(18)?,
        raw: r.get(19)?,
    })
}

fn cal_row(r: &Row) -> rusqlite::Result<Calendar> {
    Ok(Calendar {
        id: r.get(0)?,
        source_id: r.get(1)?,
        href: r.get(2)?,
        name: r.get(3)?,
        color: r.get(4)?,
        visible: r.get(5)?,
        read_only: r.get(6)?,
        ctag: r.get(7)?,
        sync_token: r.get(8)?,
    })
}

const CAL_COLS: &str = "id, source_id, href, name, color, visible, read_only, ctag, sync_token";

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = Connection::open(path)?;
        Self::init(db)
    }

    pub fn memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Store> {
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db })
    }

    // ---- sources ----------------------------------------------------------------------------

    pub fn upsert_source(&self, key: &str, kind: SourceKind, name: &str, url: &str, username: &str, secret_ref: &str) -> Result<i64> {
        self.db.execute(
            "INSERT INTO sources(key, kind, name, url, username, secret_ref) VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(key) DO UPDATE SET kind=?2, name=?3, url=?4, username=?5, secret_ref=?6",
            params![key, kind.as_str(), name, url, username, secret_ref],
        )?;
        Ok(self.db.query_row("SELECT id FROM sources WHERE key=?1", [key], |r| r.get(0))?)
    }

    pub fn sources(&self) -> Result<Vec<(String, Source)>> {
        let mut st = self.db.prepare("SELECT id, key, kind, name, url, username, secret_ref, status, last_sync FROM sources ORDER BY id")?;
        let v = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    Source {
                        id: r.get(0)?,
                        kind: SourceKind::parse(&r.get::<_, String>(2)?),
                        name: r.get(3)?,
                        url: r.get(4)?,
                        username: r.get(5)?,
                        secret_ref: r.get(6)?,
                        status: r.get(7)?,
                        last_sync: r.get(8)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn set_source_status(&self, id: i64, status: &str, synced: bool) -> Result<()> {
        if synced {
            self.db.execute("UPDATE sources SET status=?2, last_sync=strftime('%s','now') WHERE id=?1", params![id, status])?;
        } else {
            self.db.execute("UPDATE sources SET status=?2 WHERE id=?1", params![id, status])?;
        }
        Ok(())
    }

    pub fn delete_source(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM outbox WHERE calendar_id IN (SELECT id FROM calendars WHERE source_id=?1)", [id])?;
        self.db.execute("DELETE FROM sources WHERE id=?1", [id])?;
        Ok(())
    }

    // ---- calendars --------------------------------------------------------------------------

    pub fn add_calendar(&self, source_id: i64, href: &str, name: &str, color: &str, read_only: bool) -> Result<i64> {
        let pos: i64 = self.db.query_row("SELECT COALESCE(MAX(position),0)+1 FROM calendars", [], |r| r.get(0))?;
        self.db.execute(
            "INSERT INTO calendars(source_id, href, name, color, read_only, position) VALUES(?1,?2,?3,?4,?5,?6)",
            params![source_id, href, name, color, read_only, pos],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        let mut st = self.db.prepare(&format!("SELECT {CAL_COLS} FROM calendars ORDER BY position, id"))?;
        let v = st.query_map([], cal_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn calendar(&self, id: i64) -> Result<Option<Calendar>> {
        Ok(self.db.query_row(&format!("SELECT {CAL_COLS} FROM calendars WHERE id=?1"), [id], cal_row).optional()?)
    }

    pub fn calendars_of(&self, source_id: i64) -> Result<Vec<Calendar>> {
        let mut st = self.db.prepare(&format!("SELECT {CAL_COLS} FROM calendars WHERE source_id=?1 ORDER BY position, id"))?;
        let v = st.query_map([source_id], cal_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn update_calendar(&self, c: &Calendar) -> Result<()> {
        self.db.execute(
            "UPDATE calendars SET name=?2, color=?3, visible=?4, read_only=?5, ctag=?6, sync_token=?7, href=?8 WHERE id=?1",
            params![c.id, c.name, c.color, c.visible, c.read_only, c.ctag, c.sync_token, c.href],
        )?;
        Ok(())
    }

    pub fn delete_calendar(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM outbox WHERE calendar_id=?1", [id])?;
        self.db.execute("DELETE FROM calendars WHERE id=?1", [id])?;
        Ok(())
    }

    // ---- events -----------------------------------------------------------------------------

    /// Insert (id 0) or replace an event; returns its id.
    pub fn save_event(&self, e: &Event) -> Result<i64> {
        let ex = serde_json::to_string(&e.exdates)?;
        if e.id == 0 {
            self.db.execute(
                &format!("INSERT INTO events({}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)", &EV_COLS[4..]),
                params![e.calendar_id, e.uid, e.recurrence_id, e.title, e.location, e.description, e.start, e.end, e.all_day, e.tzid, e.rrule, ex, e.reminder, e.tentative, e.rsvp, e.attendees, e.href, e.etag, e.raw],
            )?;
            Ok(self.db.last_insert_rowid())
        } else {
            self.db.execute(
                &format!("INSERT OR REPLACE INTO events({EV_COLS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)"),
                params![e.id, e.calendar_id, e.uid, e.recurrence_id, e.title, e.location, e.description, e.start, e.end, e.all_day, e.tzid, e.rrule, ex, e.reminder, e.tentative, e.rsvp, e.attendees, e.href, e.etag, e.raw],
            )?;
            Ok(e.id)
        }
    }

    pub fn event(&self, id: i64) -> Result<Option<Event>> {
        Ok(self.db.query_row(&format!("SELECT {EV_COLS} FROM events WHERE id=?1"), [id], ev_row).optional()?)
    }

    pub fn delete_event(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM events WHERE id=?1", [id])?;
        Ok(())
    }

    /// Events (masters and overrides) of a UID within a calendar.
    pub fn events_by_uid(&self, calendar_id: i64, uid: &str) -> Result<Vec<Event>> {
        let mut st = self.db.prepare(&format!("SELECT {EV_COLS} FROM events WHERE calendar_id=?1 AND uid=?2 ORDER BY recurrence_id"))?;
        let v = st.query_map(params![calendar_id, uid], ev_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn events_of_calendar(&self, calendar_id: i64) -> Result<Vec<Event>> {
        let mut st = self.db.prepare(&format!("SELECT {EV_COLS} FROM events WHERE calendar_id=?1"))?;
        let v = st.query_map([calendar_id], ev_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Candidates for a time range: single events overlapping it, plus every recurring master.
    pub fn events_for_range(&self, from: i64, to: i64) -> Result<Vec<Event>> {
        let mut st = self.db.prepare(&format!(
            "SELECT {EV_COLS} FROM events WHERE (rrule != '' AND start < ?2) OR (start < ?2 AND end >= ?1) ORDER BY start"
        ))?;
        let v = st.query_map(params![from, to], ev_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// Delete all events of a calendar whose href is not in `keep`.
    pub fn delete_missing(&self, calendar_id: i64, keep: &[String]) -> Result<usize> {
        let evs = self.events_of_calendar(calendar_id)?;
        let mut n = 0;
        for e in evs {
            if !e.href.is_empty() && !keep.contains(&e.href) {
                self.delete_event(e.id)?;
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn clear_calendar(&self, calendar_id: i64) -> Result<()> {
        self.db.execute("DELETE FROM events WHERE calendar_id=?1", [calendar_id])?;
        Ok(())
    }

    // ---- outbox -----------------------------------------------------------------------------

    pub fn queue(&self, calendar_id: i64, op: &str, uid: &str, href: &str, etag: &str) -> Result<()> {
        // a newer change of the same object replaces older ones
        self.db.execute("DELETE FROM outbox WHERE calendar_id=?1 AND uid=?2", params![calendar_id, uid])?;
        self.db.execute(
            "INSERT INTO outbox(calendar_id, op, uid, href, etag, created) VALUES(?1,?2,?3,?4,?5,strftime('%s','now'))",
            params![calendar_id, op, uid, href, etag],
        )?;
        Ok(())
    }

    pub fn outbox(&self) -> Result<Vec<OutboxItem>> {
        let mut st = self.db.prepare("SELECT id, calendar_id, op, uid, href, etag FROM outbox ORDER BY id")?;
        let v = st
            .query_map([], |r| {
                Ok(OutboxItem { id: r.get(0)?, calendar_id: r.get(1)?, op: r.get(2)?, uid: r.get(3)?, href: r.get(4)?, etag: r.get(5)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn outbox_done(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM outbox WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn pending_uids(&self, calendar_id: i64) -> Result<Vec<String>> {
        let mut st = self.db.prepare("SELECT uid FROM outbox WHERE calendar_id=?1")?;
        let v = st.query_map([calendar_id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---- reminders --------------------------------------------------------------------------

    /// Remember that a reminder was shown; false if it already was.
    pub fn mark_notified(&self, event_id: i64, occ: i64) -> Result<bool> {
        Ok(self.db.execute("INSERT OR IGNORE INTO notified(event_id, occ) VALUES(?1,?2)", params![event_id, occ])? > 0)
    }

    pub fn prune_notified(&self, before: i64) -> Result<()> {
        self.db.execute("DELETE FROM notified WHERE occ < ?1", [before])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_and_outbox() {
        let s = Store::memory().unwrap();
        let src = s.upsert_source("local", SourceKind::Local, "Local", "", "", "").unwrap();
        assert_eq!(s.upsert_source("local", SourceKind::Local, "Local", "", "", "").unwrap(), src);
        let cal = s.add_calendar(src, "", "Personal", "#2383e2", false).unwrap();
        let mut e = Event { calendar_id: cal, uid: "u1".into(), title: "A".into(), start: 100, end: 200, exdates: vec![5], ..Default::default() };
        e.id = s.save_event(&e).unwrap();
        assert_eq!(s.event(e.id).unwrap().unwrap(), e);
        e.title = "B".into();
        s.save_event(&e).unwrap();
        assert_eq!(s.event(e.id).unwrap().unwrap().title, "B");
        assert_eq!(s.events_for_range(150, 300).unwrap().len(), 1);
        assert_eq!(s.events_for_range(300, 400).unwrap().len(), 0);
        s.queue(cal, "put", "u1", "", "").unwrap();
        s.queue(cal, "delete", "u1", "/x.ics", "e").unwrap();
        assert_eq!(s.outbox().unwrap().len(), 1);
        assert!(s.mark_notified(e.id, 100).unwrap());
        assert!(!s.mark_notified(e.id, 100).unwrap());
        s.delete_source(src).unwrap();
        assert!(s.event(e.id).unwrap().is_none());
    }
}
