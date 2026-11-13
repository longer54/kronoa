//! Google Calendar: OAuth 2.0 for installed apps (loopback redirect + PKCE) and the Calendar API v3.
//!
//! * Sign-in: a one-shot HTTP listener on `127.0.0.1:<random port>`, the browser is opened with
//!   an authorization URL (S256 code challenge), the returned code is exchanged for a refresh
//!   token which is kept in the keyring (`secrets`), never in `config.toml`.
//! * Access tokens live in memory only and are refreshed on demand.
//! * Sync: every Google calendar becomes a calendar of the account; events are pulled
//!   incrementally with `syncToken` (full re-read on HTTP 410) and local edits are pushed from the
//!   offline outbox (insert / patch / delete). Series are stored as in iCalendar: a master with
//!   an RRULE plus overridden occurrences (`recurrence_id`). `Event::href` holds the Google event id.
//!
//! The OAuth client (`client_id` / `client_secret`) is the user's own: it is read from the
//! `[google]` section of `config.toml` or from `KRONOA_GOOGLE_CLIENT_ID` / `_SECRET`.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::caldav::agent;
use crate::config::{Config, GoogleConfig, Paths};
use crate::model::*;
use crate::secrets;
use crate::store::Store;
use crate::time::{self, Zone};

pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const API: &str = "https://www.googleapis.com/calendar/v3";

// ---- credentials -------------------------------------------------------------------------------

/// The user's OAuth client: environment first, then `[google]` in `config.toml`.
pub fn credentials(cfg: &GoogleConfig) -> Result<(String, String)> {
    let env = |k: &str| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let id = env("KRONOA_GOOGLE_CLIENT_ID").unwrap_or_else(|| cfg.client_id.trim().to_string());
    let secret = env("KRONOA_GOOGLE_CLIENT_SECRET").unwrap_or_else(|| cfg.client_secret.trim().to_string());
    if id.is_empty() || id.starts_with("YOUR_") {
        bail!("Google sign-in needs your own OAuth client: put client_id and client_secret in the [google] section of config.toml (see the README)");
    }
    Ok((id, secret))
}

pub fn configured(cfg: &GoogleConfig) -> bool {
    credentials(cfg).is_ok()
}

// ---- small helpers -----------------------------------------------------------------------------

/// Percent-encode everything except RFC 3986 unreserved characters.
pub fn urlencode(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

pub fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) => {
                    out.push(v);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&")
}

fn rand_hex() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ---- PKCE and the authorization URL ------------------------------------------------------------

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// S256 challenge of a verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn new_pkce() -> Pkce {
    let verifier = format!("{}{}", rand_hex(), rand_hex()); // 64 chars from [0-9a-f]
    let challenge = pkce_challenge(&verifier);
    Pkce { verifier, challenge }
}

pub fn auth_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let q = form(&[
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ]);
    format!("{AUTH_URL}?{q}")
}

// ---- loopback redirect receiver ----------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Callback {
    Code(String),
    Error(String),
    /// Not the redirect (favicon, scanners…).
    Ignore,
}

/// Interpret the request line of the redirect (`GET /?code=…&state=… HTTP/1.1`).
pub fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Callback::Ignore;
    }
    let target = parts.next().unwrap_or("");
    let Some((path, query)) = target.split_once('?') else { return Callback::Ignore };
    if path != "/" {
        return Callback::Ignore;
    }
    let mut code = None;
    let mut error = None;
    let mut state = None;
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "code" => code = Some(urldecode(v)),
            "error" => error = Some(urldecode(v)),
            "state" => state = Some(urldecode(v)),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Callback::Error("the sign-in response did not match this request (state)".into());
    }
    match (code, error) {
        (_, Some(e)) => Callback::Error(format!("Google refused the sign-in: {e}")),
        (Some(c), None) if !c.is_empty() => Callback::Code(c),
        _ => Callback::Error("the sign-in response contained no code".into()),
    }
}

pub struct Loopback {
    listener: TcpListener,
    pub port: u16,
}

impl Loopback {
    pub fn bind() -> Result<Loopback> {
        let listener = TcpListener::bind("127.0.0.1:0").context("cannot open a local port for the sign-in redirect")?;
        let port = listener.local_addr()?.port();
        Ok(Loopback { listener, port })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Wait for the browser redirect; returns the authorization code.
    pub fn wait(&self, state: &str, timeout: Duration, cancel: &AtomicBool) -> Result<String> {
        self.listener.set_nonblocking(true)?;
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                bail!("sign-in cancelled");
            }
            if Instant::now() > deadline {
                bail!("sign-in timed out (no answer from the browser)");
            }
            let mut stream = match self.listener.accept() {
                Ok((s, _)) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = [0u8; 8192];
            let mut n = 0;
            while n < buf.len() {
                match stream.read(&mut buf[n..]) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => {
                        n += k;
                        if buf[..n].windows(2).any(|w| w == b"\r\n") {
                            break;
                        }
                    }
                }
            }
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = head.lines().next().unwrap_or("");
            let (status, body, result) = match parse_callback(line, state) {
                Callback::Ignore => ("404 Not Found", "Not found".to_string(), None),
                Callback::Code(c) => (
                    "200 OK",
                    page("Kronoa", "Signed in. You can close this tab and go back to Kronoa. · Zalogowano — możesz zamknąć tę kartę i wrócić do Kronoi."),
                    Some(Ok(c)),
                ),
                Callback::Error(e) => ("200 OK", page("Kronoa", &format!("Sign-in failed: {e}")), Some(Err(e))),
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
            if let Some(r) = result {
                return r.map_err(|e| anyhow!(e));
            }
        }
    }
}

fn page(title: &str, msg: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><title>{t}</title><body style=\"font-family:sans-serif;text-align:center;margin-top:18vh\"><h2>{t}</h2><p>{m}</p></body>",
        t = esc(title),
        m = esc(msg)
    )
}

// ---- token endpoint ----------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct Tokens {
    pub access_token: String,
    pub expires_in: i64,
    pub refresh_token: Option<String>,
}

pub fn parse_token_response(status: u16, body: &str) -> Result<Tokens> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if status != 200 || v.get("access_token").is_none() {
        let err = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
        if err == "invalid_grant" {
            bail!("the Google sign-in expired or was revoked - sign in again");
        }
        let desc = v.get("error_description").and_then(|e| e.as_str()).unwrap_or("");
        bail!("Google token endpoint: HTTP {status} {err} {desc}");
    }
    Ok(Tokens {
        access_token: v["access_token"].as_str().unwrap_or("").to_string(),
        expires_in: v.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(3600),
        refresh_token: v.get("refresh_token").and_then(|x| x.as_str()).map(String::from),
    })
}

