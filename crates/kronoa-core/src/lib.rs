//! Kronoa calendar engine.
//!
//! Everything except drawing lives here: the data model, the local SQLite store with an
//! offline outbox, undo, recurrence expansion, the PL/EN quick-add parser, iCalendar
//! parsing/writing and the calendar sources (local, CalDAV, read-only ICS subscriptions).

pub mod caldav;
pub mod config;
pub mod demo;
pub mod engine;
pub mod google;
pub mod ical;
pub mod model;
pub mod quickadd;
pub mod recur;
pub mod secrets;
pub mod store;
pub mod sync;
pub mod time;

pub use engine::Core;
