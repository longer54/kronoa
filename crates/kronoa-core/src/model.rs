//! Data model shared by the store, the sources and the UI bridge.

use serde::{Deserialize, Serialize};

/// Where a calendar comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Stored only on this computer.
    #[default]
    Local,
    /// A CalDAV account (Nextcloud, Fastmail, iCloud, Zimbra, Radicale…).
    Caldav,
    /// A read-only iCalendar subscription (`.ics` / `webcal://` URL).
    Ics,
    /// Google Calendar API with OAuth — planned, not implemented in 0.1.
    Google,
}

impl SourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::Local => "local",
            SourceKind::Caldav => "caldav",
            SourceKind::Ics => "ics",
            SourceKind::Google => "google",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "caldav" => SourceKind::Caldav,
            "ics" => SourceKind::Ics,
            "google" => SourceKind::Google,
            _ => SourceKind::Local,
        }
    }
}

/// An account or subscription that provides calendars.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: i64,
    pub kind: SourceKind,
    pub name: String,
    pub url: String,
    pub username: String,
    #[serde(skip)]
    pub secret_ref: String,
    pub status: String,
    pub last_sync: i64,
}

/// One calendar (a CalDAV collection, a subscription or a local calendar).
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub id: i64,
    pub source_id: i64,
    #[serde(skip)]
    pub href: String,
    pub name: String,
    pub color: String,
    pub visible: bool,
    pub read_only: bool,
    #[serde(skip)]
    pub ctag: String,
    #[serde(skip)]
    pub sync_token: String,
}

/// A stored event (a single event or the master of a series).
///
/// Times are Unix seconds. Timed events are real instants; all-day events are stored as
/// midnight UTC of their first day, with an exclusive `end`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Event {
    pub id: i64,
    pub calendar_id: i64,
    pub uid: String,
    /// For an overridden occurrence of a series: the original start of that occurrence.
    pub recurrence_id: i64,
    pub title: String,
    pub location: String,
    pub description: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// IANA zone the event was written in ("" = floating/local).
    pub tzid: String,
    /// RRULE value without the `RRULE:` prefix ("" = single event).
    pub rrule: String,
    pub exdates: Vec<i64>,
    /// Minutes before the start; `None` = no reminder.
    pub reminder: Option<i32>,
    /// Not confirmed yet (STATUS:TENTATIVE): drawn with diagonal stripes.
    pub tentative: bool,
    /// My answer to an invitation: "" | "accepted" | "tentative" | "declined".
    pub rsvp: String,
    pub attendees: i32,
    #[serde(skip)]
    pub href: String,
    #[serde(skip)]
    pub etag: String,
    /// The original VEVENT as received from the server (unknown properties are kept on save).
    #[serde(skip)]
    pub raw: String,
}

/// One occurrence of an event in a time range, as shown in the views.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub id: i64,
    pub calendar_id: i64,
    pub title: String,
    pub location: String,
    /// Milliseconds since the epoch (JavaScript time).
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    /// For all-day occurrences: first day and exclusive last day, `YYYY-MM-DD`.
    pub start_date: String,
    pub end_date: String,
    pub tentative: bool,
    pub recurring: bool,
    pub color: String,
    pub read_only: bool,
    pub rsvp: String,
    pub attendees: i32,
    pub reminder: Option<i32>,
    pub link: String,
}

/// Colours given to new calendars, in order.
pub const PALETTE: [&str; 8] =
    ["#2383e2", "#0f9d58", "#e16259", "#9065b0", "#d9730d", "#0b8a8a", "#c14c8a", "#7a6a3a"];

/// The first video-call / web link found in `text` (Meet, Zoom, Teams, Jitsi or any https URL).
pub fn find_link(text: &str) -> String {
    for word in text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"') {
        let w = word.trim_matches(|c: char| c == ',' || c == ';' || c == ')' || c == '(');
        if w.starts_with("https://") || w.starts_with("http://") {
            return w.to_string();
        }
    }
    String::new()
}