fn post_form(url: &str, pairs: &[(&str, &str)]) -> Result<(u16, String)> {
    let req = ureq::http::Request::builder()
        .method("POST")
        .uri(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form(pairs))?;
    let mut resp = agent().run(req).with_context(|| format!("POST {url}"))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(1 << 20).read_to_string().unwrap_or_default();
    Ok((status, text))
}

pub fn exchange_code(client_id: &str, client_secret: &str, code: &str, verifier: &str, redirect_uri: &str) -> Result<Tokens> {
    let mut p = vec![
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    if !client_secret.is_empty() {
        p.push(("client_secret", client_secret));
    }
    let (s, b) = post_form(TOKEN_URL, &p)?;
    parse_token_response(s, &b)
}

pub fn refresh(client_id: &str, client_secret: &str, refresh_token: &str) -> Result<Tokens> {
    let mut p = vec![("client_id", client_id), ("refresh_token", refresh_token), ("grant_type", "refresh_token")];
    if !client_secret.is_empty() {
        p.push(("client_secret", client_secret));
    }
    let (s, b) = post_form(TOKEN_URL, &p)?;
    parse_token_response(s, &b)
}

/// Best effort: tell Google the refresh token is no longer needed.
pub fn revoke(token: &str) {
    let _ = post_form(REVOKE_URL, &[("token", token)]);
}

static TOKENS: Mutex<Option<HashMap<String, (String, i64)>>> = Mutex::new(None);

/// A valid access token for the account `key` (refreshed from the stored refresh token).
pub fn access_token(paths: &Paths, key: &str, cfg: &GoogleConfig) -> Result<String> {
    let now = time::now();
    if let Some((t, exp)) = TOKENS.lock().unwrap().as_ref().and_then(|m| m.get(key).cloned()) {
        if exp > now + 60 {
            return Ok(t);
        }
    }
    let (id, secret) = credentials(cfg)?;
    let rt = secrets::load(paths, key).ok_or_else(|| anyhow!("no Google sign-in stored for this account - sign in again"))?;
    let t = refresh(&id, &secret, &rt)?;
    TOKENS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key.to_string(), (t.access_token.clone(), now + t.expires_in));
    Ok(t.access_token)
}

pub fn forget_token(key: &str) {
    if let Some(m) = TOKENS.lock().unwrap().as_mut() {
        m.remove(key);
    }
}

// ---- sign-in flow ------------------------------------------------------------------------------

pub struct Login {
    pub email: String,
    pub refresh_token: String,
}

/// The whole browser sign-in. `open` receives the authorization URL (it should open a browser).
pub fn login(client_id: &str, client_secret: &str, cancel: &AtomicBool, open: &dyn Fn(&str)) -> Result<Login> {
    let lb = Loopback::bind()?;
    let redirect = lb.redirect_uri();
    let pk = new_pkce();
    let state = rand_hex();
    open(&auth_url(client_id, &redirect, &pk.challenge, &state));
    let code = lb.wait(&state, Duration::from_secs(300), cancel)?;
    let t = exchange_code(client_id, client_secret, &code, &pk.verifier, &redirect)?;
    let refresh_token = t.refresh_token.ok_or_else(|| {
        anyhow!("Google returned no refresh token - remove Kronoa at https://myaccount.google.com/permissions and sign in again")
    })?;
    let email = Api::new(&t.access_token).primary_calendar_id()?;
    Ok(Login { email, refresh_token })
}

// ---- Calendar API ------------------------------------------------------------------------------

pub struct Api {
    token: String,
}

pub fn api_error(status: u16, body: &str) -> anyhow::Error {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("").to_string();
    let reason = v.pointer("/error/errors/0/reason").and_then(|m| m.as_str()).unwrap_or("");
    let hint = match reason {
        "accessNotConfigured" => " (enable the Google Calendar API in your Google Cloud project)",
        "rateLimitExceeded" | "userRateLimitExceeded" => " (rate limit - will retry on the next sync)",
        _ => "",
    };
    anyhow!("Google Calendar API: HTTP {status} {msg}{hint}")
}

impl Api {
    pub fn new(token: &str) -> Api {
        Api { token: token.to_string() }
    }

    /// (status, JSON body; `Null` when empty)
    pub fn call(&self, method: &str, path: &str, query: &[(&str, &str)], body: Option<&Value>) -> Result<(u16, Value)> {
        let mut url = format!("{API}{path}");
        if !query.is_empty() {
            url.push('?');
            url.push_str(&form(query));
        }
        let mut b = ureq::http::Request::builder().method(method).uri(&url).header("Authorization", format!("Bearer {}", self.token));
        let mut resp = match body {
            Some(v) => agent().run(b.header("Content-Type", "application/json").body(v.to_string())?),
            None => {
                b = b.header("Accept", "application/json");
                agent().run(b.body(())?)
            }
        }
        .with_context(|| format!("{method} {path}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().with_config().limit(64 << 20).read_to_string().unwrap_or_default();
        let v = if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text.clone())) };
        if status == 401 {
            bail!("HTTP 401: Google rejected the access token");
        }
        if status >= 400 && status != 404 && status != 409 && status != 410 {
            return Err(api_error(status, &text));
        }
        Ok((status, v))
    }

    pub fn primary_calendar_id(&self) -> Result<String> {
        let (s, v) = self.call("GET", "/users/me/calendarList/primary", &[], None)?;
        if s != 200 {
            return Err(api_error(s, &v.to_string()));
        }
        Ok(v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string())
    }

    pub fn calendar_list(&self) -> Result<Vec<GCalendar>> {
        let mut out = vec![];
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![("maxResults", "250"), ("minAccessRole", "reader")];
            if let Some(p) = &page {
                q.push(("pageToken", p));
            }
            let (s, v) = self.call("GET", "/users/me/calendarList", &q, None)?;
            if s != 200 {
                return Err(api_error(s, &v.to_string()));
            }
            let (items, next) = parse_calendar_list(&v);
            out.extend(items);
            match next {
                Some(n) => page = Some(n),
                None => return Ok(out),
            }
        }
    }

    pub fn delete_event(&self, cal: &str, id: &str) -> Result<()> {
        let (s, v) = self.call("DELETE", &format!("/calendars/{}/events/{}", urlencode(cal), urlencode(id)), &[("sendUpdates", "none")], None)?;
        match s {
            200..=299 | 404 | 410 => Ok(()),
            _ => Err(api_error(s, &v.to_string())),
        }
    }
}

