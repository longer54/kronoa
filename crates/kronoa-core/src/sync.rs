//! Synchronisation with CalDAV accounts and ICS subscriptions.
//!
//! Runs on its own `Store` connection (SQLite in WAL mode), so the UI thread is never blocked.
//! CalDAV: push the offline outbox first, then pull what changed (ctag, then per-object etags).

use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, HashSet};

use crate::caldav::{self, Client};
use crate::config::{Config, Paths};
use crate::ical;
use crate::model::*;
use crate::secrets;
use crate::store::Store;

/// Check the settings of a new account; returns the number of calendars found.
pub fn test_account(kind: &str, url: &str, username: &str, password: &str) -> Result<usize> {
    match SourceKind::parse(kind) {
        SourceKind::Caldav => Ok(Client::new(username, password).discover(url)?.len()),
        SourceKind::Ics => {
            caldav::fetch_ics(url, "")?;
            Ok(1)
        }
        _ => bail!("unsupported account type: {kind}"),
    }
}

/// Sync every remote source. One result per account id.
pub fn sync_all(paths: &Paths, config: &Config) -> Vec<(String, std::result::Result<String, String>)> {
    let store = match Store::open(&paths.db_file()) {
        Ok(s) => s,
        Err(e) => return vec![("".into(), Err(e.to_string()))],
    };
    let mut out = vec![];
    for (key, src) in store.sources().unwrap_or_default() {
        if matches!(src.kind, SourceKind::Local | SourceKind::Google) || !config.accounts.iter().any(|a| a.id == key) {
            continue;
        }
        let r = sync_source(&store, paths, &key, &src);
        match &r {
            Ok(msg) => {
                let _ = store.set_source_status(src.id, msg, true);
            }
            Err(e) => {
                let _ = store.set_source_status(src.id, &format!("error: {e}"), false);
            }
        }
        out.push((key, r.map_err(|e| e.to_string())));
    }
    out
}

pub fn sync_source(store: &Store, paths: &Paths, key: &str, src: &Source) -> Result<String> {
    match src.kind {
        SourceKind::Ics => sync_ics(store, src),
        SourceKind::Caldav => sync_caldav(store, paths, key, src),
        _ => Ok(String::new()),
    }
}

fn sync_ics(store: &Store, src: &Source) -> Result<String> {
    let mut cal = store.calendars_of(src.id)?.into_iter().next().ok_or_else(|| anyhow!("subscription has no calendar"))?;
    let Some((text, etag)) = caldav::fetch_ics(&src.url, &cal.ctag)? else { return Ok("up to date".into()) };
    let events = ical::parse_events(&text, "");
    store.clear_calendar(cal.id)?;
    for mut e in events {
        e.calendar_id = cal.id;
        store.save_event(&e)?;
    }
    cal.ctag = etag;
    store.update_calendar(&cal)?;
    Ok("ok".into())
}

fn resource_name(uid: &str) -> String {
    let s: String = uid.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    format!("{s}.ics")
}

