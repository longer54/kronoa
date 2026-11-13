//! A small blocking CalDAV client (RFC 4791) on top of ureq.
//!
//! Discovery: current-user-principal → calendar-home-set → calendar collections. Sync uses the
//! collection ctag (`getctag`) to skip unchanged calendars and per-object ETags to fetch only
//! changed events (calendar-multiget). Writes are `PUT` with `If-Match` / `If-None-Match`.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use std::time::Duration;

pub struct Client {
    agent: ureq::Agent,
    auth: String,
}

/// A response to a PROPFIND/REPORT: one `<response>` element.
#[derive(Debug, Default, Clone)]
pub struct DavItem {
    pub href: String,
    pub is_calendar: bool,
    pub is_collection: bool,
    pub supports_events: bool,
    pub display_name: String,
    pub color: String,
    pub ctag: String,
    pub sync_token: String,
    pub etag: String,
    pub data: String,
    pub read_only: bool,
    pub principal: String,
    pub home: String,
    pub status_ok: bool,
}

/// A discovered calendar collection.
#[derive(Debug, Clone)]
pub struct RemoteCalendar {
    pub url: String,
    pub name: String,
    pub color: String,
    pub ctag: String,
    pub read_only: bool,
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(40)))
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .user_agent(concat!("Kronoa/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Resolve `href` against `base` (absolute URL).
pub fn join(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = base[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(base.len());
    if href.starts_with('/') {
        format!("{}{}", &base[..host_end], href)
    } else {
        let dir_end = base.rfind('/').filter(|i| *i >= host_end).map(|i| i + 1).unwrap_or(base.len());
        let mut b = base[..dir_end].to_string();
        if !b.ends_with('/') {
            b.push('/');
        }
        format!("{b}{href}")
    }
}

fn path_of(url: &str) -> String {
    let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
    url[scheme_end..].find('/').map(|i| url[scheme_end + i..].to_string()).unwrap_or_else(|| "/".into())
}

/// Same resource? (compares paths, ignoring a trailing slash and percent-encoding of '@').
pub fn same(a: &str, b: &str) -> bool {
    let n = |s: &str| path_of(s).trim_end_matches('/').replace("%40", "@").to_lowercase();
    n(a) == n(b)
}

impl Client {
    pub fn new(username: &str, password: &str) -> Client {
        let auth = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}")));
        Client { agent: agent(), auth }
    }

    fn request(&self, method: &str, url: &str, depth: Option<&str>, headers: &[(&str, &str)], body: Option<String>) -> Result<(u16, String, String, String)> {
        let mut b = ureq::http::Request::builder()
            .method(ureq::http::Method::from_bytes(method.as_bytes())?)
            .uri(url)
            .header("Authorization", &self.auth);
        if let Some(d) = depth {
            b = b.header("Depth", d);
        }
        if body.is_some() {
            let ct = if method == "PUT" { "text/calendar; charset=utf-8" } else { "application/xml; charset=utf-8" };
            b = b.header("Content-Type", ct);
        }
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let mut resp = match body {
            Some(s) => self.agent.run(b.body(s)?),
            None => self.agent.run(b.body(())?),
        }
        .with_context(|| format!("{method} {url}"))?;
        let status = resp.status().as_u16();
        let header = |k: &str| resp.headers().get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let etag = header("etag");
        let location = header("location");
        let text = resp.body_mut().with_config().limit(64 << 20).read_to_string().unwrap_or_default();
        Ok((status, text, etag, location))
    }

    fn propfind(&self, url: &str, depth: &str, props: &str) -> Result<Vec<DavItem>> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/"><d:prop>{props}</d:prop></d:propfind>"#
        );
        let (status, text, _, _) = self.request("PROPFIND", url, Some(depth), &[], Some(body))?;
        match status {
            207 => parse_multistatus(&text),
            401 | 403 => bail!("authentication failed (HTTP {status})"),
            _ => bail!("PROPFIND {url}: HTTP {status}"),
        }
    }

    /// Find the calendars of an account. `url` may be the server root, a `.well-known` URL,
    /// a principal or a calendar home.
    pub fn discover(&self, url: &str) -> Result<Vec<RemoteCalendar>> {
        let url = url.trim().replace("webcal://", "https://");
        let url = if url.contains("://") { url } else { format!("https://{url}") };
        let mut candidates = vec![url.clone()];
        let root = join(&url, "/");
        candidates.push(join(&root, "/.well-known/caldav"));
        let mut last_err = anyhow!("no CalDAV service found at {url}");
        for start in candidates {
            match self.discover_from(&start) {
                Ok(v) if !v.is_empty() => return Ok(v),
                Ok(_) => last_err = anyhow!("no calendars found at {start}"),
                Err(e) => {
                    if e.to_string().contains("authentication") {
                        return Err(e);
                    }
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    fn discover_from(&self, start: &str) -> Result<Vec<RemoteCalendar>> {
        let items = self.propfind(start, "0", "<d:current-user-principal/><c:calendar-home-set/><d:resourcetype/>")?;
        let first = items.first().cloned().unwrap_or_default();
        let mut home = first.home.clone();
        if home.is_empty() && !first.principal.is_empty() {
            let p = join(start, &first.principal);
            let items = self.propfind(&p, "0", "<c:calendar-home-set/>")?;
            home = items.first().map(|i| i.home.clone()).unwrap_or_default();
        }
        // The URL itself may already be a calendar home (or a calendar).
        let home_url = if home.is_empty() { start.to_string() } else { join(start, &home) };
        let items = self.propfind(
            &home_url,
            "1",
            "<d:resourcetype/><d:displayname/><ic:calendar-color/><cs:getctag/><d:sync-token/><c:supported-calendar-component-set/><d:current-user-privilege-set/>",
        )?;
        Ok(items
            .into_iter()
            .filter(|i| i.is_calendar && i.supports_events)
            .map(|i| RemoteCalendar {
                url: join(&home_url, &i.href),
                name: if i.display_name.is_empty() { i.href.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_string() } else { i.display_name },
                color: normalize_color(&i.color),
                ctag: if i.ctag.is_empty() { i.sync_token } else { i.ctag },
                read_only: i.read_only,
            })
            .collect())
    }

    /// The collection's ctag (or sync-token when the server has no ctag).
    pub fn ctag(&self, cal_url: &str) -> Result<String> {
        let items = self.propfind(cal_url, "0", "<cs:getctag/><d:sync-token/>")?;
        Ok(items.first().map(|i| if i.ctag.is_empty() { i.sync_token.clone() } else { i.ctag.clone() }).unwrap_or_default())
    }

    /// (href, etag) of every event object in the collection.
    pub fn list(&self, cal_url: &str) -> Result<Vec<(String, String)>> {
        let body = r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"/></c:comp-filter></c:filter></c:calendar-query>"#;
        let (status, text, _, _) = self.request("REPORT", cal_url, Some("1"), &[], Some(body.into()))?;
        let items = if status == 207 { parse_multistatus(&text)? } else { self.propfind(cal_url, "1", "<d:getetag/><d:resourcetype/>")? };
        Ok(items
            .into_iter()
            .filter(|i| !i.is_collection && !i.etag.is_empty() && !same(&join(cal_url, &i.href), cal_url))
            .map(|i| (join(cal_url, &i.href), i.etag))
            .collect())
    }

    /// Fetch objects by URL: (url, etag, calendar data).
    pub fn multiget(&self, cal_url: &str, urls: &[String]) -> Result<Vec<(String, String, String)>> {
        let mut out = vec![];
        for chunk in urls.chunks(40) {
            let hrefs: String = chunk.iter().map(|u| format!("<d:href>{}</d:href>", xml_escape(&path_of(u)))).collect();
            let body = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop>{hrefs}</c:calendar-multiget>"#
            );
            let (status, text, _, _) = self.request("REPORT", cal_url, Some("1"), &[], Some(body))?;
            if status == 207 {
                for i in parse_multistatus(&text)? {
                    if !i.data.is_empty() {
                        out.push((join(cal_url, &i.href), i.etag, i.data));
                    }
                }
            } else {
                // servers without multiget: GET one by one
                for u in chunk {
                    let (s, text, etag, _) = self.request("GET", u, None, &[], None)?;
                    if s == 200 {
                        out.push((u.clone(), etag, text));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Create or update an object. Returns the new ETag ("" if the server did not send one).
    pub fn put(&self, url: &str, ics: &str, etag: &str) -> Result<String> {
        let cond: Vec<(&str, &str)> = if etag.is_empty() { vec![("If-None-Match", "*")] } else { vec![("If-Match", etag)] };
        let (status, _, new_etag, _) = self.request("PUT", url, None, &cond, Some(ics.to_string()))?;
        match status {
            200..=299 => Ok(new_etag),
            412 => bail!("conflict: the event was changed on the server (HTTP 412)"),
            401 | 403 => bail!("not allowed (HTTP {status})"),
            _ => bail!("PUT {url}: HTTP {status}"),
        }
    }

    pub fn delete(&self, url: &str, etag: &str) -> Result<()> {
        let cond: Vec<(&str, &str)> = if etag.is_empty() { vec![] } else { vec![("If-Match", etag)] };
        let (status, _, _, _) = self.request("DELETE", url, None, &cond, None)?;
        match status {
            200..=299 | 404 | 410 => Ok(()),
            412 => bail!("conflict: the event was changed on the server (HTTP 412)"),
            _ => bail!("DELETE {url}: HTTP {status}"),
        }
    }

    /// Create a calendar collection (MKCALENDAR) — used by tests against a fresh server.
    pub fn mkcalendar(&self, url: &str, name: &str) -> Result<()> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><c:mkcalendar xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:set><d:prop><d:displayname>{}</d:displayname><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set></d:prop></d:set></c:mkcalendar>"#,
            xml_escape(name)
        );
        let (status, _, _, _) = self.request("MKCALENDAR", url, None, &[], Some(body))?;
        if (200..300).contains(&status) || status == 405 {
            Ok(())
        } else {
            bail!("MKCALENDAR {url}: HTTP {status}")
        }
    }
}

/// Download a read-only iCalendar subscription. Returns `None` when unchanged (304).
pub fn fetch_ics(url: &str, etag: &str) -> Result<Option<(String, String)>> {
    let url = url.trim().replace("webcal://", "https://");
    let mut b = ureq::http::Request::builder().method("GET").uri(&url);
    if !etag.is_empty() {
        b = b.header("If-None-Match", etag);
    }
    let mut resp = agent().run(b.body(())?).with_context(|| format!("GET {url}"))?;
    let status = resp.status().as_u16();
    if status == 304 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        bail!("GET {url}: HTTP {status}");
    }
    let new_etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let text = resp.body_mut().with_config().limit(64 << 20).read_to_string()?;
    if !text.contains("BEGIN:VCALENDAR") {
        bail!("{url} is not an iCalendar file");
    }
    Ok(Some((text, new_etag)))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `#RRGGBBAA` (Apple) → `#rrggbb`.
pub fn normalize_color(c: &str) -> String {
    let c = c.trim();
    if c.starts_with('#') && (c.len() == 7 || c.len() == 9) {
        c[..7].to_lowercase()
    } else {
        String::new()
    }
}

/// Parse a DAV multistatus document.
pub fn parse_multistatus(xml: &str) -> Result<Vec<DavItem>> {
    let doc = roxmltree::Document::parse(xml).context("invalid XML from the server")?;
    let mut out = vec![];
    for resp in doc.descendants().filter(|n| n.tag_name().name() == "response") {
        let mut it = DavItem { read_only: true, ..Default::default() };
        let mut can_write = false;
        let mut has_privs = false;
        if let Some(h) = resp.children().find(|n| n.tag_name().name() == "href") {
            it.href = h.text().unwrap_or("").trim().to_string();
        }
        for ps in resp.children().filter(|n| n.tag_name().name() == "propstat") {
            let ok = ps
                .children()
                .find(|n| n.tag_name().name() == "status")
                .and_then(|s| s.text())
                .is_none_or(|s| s.contains(" 200"));
            if !ok {
                continue;
            }
            it.status_ok = true;
            let Some(prop) = ps.children().find(|n| n.tag_name().name() == "prop") else { continue };
            for p in prop.children().filter(|n| n.is_element()) {
                let text = || p.text().unwrap_or("").trim().to_string();
                let href_in = || {
                    p.descendants().find(|n| n.tag_name().name() == "href").and_then(|n| n.text()).unwrap_or("").trim().to_string()
                };
                match p.tag_name().name() {
                    "resourcetype" => {
                        for r in p.children().filter(|n| n.is_element()) {
                            match r.tag_name().name() {
                                "calendar" => it.is_calendar = true,
                                "collection" => it.is_collection = true,
                                _ => {}
                            }
                        }
                    }
                    "displayname" => it.display_name = text(),
                    "calendar-color" => it.color = text(),
                    "getctag" => it.ctag = text(),
                    "sync-token" => it.sync_token = text(),
                    "getetag" => it.etag = text(),
                    "calendar-data" => it.data = p.text().unwrap_or("").to_string(),
                    "current-user-principal" => it.principal = href_in(),
                    "calendar-home-set" => it.home = href_in(),
                    "supported-calendar-component-set" => {
                        it.supports_events = p.children().any(|c| c.attribute("name") == Some("VEVENT"));
                    }
                    "current-user-privilege-set" => {
                        has_privs = true;
                        can_write = p.descendants().any(|n| matches!(n.tag_name().name(), "write" | "write-content" | "all" | "bind"));
                    }
                    _ => {}
                }
            }
        }
        // no supported-component-set reported = assume events are supported
        if it.is_calendar && !xml.contains("supported-calendar-component-set") {
            it.supports_events = true;
        }
        if it.is_calendar && !it.supports_events {
            let declared_empty = resp.descendants().any(|n| n.tag_name().name() == "supported-calendar-component-set" && n.children().all(|c| !c.is_element()));
            it.supports_events = declared_empty;
        }
        it.read_only = has_privs && !can_write;
        out.push(it);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_urls() {
        assert_eq!(join("https://dav.example.org/cal/user/", "/cal/user/work/"), "https://dav.example.org/cal/user/work/");
        assert_eq!(join("https://dav.example.org/cal/user/", "work/"), "https://dav.example.org/cal/user/work/");
        assert_eq!(join("https://dav.example.org", "/x"), "https://dav.example.org/x");
        assert!(same("https://a.example/x/y/", "/x/y"));
    }

    #[test]
    fn multistatus() {
        let xml = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/">
<d:response><d:href>/dav/alice/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/dav/alice/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Work</d:displayname><ic:calendar-color>#FF0000FF</ic:calendar-color><cs:getctag>42</cs:getctag><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set><d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/dav/alice/tasks/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
        let v = parse_multistatus(xml).unwrap();
        assert_eq!(v.len(), 3);
        assert!(v[1].is_calendar && v[1].supports_events && v[1].read_only);
        assert_eq!(v[1].ctag, "42");
        assert_eq!(normalize_color(&v[1].color), "#ff0000");
        assert!(!v[2].supports_events);
    }
}