// ---- calendars ---------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GCalendar {
    pub id: String,
    pub name: String,
    pub color: String,
    pub read_only: bool,
    pub primary: bool,
    pub selected: bool,
    pub timezone: String,
    /// Default popup reminder of the calendar in minutes.
    pub default_reminder: Option<i32>,
}

/// One page of `calendarList` → (calendars, next page token).
pub fn parse_calendar_list(v: &Value) -> (Vec<GCalendar>, Option<String>) {
    let mut out = vec![];
    for it in v.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default() {
        let s = |k: &str| it.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let role = s("accessRole");
        if role == "freeBusyReader" || s("id").is_empty() || it.get("deleted").and_then(|d| d.as_bool()) == Some(true) {
            continue;
        }
        let name = if !s("summaryOverride").is_empty() { s("summaryOverride") } else { s("summary") };
        let color = s("backgroundColor").to_lowercase();
        let default_reminder = it
            .get("defaultReminders")
            .and_then(|r| r.as_array())
            .and_then(|r| r.iter().find(|x| x.get("method").and_then(|m| m.as_str()) == Some("popup")))
            .and_then(|r| r.get("minutes"))
            .and_then(|m| m.as_i64())
            .map(|m| m as i32);
        out.push(GCalendar {
            id: s("id"),
            name,
            color: if color.starts_with('#') && color.len() == 7 { color } else { String::new() },
            read_only: !matches!(role.as_str(), "owner" | "writer"),
            primary: it.get("primary").and_then(|x| x.as_bool()).unwrap_or(false),
            selected: it.get("selected").and_then(|x| x.as_bool()).unwrap_or(false),
            timezone: s("timeZone"),
            default_reminder,
        });
    }
    (out, v.get("nextPageToken").and_then(|x| x.as_str()).map(String::from))
}

// ---- events: Google JSON → Kronoa -------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Parsed {
    /// Google's event id (stored in `Event::href`).
    pub gid: String,
    pub cancelled: bool,
    /// For instances of a series: the id of the series master.
    pub master_gid: String,
    pub event: Event,
}

/// `{ "date": … }` / `{ "dateTime": …, "timeZone": … }` → (instant, all day, tzid).
fn gtime(v: &Value) -> Option<(i64, bool, String)> {
    if let Some(d) = v.get("date").and_then(|x| x.as_str()) {
        let d = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
        return Some((time::date_ts(d), true, String::new()));
    }
    let dt = v.get("dateTime").and_then(|x| x.as_str())?;
    let ts = DateTime::parse_from_rfc3339(dt).ok()?.timestamp();
    Some((ts, false, v.get("timeZone").and_then(|x| x.as_str()).unwrap_or("").to_string()))
}

/// `EXDATE;TZID=Europe/Warsaw:20261120T100000,…` / `EXDATE:…Z` / `EXDATE;VALUE=DATE:20261120`.
fn parse_exdate(line: &str, default_tz: &str) -> Vec<i64> {
    let Some((head, vals)) = line.split_once(':') else { return vec![] };
    let tz = head.split(';').find_map(|p| p.strip_prefix("TZID=")).unwrap_or(default_tz);
    let zone = Zone::from_tzid(tz);
    vals.split(',')
        .filter_map(|v| {
            let v = v.trim();
            if v.len() == 8 {
                return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(time::date_ts);
            }
            let (body, utc) = match v.strip_suffix('Z') {
                Some(b) => (b, true),
                None => (v, false),
            };
            let n = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
            Some(if utc { n.and_utc().timestamp() } else { zone.to_ts(n) })
        })
        .collect()
}

/// Map one Google event resource. `cal_tz` is the calendar's zone, `default_reminder` the calendar default.
pub fn parse_event(v: &Value, cal_tz: &str, default_reminder: Option<i32>) -> Option<Parsed> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let gid = s("id");
    if gid.is_empty() {
        return None;
    }
    let cancelled = s("status") == "cancelled";
    let mut e = Event { uid: s("iCalUID"), href: gid.clone(), etag: s("etag"), ..Default::default() };
    if e.uid.is_empty() {
        e.uid = format!("{gid}@google.com");
    }
    let master_gid = s("recurringEventId");
    if let Some((ts, _, _)) = v.get("originalStartTime").and_then(gtime) {
        e.recurrence_id = ts;
    }
    if cancelled {
        return Some(Parsed { gid, cancelled, master_gid, event: e });
    }
    let (start, all_day, tz) = v.get("start").and_then(gtime)?;
    let (end, _, _) = v.get("end").and_then(gtime).unwrap_or((start, all_day, String::new()));
    e.start = start;
    e.all_day = all_day;
    e.end = if end > start { end } else { start + if all_day { 86_400 } else { 3600 } };
    e.tzid = if all_day { String::new() } else if !tz.is_empty() { tz } else { cal_tz.to_string() };
    e.title = s("summary");
    e.location = s("location");
    e.description = s("description");
    e.tentative = s("status") == "tentative";
    for line in v.get("recurrence").and_then(|r| r.as_array()).cloned().unwrap_or_default() {
        let line = line.as_str().unwrap_or("").to_string();
        if let Some(r) = line.strip_prefix("RRULE:") {
            if e.rrule.is_empty() {
                e.rrule = r.to_string();
            }
        } else if line.starts_with("EXDATE") {
            e.exdates.extend(parse_exdate(&line, &e.tzid));
        }
    }
    e.reminder = match v.get("reminders") {
        Some(r) if r.get("useDefault").and_then(|d| d.as_bool()) == Some(true) => default_reminder,
        Some(r) => r
            .get("overrides")
            .and_then(|o| o.as_array())
            .and_then(|o| o.iter().find(|x| x.get("method").and_then(|m| m.as_str()) == Some("popup")).or_else(|| o.first()))
            .and_then(|o| o.get("minutes"))
            .and_then(|m| m.as_i64())
            .map(|m| m as i32),
        None => default_reminder,
    };
    if let Some(att) = v.get("attendees").and_then(|a| a.as_array()) {
        e.attendees = att.len() as i32;
        if let Some(me) = att.iter().find(|a| a.get("self").and_then(|x| x.as_bool()) == Some(true)) {
            e.rsvp = match me.get("responseStatus").and_then(|x| x.as_str()).unwrap_or("") {
                st @ ("accepted" | "tentative" | "declined") => st.to_string(),
                _ => String::new(),
            };
        }
    }
    Some(Parsed { gid, cancelled, master_gid, event: e })
}