fn push(store: &Store, client: &Client) -> Result<usize> {
    let mut pushed = 0;
    for item in store.outbox()? {
        let Some(mut cal) = store.calendar(item.calendar_id)? else {
            store.outbox_done(item.id)?;
            continue;
        };
        let r: Result<()> = (|| {
            if item.op == "delete" {
                if !item.href.is_empty() {
                    client.delete(&item.href, &item.etag)?;
                }
                return Ok(());
            }
            let evs = store.events_by_uid(cal.id, &item.uid)?;
            if evs.is_empty() {
                return Ok(());
            }
            let url = match evs.iter().find(|e| !e.href.is_empty()) {
                Some(e) => e.href.clone(),
                None => caldav::join(&cal.href, &resource_name(&item.uid)),
            };
            let etag = evs.iter().find(|e| !e.etag.is_empty()).map(|e| e.etag.clone()).unwrap_or_default();
            let new_etag = client.put(&url, &ical::write_calendar(&evs), &etag)?;
            for mut e in evs {
                e.href = url.clone();
                e.etag = new_etag.clone();
                store.save_event(&e)?;
            }
            Ok(())
        })();
        match r {
            Ok(()) => {
                store.outbox_done(item.id)?;
                pushed += 1;
                cal.ctag.clear();
                store.update_calendar(&cal)?;
            }
            Err(e) if e.to_string().contains("conflict") => {
                // the server has a newer version: take it
                store.outbox_done(item.id)?;
                cal.ctag.clear();
                for mut ev in store.events_by_uid(cal.id, &item.uid)? {
                    ev.etag = "stale".into();
                    store.save_event(&ev)?;
                }
                store.update_calendar(&cal)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(pushed)
}

fn sync_caldav(store: &Store, paths: &Paths, key: &str, src: &Source) -> Result<String> {
    let password = secrets::load(paths, key).ok_or_else(|| anyhow!("no password stored for this account"))?;
    let client = Client::new(&src.username, &password);
    let pushed = push(store, &client)?;
    let remote = client.discover(&src.url)?;
    let me = if src.username.contains('@') { src.username.clone() } else { String::new() };
    let mut local = store.calendars_of(src.id)?;
    // calendars that disappeared on the server
    for c in local.clone() {
        if !remote.iter().any(|r| caldav::same(&r.url, &c.href)) {
            store.delete_calendar(c.id)?;
            local.retain(|x| x.id != c.id);
        }
    }
    let pending: Vec<_> = store.outbox()?;
    let mut changed = 0usize;
    for r in &remote {
        let mut cal = match local.iter().find(|c| caldav::same(&c.href, &r.url)) {
            Some(c) => c.clone(),
            None => {
                let color = if r.color.is_empty() { PALETTE[store.calendars()?.len() % PALETTE.len()].to_string() } else { r.color.clone() };
                let id = store.add_calendar(src.id, &r.url, &r.name, &color, r.read_only)?;
                store.calendar(id)?.unwrap()
            }
        };
        if cal.name != r.name || cal.read_only != r.read_only {
            cal.name = r.name.clone();
            cal.read_only = r.read_only;
            store.update_calendar(&cal)?;
        }
        let ctag = client.ctag(&cal.href).unwrap_or_else(|_| r.ctag.clone());
        if !ctag.is_empty() && ctag == cal.ctag && pushed == 0 {
            continue;
        }
        let listing = client.list(&cal.href)?;
        let have = store.events_of_calendar(cal.id)?;
        let mut etags: HashMap<String, String> = HashMap::new();
        for e in &have {
            if !e.href.is_empty() {
                etags.insert(e.href.clone(), e.etag.clone());
            }
        }
        let pend_uid: HashSet<String> = pending.iter().filter(|p| p.calendar_id == cal.id).map(|p| p.uid.clone()).collect();
        let pend_href: HashSet<String> = pending.iter().filter(|p| p.calendar_id == cal.id).map(|p| p.href.clone()).collect();
        let want: Vec<String> = listing
            .iter()
            .filter(|(h, e)| etags.get(h).map(|x| x != e).unwrap_or(true) && !pend_href.contains(h))
            .map(|(h, _)| h.clone())
            .collect();
        for (href, etag, data) in client.multiget(&cal.href, &want)? {
            let evs = ical::parse_events(&data, &me);
            if evs.iter().any(|e| pend_uid.contains(&e.uid)) {
                continue;
            }
            for old in have.iter().filter(|e| e.href == href) {
                store.delete_event(old.id)?;
            }
            for mut e in evs {
                // same UID filed under another URL (a moved object): replace it
                for old in store.events_by_uid(cal.id, &e.uid)? {
                    store.delete_event(old.id)?;
                }
                e.calendar_id = cal.id;
                e.href = href.clone();
                e.etag = etag.clone();
                store.save_event(&e)?;
            }
            changed += 1;
        }
        let keep: Vec<String> = listing.iter().map(|(h, _)| h.clone()).collect();
        changed += store.delete_missing(cal.id, &keep)?;
        cal.ctag = ctag;
        store.update_calendar(&cal)?;
    }
    Ok(format!("ok ({} calendars, {} sent, {} received)", remote.len(), pushed, changed))
}