// ---- events: Kronoa → Google JSON --------------------------------------------------------------

/// The id Google gives an instance of a series: `<master id>_<original start>`.
pub fn instance_id(master_gid: &str, recurrence_id: i64, all_day: bool) -> String {
    let t = DateTime::<Utc>::from_timestamp(recurrence_id, 0).unwrap_or_default();
    format!("{master_gid}_{}", t.format(if all_day { "%Y%m%d" } else { "%Y%m%dT%H%M%SZ" }))
}

fn rfc3339(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn valid_tz(tz: &str) -> String {
    if tz.parse::<chrono_tz::Tz>().is_ok() {
        tz.to_string()
    } else {
        let l = time::local_tzid();
        if l.parse::<chrono_tz::Tz>().is_ok() { l } else { "UTC".into() }
    }
}

/// The body for `events.insert` / `events.patch`.
pub fn to_google(e: &Event) -> Value {
    let (start, end) = if e.all_day {
        (
            json!({ "date": time::ymd(time::ts_date(e.start)), "dateTime": null, "timeZone": null }),
            json!({ "date": time::ymd(time::ts_date(e.end.max(e.start + 86_400))), "dateTime": null, "timeZone": null }),
        )
    } else {
        let tz = valid_tz(&e.tzid);
        (
            json!({ "dateTime": rfc3339(e.start), "timeZone": tz, "date": null }),
            json!({ "dateTime": rfc3339(e.end.max(e.start + 60)), "timeZone": tz, "date": null }),
        )
    };
    let mut v = json!({
        "summary": e.title,
        "location": e.location,
        "description": e.description,
        "start": start,
        "end": end,
        "status": if e.tentative { "tentative" } else { "confirmed" },
        "reminders": match e.reminder {
            Some(m) => json!({ "useDefault": false, "overrides": [{ "method": "popup", "minutes": m.clamp(0, 40320) }] }),
            None => json!({ "useDefault": false, "overrides": [] }),
        },
    });
    if e.recurrence_id == 0 {
        let mut rec = vec![];
        if !e.rrule.is_empty() {
            rec.push(format!("RRULE:{}", e.rrule));
            for x in &e.exdates {
                let t = DateTime::<Utc>::from_timestamp(*x, 0).unwrap_or_default();
                rec.push(if e.all_day { format!("EXDATE;VALUE=DATE:{}", t.format("%Y%m%d")) } else { format!("EXDATE:{}", t.format("%Y%m%dT%H%M%SZ")) });
            }
        }
        v["recurrence"] = if rec.is_empty() { Value::Null } else { json!(rec) };
    }
    v
}

/// Drop `null` members (they are only meaningful in a PATCH).
pub fn strip_nulls(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().filter(|(_, x)| !x.is_null()).map(|(k, x)| (k.clone(), strip_nulls(x))).collect()),
        o => o.clone(),
    }
}

// ---- sync --------------------------------------------------------------------------------------

/// Pull results into the store. Returns the number of changed events.
pub fn apply_items(store: &Store, cal: &Calendar, items: Vec<Parsed>, pending: &HashSet<String>, by_href: &mut HashMap<String, i64>) -> Result<usize> {
    let mut changed = 0;
    for it in items {
        if it.cancelled {
            if !it.master_gid.is_empty() {
                // one occurrence of a series was cancelled: drop its override and exclude the date
                if let Some(id) = by_href.remove(&it.gid) {
                    store.delete_event(id)?;
                }
                if let Some(mid) = by_href.get(&it.master_gid).copied() {
                    if let Some(mut m) = store.event(mid)? {
                        if it.event.recurrence_id != 0 && !m.exdates.contains(&it.event.recurrence_id) && !pending.contains(&m.uid) {
                            m.exdates.push(it.event.recurrence_id);
                            store.save_event(&m)?;
                        }
                    }
                }
            } else if let Some(id) = by_href.remove(&it.gid) {
                if let Some(old) = store.event(id)? {
                    if pending.contains(&old.uid) {
                        by_href.insert(it.gid, id);
                        continue;
                    }
                    for x in store.events_by_uid(cal.id, &old.uid)? {
                        by_href.retain(|_, v| *v != x.id);
                        store.delete_event(x.id)?;
                    }
                }
            }
            changed += 1;
            continue;
        }
        if pending.contains(&it.event.uid) {
            continue;
        }
        if let Some(id) = by_href.remove(&it.gid) {
            store.delete_event(id)?;
        }
        let mut e = it.event;
        e.calendar_id = cal.id;
        let id = store.save_event(&e)?;
        by_href.insert(it.gid, id);
        changed += 1;
    }
    Ok(changed)
}

fn events_page(api: &Api, cal: &Calendar, sync_token: &str, page: Option<&str>) -> Result<(u16, Value)> {
    let timemin = rfc3339(time::now() - 365 * 86_400);
    let mut q: Vec<(&str, &str)> = vec![("maxResults", "2500"), ("showDeleted", "true"), ("singleEvents", "false")];
    if sync_token.is_empty() {
        q.push(("timeMin", &timemin));
    } else {
        q.push(("syncToken", sync_token));
    }
    if let Some(p) = page {
        q.push(("pageToken", p));
    }
    api.call("GET", &format!("/calendars/{}/events", urlencode(&cal.href)), &q, None)
}

fn pull_calendar(store: &Store, api: &Api, cal: &mut Calendar, g: &GCalendar, pending: &HashSet<String>) -> Result<usize> {
    let mut token = cal.sync_token.clone();
    for attempt in 0..2 {
        let full = token.is_empty();
        let mut items: Vec<Parsed> = vec![];
        let mut page: Option<String> = None;
        let mut next_sync = String::new();
        let mut gone = false;
        loop {
            let (status, v) = events_page(api, cal, &token, page.as_deref())?;
            if status == 410 && !full {
                gone = true;
                break;
            }
            if status != 200 {
                return Err(api_error(status, &v.to_string()));
            }
            for it in v.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default() {
                if let Some(p) = parse_event(&it, &g.timezone, g.default_reminder) {
                    items.push(p);
                }
            }
            if let Some(n) = v.get("nextSyncToken").and_then(|x| x.as_str()) {
                next_sync = n.to_string();
            }
            match v.get("nextPageToken").and_then(|x| x.as_str()) {
                Some(p) => page = Some(p.to_string()),
                None => break,
            }
        }
        if gone && attempt == 0 {
            token.clear();
            cal.sync_token.clear();
            continue;
        }
        let mut by_href: HashMap<String, i64> = store.events_of_calendar(cal.id)?.into_iter().filter(|e| !e.href.is_empty()).map(|e| (e.href, e.id)).collect();
        // masters first, so that cancelled instances find their series
        items.sort_by_key(|p| (p.cancelled && !p.master_gid.is_empty()) as u8);
        let keep: Vec<String> = items.iter().filter(|p| !p.cancelled).map(|p| p.gid.clone()).collect();
        let mut changed = apply_items(store, cal, items, pending, &mut by_href)?;
        if full {
            let keep_set: HashSet<&String> = keep.iter().collect();
            for e in store.events_of_calendar(cal.id)? {
                if !e.href.is_empty() && !keep_set.contains(&e.href) && !pending.contains(&e.uid) {
                    store.delete_event(e.id)?;
                    changed += 1;
                }
            }
        }
        cal.sync_token = next_sync;
        store.update_calendar(cal)?;
        return Ok(changed);
    }
    bail!("could not synchronise calendar {}", cal.name)
}

fn push(store: &Store, api: &Api, source_id: i64) -> Result<usize> {
    let mut pushed = 0;
    for item in store.outbox()? {
        let Some(cal) = store.calendar(item.calendar_id)? else {
            store.outbox_done(item.id)?;
            continue;
        };
        if cal.source_id != source_id {
            continue;
        }
        if item.op == "delete" {
            if !item.href.is_empty() {
                api.delete_event(&cal.href, &item.href)?;
            }
            store.outbox_done(item.id)?;
            pushed += 1;
            continue;
        }
        let mut evs = store.events_by_uid(cal.id, &item.uid)?;
        evs.sort_by_key(|e| e.recurrence_id != 0); // the series master first
        if evs.is_empty() {
            store.outbox_done(item.id)?;
            continue;
        }
        let mut master_gid = evs.iter().find(|e| e.recurrence_id == 0).map(|e| e.href.clone()).unwrap_or_default();
        let mut new_uid: Option<String> = None;
        let mut saved: Vec<(Event, String, String)> = vec![];
        for e in &evs {
            let (gid, is_new) = if !e.href.is_empty() {
                (e.href.clone(), false)
            } else if e.recurrence_id != 0 {
                if master_gid.is_empty() {
                    continue;
                }
                (instance_id(&master_gid, e.recurrence_id, e.all_day), false)
            } else {
                (rand_hex(), true)
            };
            let body = to_google(e);
            let base = format!("/calendars/{}/events", urlencode(&cal.href));
            let mut result = None;
            if is_new {
                let mut b = strip_nulls(&body);
                b["id"] = gid.clone().into();
                let (s, v) = api.call("POST", &base, &[("sendUpdates", "none")], Some(&b))?;
                match s {
                    200..=299 => result = Some(v),
                    409 => {} // the previous attempt got through: fall back to a patch
                    _ => return Err(api_error(s, &v.to_string())),
                }
            }
            if result.is_none() {
                let (s, v) = api.call("PATCH", &format!("{base}/{}", urlencode(&gid)), &[("sendUpdates", "none")], Some(&body))?;
                match s {
                    200..=299 => result = Some(v),
                    404 | 410 if !is_new => continue, // deleted on the server meanwhile; the next pull removes it
                    _ => return Err(api_error(s, &v.to_string())),
                }
            }
            let v = result.unwrap_or(Value::Null);
            let etag = v.get("etag").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if e.recurrence_id == 0 {
                master_gid = gid.clone();
                if let Some(u) = v.get("iCalUID").and_then(|x| x.as_str()) {
                    if u != e.uid {
                        new_uid = Some(u.to_string());
                    }
                }
            }
            saved.push((e.clone(), gid, etag));
        }
        for (mut e, gid, etag) in saved {
            e.href = gid;
            e.etag = etag;
            if let Some(u) = &new_uid {
                e.uid = u.clone();
            }
            store.save_event(&e)?;
        }
        store.outbox_done(item.id)?;
        pushed += 1;
    }
    Ok(pushed)
}

fn sync_with(store: &Store, api: &Api, src: &Source) -> Result<String> {
    let pushed = push(store, api, src.id)?;
    let remote = api.calendar_list()?;
    let mut local = store.calendars_of(src.id)?;
    for c in local.clone() {
        if !remote.iter().any(|r| r.id == c.href) {
            store.delete_calendar(c.id)?;
            local.retain(|x| x.id != c.id);
        }
    }
    let pending: HashSet<String> = store.outbox()?.into_iter().map(|p| p.uid).collect();
    let mut changed = 0usize;
    let mut errors: Vec<String> = vec![];
    for g in &remote {
        let mut cal = match local.iter().find(|c| c.href == g.id) {
            Some(c) => c.clone(),
            None => {
                let color = if g.color.is_empty() { PALETTE[store.calendars()?.len() % PALETTE.len()].to_string() } else { g.color.clone() };
                let id = store.add_calendar(src.id, &g.id, &g.name, &color, g.read_only)?;
                let mut c = store.calendar(id)?.ok_or_else(|| anyhow!("calendar vanished"))?;
                c.visible = g.selected || g.primary;
                c.ctag = format!("color:{}", g.color);
                store.update_calendar(&c)?;
                c
            }
        };
        let mut dirty = false;
        if cal.name != g.name || cal.read_only != g.read_only {
            cal.name = g.name.clone();
            cal.read_only = g.read_only;
            dirty = true;
        }
        // follow colour changes made in Google, but keep a colour chosen in Kronoa otherwise
        let seen = format!("color:{}", g.color);
        if cal.ctag != seen {
            if !g.color.is_empty() {
                cal.color = g.color.clone();
            }
            cal.ctag = seen;
            dirty = true;
        }
        if dirty {
            store.update_calendar(&cal)?;
        }
        match pull_calendar(store, api, &mut cal, g, &pending) {
            Ok(n) => changed += n,
            Err(e) => errors.push(format!("{}: {e}", g.name)),
        }
    }
    if !errors.is_empty() {
        bail!("{} ({} calendar(s) failed)", errors.join("; "), errors.len());
    }
    Ok(format!("ok ({} calendars, {} sent, {} received)", remote.len(), pushed, changed))
}

/// Sync one Google account (push local changes, then pull).
pub fn sync_account(store: &Store, paths: &Paths, config: &Config, key: &str, src: &Source) -> Result<String> {
    let mut last = None;
    for _ in 0..2 {
        let token = access_token(paths, key, &config.google)?;
        match sync_with(store, &Api::new(&token), src) {
            Err(e) if e.to_string().contains("HTTP 401") => {
                forget_token(key);
                last = Some(e);
            }
            r => return r,
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("Google sign-in failed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding() {
        assert_eq!(urlencode("a b/c@d"), "a%20b%2Fc%40d");
        assert_eq!(urldecode("4%2F0Abc+d%C5%82"), "4/0Abc dł");
        assert_eq!(urldecode("100%"), "100%");
    }

    #[test]
    fn pkce_rfc7636_vector() {
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let p = new_pkce();
        assert!(p.verifier.len() >= 43 && p.verifier.len() <= 128);
        assert_eq!(pkce_challenge(&p.verifier), p.challenge);
        assert_ne!(new_pkce().verifier, p.verifier);
    }

    #[test]
    fn authorization_url() {
        let u = auth_url("abc.apps.googleusercontent.com", "http://127.0.0.1:4567", "CHAL", "st8");
        assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(u.contains("client_id=abc.apps.googleusercontent.com"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A4567"));
        assert!(u.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar&"));
        assert!(u.contains("code_challenge=CHAL&code_challenge_method=S256"));
        assert!(u.contains("state=st8"));
        assert!(u.contains("access_type=offline") && u.contains("response_type=code"));
        assert!(!u.contains("client_secret"));
    }

    #[test]
    fn callback_parsing() {
        assert_eq!(parse_callback("GET /?code=4%2F0Ab&state=xyz&scope=s HTTP/1.1", "xyz"), Callback::Code("4/0Ab".into()));
        assert!(matches!(parse_callback("GET /?code=1&state=other HTTP/1.1", "xyz"), Callback::Error(_)));
        assert!(matches!(parse_callback("GET /?error=access_denied&state=xyz HTTP/1.1", "xyz"), Callback::Error(e) if e.contains("access_denied")));
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "xyz"), Callback::Ignore);
        assert_eq!(parse_callback("POST /?code=1&state=xyz HTTP/1.1", "xyz"), Callback::Ignore);
    }

    #[test]
    fn loopback_roundtrip() {
        let lb = Loopback::bind().unwrap();
        let port = lb.port;
        let h = std::thread::spawn(move || {
            for path in ["/favicon.ico", "/?code=THE%2FCODE&state=S1"] {
                let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut r = String::new();
                let _ = s.read_to_string(&mut r);
                assert!(r.starts_with("HTTP/1.1 "));
            }
        });
        let code = lb.wait("S1", Duration::from_secs(10), &AtomicBool::new(false)).unwrap();
        assert_eq!(code, "THE/CODE");
        h.join().unwrap();
    }

    #[test]
    fn loopback_cancel() {
        let lb = Loopback::bind().unwrap();
        let cancel = AtomicBool::new(true);
        assert!(lb.wait("s", Duration::from_secs(5), &cancel).unwrap_err().to_string().contains("cancelled"));
    }

    #[test]
    fn token_responses() {
        let t = parse_token_response(200, r#"{"access_token":"ya29.x","expires_in":3599,"refresh_token":"1//r","scope":"s","token_type":"Bearer"}"#).unwrap();
        assert_eq!(t, Tokens { access_token: "ya29.x".into(), expires_in: 3599, refresh_token: Some("1//r".into()) });
        let t = parse_token_response(200, r#"{"access_token":"a","expires_in":10}"#).unwrap();
        assert_eq!(t.refresh_token, None);
        let e = parse_token_response(400, r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#).unwrap_err();
        assert!(e.to_string().contains("sign in again"));
        assert!(parse_token_response(401, r#"{"error":"invalid_client"}"#).unwrap_err().to_string().contains("invalid_client"));
    }

    #[test]
    fn credentials_from_config() {
        let ok = GoogleConfig { client_id: "id.apps.googleusercontent.com".into(), client_secret: "sec".into() };
        if std::env::var_os("KRONOA_GOOGLE_CLIENT_ID").is_none() {
            assert_eq!(credentials(&ok).unwrap().0, "id.apps.googleusercontent.com");
            assert!(credentials(&GoogleConfig::default()).is_err());
            assert!(credentials(&GoogleConfig { client_id: "YOUR_CLIENT_ID".into(), ..Default::default() }).is_err());
        }
    }

    #[test]
    fn calendar_list() {
        let v: Value = serde_json::from_str(
            r##"{"kind":"calendar#calendarList","nextPageToken":"NP","items":[
            {"id":"me@gmail.com","summary":"me@gmail.com","primary":true,"selected":true,"accessRole":"owner","backgroundColor":"#9FE1E7","timeZone":"Europe/Warsaw","defaultReminders":[{"method":"popup","minutes":30}]},
            {"id":"pl.polish#holiday@group.v.calendar.google.com","summary":"Święta w Polsce","summaryOverride":"Święta","accessRole":"reader","backgroundColor":"#16a765"},
            {"id":"fb@x","summary":"busy","accessRole":"freeBusyReader"}]}"##,
        )
        .unwrap();
        let (c, next) = parse_calendar_list(&v);
        assert_eq!(next.as_deref(), Some("NP"));
        assert_eq!(c.len(), 2);
        assert!(c[0].primary && c[0].selected && !c[0].read_only);
        assert_eq!((c[0].color.as_str(), c[0].timezone.as_str(), c[0].default_reminder), ("#9fe1e7", "Europe/Warsaw", Some(30)));
        assert_eq!(c[1].name, "Święta");
        assert!(c[1].read_only && !c[1].selected);
    }

    fn ev(json: &str) -> Parsed {
        parse_event(&serde_json::from_str::<Value>(json).unwrap(), "Europe/Warsaw", Some(10)).unwrap()
    }

    #[test]
    fn timed_event() {
        let p = ev(r#"{"id":"abc","etag":"\"e1\"","iCalUID":"abc@google.com","status":"confirmed","summary":"Spotkanie","location":"Biuro","description":"Link https://meet.google.com/x",
          "start":{"dateTime":"2026-11-16T10:00:00+01:00","timeZone":"Europe/Warsaw"},"end":{"dateTime":"2026-11-16T11:30:00+01:00","timeZone":"Europe/Warsaw"},
          "reminders":{"useDefault":false,"overrides":[{"method":"popup","minutes":15}]},
          "attendees":[{"email":"a@x"},{"email":"me@x","self":true,"responseStatus":"accepted"}]}"#);
        let e = p.event;
        assert_eq!((e.uid.as_str(), e.href.as_str(), e.title.as_str()), ("abc@google.com", "abc", "Spotkanie"));
        assert_eq!(e.start, 1_794_819_600); // 2026-11-16 09:00:00 UTC
        assert_eq!(e.end - e.start, 5400);
        assert!(!e.all_day);
        assert_eq!(e.tzid, "Europe/Warsaw");
        assert_eq!((e.reminder, e.attendees, e.rsvp.as_str()), (Some(15), 2, "accepted"));
        assert_eq!(e.recurrence_id, 0);
    }

    #[test]
    fn all_day_event_and_default_reminder() {
        let e = ev(r#"{"id":"d1","summary":"Urlop","start":{"date":"2026-12-24"},"end":{"date":"2026-12-27"},"reminders":{"useDefault":true},"status":"tentative"}"#).event;
        assert!(e.all_day && e.tentative);
        assert_eq!(e.start, time::date_ts(NaiveDate::from_ymd_opt(2026, 12, 24).unwrap()));
        assert_eq!(e.end - e.start, 3 * 86_400);
        assert_eq!((e.tzid.as_str(), e.reminder, e.uid.as_str()), ("", Some(10), "d1@google.com"));
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap().and_utc().timestamp()
    }

    #[test]
    fn recurring_with_exdate_and_override() {
        let m = ev(r#"{"id":"r1","iCalUID":"r1@google.com","summary":"Daily","start":{"dateTime":"2026-11-16T09:00:00+01:00","timeZone":"Europe/Warsaw"},"end":{"dateTime":"2026-11-16T09:15:00+01:00","timeZone":"Europe/Warsaw"},
          "recurrence":["RRULE:FREQ=DAILY;COUNT=10","EXDATE;TZID=Europe/Warsaw:20261118T090000,20261119T090000","EXDATE:20261120T080000Z"]}"#)
        .event;
        assert_eq!(m.rrule, "FREQ=DAILY;COUNT=10");
        assert_eq!(m.exdates, vec![utc(2026, 11, 18, 8, 0), utc(2026, 11, 19, 8, 0), utc(2026, 11, 20, 8, 0)]);
        let o = ev(r#"{"id":"r1_20261117T080000Z","iCalUID":"r1@google.com","recurringEventId":"r1","summary":"Daily (moved)",
          "originalStartTime":{"dateTime":"2026-11-17T09:00:00+01:00","timeZone":"Europe/Warsaw"},
          "start":{"dateTime":"2026-11-17T14:00:00+01:00"},"end":{"dateTime":"2026-11-17T14:15:00+01:00"}}"#);
        assert_eq!(o.master_gid, "r1");
        assert_eq!(o.event.recurrence_id, utc(2026, 11, 17, 8, 0));
        assert_eq!(o.event.tzid, "Europe/Warsaw"); // from the calendar when the event has none
        assert_eq!(o.event.uid, "r1@google.com");
        assert_eq!(instance_id("r1", o.event.recurrence_id, false), "r1_20261117T080000Z");
        assert_eq!(instance_id("r1", time::date_ts(NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()), true), "r1_20261117");
    }

    #[test]
    fn cancelled_and_incomplete() {
        let c = ev(r#"{"id":"x_20261118T080000Z","status":"cancelled","recurringEventId":"x","originalStartTime":{"dateTime":"2026-11-18T09:00:00+01:00"}}"#);
        assert!(c.cancelled);
        assert_eq!(c.master_gid, "x");
        assert_eq!(c.event.recurrence_id, utc(2026, 11, 18, 8, 0));
        assert!(parse_event(&json!({"id":"nostart","summary":"?"}), "", None).is_none());
        assert!(parse_event(&json!({"summary":"noid"}), "", None).is_none());
    }

    #[test]
    fn to_google_timed_all_day_and_series() {
        let mut e = Event {
            title: "Plan".into(),
            start: utc(2026, 11, 16, 9, 0),
            end: utc(2026, 11, 16, 10, 0),
            tzid: "Europe/Warsaw".into(),
            reminder: Some(10),
            ..Default::default()
        };
        let g = to_google(&e);
        assert_eq!(g["start"]["dateTime"], "2026-11-16T09:00:00Z");
        assert_eq!(g["start"]["timeZone"], "Europe/Warsaw");
        assert_eq!(g["start"]["date"], Value::Null);
        assert_eq!(g["reminders"]["overrides"][0]["minutes"], 10);
        assert_eq!(g["recurrence"], Value::Null);
        assert_eq!(g["status"], "confirmed");

        e.rrule = "FREQ=WEEKLY;BYDAY=MO".into();
        e.exdates = vec![utc(2026, 11, 23, 9, 0)];
        e.reminder = None;
        let g = to_google(&e);
        assert_eq!(g["recurrence"], json!(["RRULE:FREQ=WEEKLY;BYDAY=MO", "EXDATE:20261123T090000Z"]));
        assert_eq!(g["reminders"]["overrides"], json!([]));

        let d = Event { all_day: true, start: time::date_ts(NaiveDate::from_ymd_opt(2026, 12, 24).unwrap()), end: time::date_ts(NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()), ..Default::default() };
        let g = to_google(&d);
        assert_eq!((g["start"]["date"].as_str(), g["end"]["date"].as_str()), (Some("2026-12-24"), Some("2026-12-25")));
        assert_eq!(g["start"]["dateTime"], Value::Null);

        // an overridden occurrence never carries a recurrence of its own
        let o = Event { recurrence_id: 5, start: 1000, end: 2000, ..Default::default() };
        assert!(to_google(&o).get("recurrence").is_none());
        // unknown time zone falls back to something valid
        let bad = Event { start: 0, end: 3600, tzid: "Nowhere/Land".into(), ..Default::default() };
        assert!(to_google(&bad)["start"]["timeZone"].as_str().unwrap().parse::<chrono_tz::Tz>().is_ok() || to_google(&bad)["start"]["timeZone"] == "UTC");
    }

    #[test]
    fn google_roundtrip() {
        let src = Event { title: "T".into(), location: "L".into(), description: "D".into(), start: utc(2026, 11, 16, 9, 0), end: utc(2026, 11, 16, 9, 45), tzid: "Europe/Warsaw".into(), rrule: "FREQ=DAILY;COUNT=3".into(), exdates: vec![utc(2026, 11, 17, 9, 0)], reminder: Some(5), ..Default::default() };
        let mut j = to_google(&src);
        j["id"] = "g1".into();
        let back = parse_event(&j, "Europe/Warsaw", None).unwrap().event;
        assert_eq!((back.title.as_str(), back.location.as_str(), back.start, back.end), ("T", "L", src.start, src.end));
        assert_eq!((back.rrule.as_str(), back.exdates.clone(), back.reminder), ("FREQ=DAILY;COUNT=3", src.exdates.clone(), Some(5)));
        assert_eq!(back.tzid, "Europe/Warsaw");
    }

    #[test]
    fn api_errors_are_readable() {
        let e = api_error(403, r#"{"error":{"code":403,"message":"Google Calendar API has not been used in project 1 before or it is disabled.","errors":[{"reason":"accessNotConfigured"}]}}"#);
        let m = e.to_string();
        assert!(m.contains("HTTP 403") && m.contains("enable the Google Calendar API"));
    }

    fn setup() -> (Store, Calendar) {
        let s = Store::memory().unwrap();
        let sid = s.upsert_source("g", SourceKind::Google, "Google", "", "me@x", "").unwrap();
        let cid = s.add_calendar(sid, "me@x", "me", "#2383e2", false).unwrap();
        let c = s.calendar(cid).unwrap().unwrap();
        (s, c)
    }

    fn items(list: &[&str]) -> Vec<Parsed> {
        list.iter().map(|j| ev(j)).collect()
    }

    const MASTER: &str = r#"{"id":"m1","iCalUID":"m1@google.com","summary":"Series","start":{"dateTime":"2026-11-16T09:00:00+01:00","timeZone":"Europe/Warsaw"},"end":{"dateTime":"2026-11-16T10:00:00+01:00","timeZone":"Europe/Warsaw"},"recurrence":["RRULE:FREQ=DAILY"]}"#;
    const OVERRIDE: &str = r#"{"id":"m1_20261117T080000Z","iCalUID":"m1@google.com","recurringEventId":"m1","summary":"Moved","originalStartTime":{"dateTime":"2026-11-17T09:00:00+01:00"},"start":{"dateTime":"2026-11-17T12:00:00+01:00"},"end":{"dateTime":"2026-11-17T13:00:00+01:00"}}"#;
    const CANCELLED: &str = r#"{"id":"m1_20261118T080000Z","status":"cancelled","recurringEventId":"m1","originalStartTime":{"dateTime":"2026-11-18T09:00:00+01:00"}}"#;
    const SINGLE: &str = r#"{"id":"s1","summary":"One","start":{"date":"2026-11-20"},"end":{"date":"2026-11-21"}}"#;

    #[test]
    fn apply_series_override_and_cancellation() {
        let (s, cal) = setup();
        let mut by = HashMap::new();
        let n = apply_items(&s, &cal, items(&[MASTER, OVERRIDE, SINGLE]), &HashSet::new(), &mut by).unwrap();
        assert_eq!(n, 3);
        let evs = s.events_of_calendar(cal.id).unwrap();
        assert_eq!(evs.len(), 3);
        assert_eq!(evs.iter().filter(|e| e.recurrence_id != 0).count(), 1);
        assert!(evs.iter().any(|e| e.all_day && e.title == "One"));
        // an incremental pull: one occurrence cancelled → exdate on the master
        apply_items(&s, &cal, items(&[CANCELLED]), &HashSet::new(), &mut by).unwrap();
        let m = s.events_by_uid(cal.id, "m1@google.com").unwrap().into_iter().find(|e| e.recurrence_id == 0).unwrap();
        assert_eq!(m.exdates, vec![utc(2026, 11, 18, 8, 0)]);
        // the same change twice does not duplicate anything
        apply_items(&s, &cal, items(&[MASTER, OVERRIDE]), &HashSet::new(), &mut by).unwrap();
        assert_eq!(s.events_of_calendar(cal.id).unwrap().len(), 3);
        // a cancelled override removes it; a cancelled master removes the whole series
        let gone = r#"{"id":"m1_20261117T080000Z","status":"cancelled","recurringEventId":"m1","originalStartTime":{"dateTime":"2026-11-17T09:00:00+01:00"}}"#;
        apply_items(&s, &cal, items(&[gone]), &HashSet::new(), &mut by).unwrap();
        assert_eq!(s.events_of_calendar(cal.id).unwrap().len(), 2);
        apply_items(&s, &cal, items(&[r#"{"id":"m1","status":"cancelled"}"#, r#"{"id":"s1","status":"cancelled"}"#]), &HashSet::new(), &mut by).unwrap();
        assert!(s.events_of_calendar(cal.id).unwrap().is_empty());
    }

    #[test]
    fn pending_local_changes_are_not_overwritten() {
        let (s, cal) = setup();
        let mut by = HashMap::new();
        apply_items(&s, &cal, items(&[SINGLE]), &HashSet::new(), &mut by).unwrap();
        let mut local = s.events_of_calendar(cal.id).unwrap().remove(0);
        local.title = "Edited here".into();
        s.save_event(&local).unwrap();
        let pending: HashSet<String> = [local.uid.clone()].into();
        apply_items(&s, &cal, items(&[SINGLE]), &pending, &mut by).unwrap();
        assert_eq!(s.events_of_calendar(cal.id).unwrap()[0].title, "Edited here");
        apply_items(&s, &cal, items(&[r#"{"id":"s1","status":"cancelled"}"#]), &pending, &mut by).unwrap();
        assert_eq!(s.events_of_calendar(cal.id).unwrap().len(), 1);
    }
}
