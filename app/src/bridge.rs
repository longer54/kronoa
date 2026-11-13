//! QML-facing backend. The engine (`kronoa_core::engine::Core`) is a process-wide singleton behind a
//! mutex; reads and edits hit only the local SQLite store (fast, synchronous). Network work runs on
//! a plain thread and reports back to the Qt thread through a signal.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qguiapplication.h");
        type QGuiApplication = cxx_qt_lib::QGuiApplication;
        include!("application.h");
        fn kronoa_new_application(args: &[String]) -> UniquePtr<QGuiApplication>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type Backend = super::BackendRust;

        #[qinvokable]
        fn app_name(self: &Backend) -> QString;
        #[qinvokable]
        fn version(self: &Backend) -> QString;
        /// Interface language actually in use: "en" or "pl".
        #[qinvokable]
        fn lang(self: &Backend) -> QString;
        #[qinvokable]
        fn settings_json(self: &Backend) -> QString;
        /// Change one setting (`value_json` is a JSON value). Returns an error text or "".
        #[qinvokable]
        fn set_setting(self: &Backend, key: &QString, value_json: &QString) -> QString;
        #[qinvokable]
        fn calendars_json(self: &Backend) -> QString;
        #[qinvokable]
        fn set_calendar_visible(self: &Backend, id: i64, visible: bool);
        /// Occurrences in `[from_ms, to_ms)` of the visible calendars.
        #[qinvokable]
        fn occurrences_json(self: &Backend, from_ms: f64, to_ms: f64) -> QString;
        #[qinvokable]
        fn detail_json(self: &Backend, id: i64, occ_ms: f64) -> QString;
        #[qinvokable]
        fn preview_json(self: &Backend, text: &QString) -> QString;
        /// JSON `{"id":..}` or `{"error":..}`.
        #[qinvokable]
        fn create_text(self: &Backend, text: &QString) -> QString;
        #[qinvokable]
        fn create_range(self: &Backend, start_ms: f64, end_ms: f64, all_day: bool) -> QString;
        #[qinvokable]
        fn move_event(self: &Backend, id: i64, occ_start_ms: f64, start_ms: f64, end_ms: f64) -> QString;
        /// Apply a JSON patch (title, location, description, calendarId, reminder, repeat, startMs, endMs, allDay).
        #[qinvokable]
        fn update_event(self: &Backend, id: i64, patch_json: &QString) -> QString;
        /// Returns the title of the deleted event, or "!error".
        #[qinvokable]
        fn delete_event(self: &Backend, id: i64) -> QString;
        #[qinvokable]
        fn duplicate_event(self: &Backend, id: i64) -> QString;
        #[qinvokable]
        fn set_rsvp(self: &Backend, id: i64, answer: &QString) -> QString;
        /// Undo the last change; returns what was undone or "".
        #[qinvokable]
        fn undo(self: &Backend) -> QString;
        #[qinvokable]
        fn accounts_json(self: &Backend) -> QString;
        /// Verify an account in the background; answered by `account_tested`.
        #[qinvokable]
        fn test_account(self: &Backend, kind: &QString, url: &QString, username: &QString, password: &QString);
        #[qinvokable]
        fn add_account(self: &Backend, kind: &QString, name: &QString, url: &QString, username: &QString, password: &QString, color: &QString) -> QString;
        #[qinvokable]
        fn remove_account(self: &Backend, key: &QString) -> QString;
        /// Start a sync pass in the background; `synced` fires when it is done.
        #[qinvokable]
        fn sync_now(self: &Backend);
        /// Show desktop notifications for reminders that are due; returns them as JSON.
        #[qinvokable]
        fn check_reminders(self: &Backend) -> QString;
        /// Absolute path of a file in the data dir (used by the autotest).
        #[qinvokable]
        fn open_url(self: &Backend, url: &QString);

        #[qsignal]
        fn synced(self: Pin<&mut Backend>, summary: QString);
        #[qsignal]
        fn account_tested(self: Pin<&mut Backend>, ok: bool, message: QString);
    }

    impl cxx_qt::Threading for Backend {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use kronoa_core::config::Paths;
use kronoa_core::engine::Core;
use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub use qobject::kronoa_new_application;

static CORE: OnceLock<Mutex<Core>> = OnceLock::new();

#[derive(Default)]
pub struct BackendRust {}

fn pl_from_env() -> bool {
    match std::env::var("KRONOA_LANG").unwrap_or_default().to_lowercase().as_str() {
        "pl" => true,
        "en" => false,
        _ => {
            let l = std::env::var("LC_ALL").or_else(|_| std::env::var("LC_MESSAGES")).or_else(|_| std::env::var("LANG")).unwrap_or_default();
            l.starts_with("pl")
        }
    }
}

/// Open the profile's store. Must run before the QML engine starts.
pub fn init(profile: &str, demo: bool) {
    let paths = Paths::for_profile(profile);
    let cfg = kronoa_core::config::Config::load(&paths);
    let pl = match std::env::var("KRONOA_LANG") {
        Ok(_) => pl_from_env(),
        Err(_) => match cfg.language.as_str() {
            "pl" => true,
            "en" => false,
            _ => pl_from_env(),
        },
    };
    let core = Core::open(paths, demo, pl).expect("cannot open the calendar store");
    let _ = CORE.set(Mutex::new(core));
}

/// The `language` setting (auto | en | pl).
pub fn language() -> String {
    core().config.language.clone()
}

fn core() -> MutexGuard<'static, Core> {
    CORE.get().expect("engine not initialised").lock().unwrap_or_else(|e| e.into_inner())
}

fn q(s: impl AsRef<str>) -> QString {
    QString::from(s.as_ref())
}

fn err(e: impl std::fmt::Display) -> QString {
    q(json!({ "error": e.to_string() }).to_string())
}

fn result_json<T: serde::Serialize>(r: anyhow::Result<T>) -> QString {
    match r {
        Ok(v) => q(serde_json::to_string(&v).unwrap_or_else(|_| "null".into())),
        Err(e) => err(e),
    }
}

impl qobject::Backend {
    fn app_name(&self) -> QString {
        q("Kronoa")
    }

    fn version(&self) -> QString {
        q(env!("CARGO_PKG_VERSION"))
    }

    fn lang(&self) -> QString {
        q(if core().pl { "pl" } else { "en" })
    }

    fn settings_json(&self) -> QString {
        let c = core();
        let mut v = serde_json::to_value(&c.config).unwrap_or(json!({}));
        let mut out = serde_json::Map::new();
        if let Some(o) = v.as_object_mut() {
            o.remove("accounts");
            for (k, val) in o.iter() {
                let mut camel = String::new();
                let mut up = false;
                for ch in k.chars() {
                    if ch == '_' { up = true } else if up { camel.extend(ch.to_uppercase()); up = false } else { camel.push(ch) }
                }
                out.insert(camel, val.clone());
            }
        }
        q(Value::Object(out).to_string())
    }

    fn set_setting(&self, key: &QString, value_json: &QString) -> QString {
        let (key, raw) = (key.to_string(), value_json.to_string());
        let val: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        let mut c = core();
        let mut cur = serde_json::to_value(&c.config).unwrap_or(json!({}));
        if !cur.as_object().map(|o| o.contains_key(&key)).unwrap_or(false) {
            return q(format!("unknown setting: {key}"));
        }
        cur[&key] = val;
        match serde_json::from_value(cur) {
            Ok(cfg) => {
                c.config = cfg;
                match c.config.save(&c.paths) {
                    Ok(()) => q(""),
                    Err(e) => q(e.to_string()),
                }
            }
            Err(e) => q(e.to_string()),
        }
    }

    fn calendars_json(&self) -> QString {
        result_json(core().calendars())
    }

    fn set_calendar_visible(&self, id: i64, visible: bool) {
        let _ = core().set_visible(id, visible);
    }

    fn occurrences_json(&self, from_ms: f64, to_ms: f64) -> QString {
        result_json(core().occurrences(from_ms as i64, to_ms as i64))
    }

    fn detail_json(&self, id: i64, occ_ms: f64) -> QString {
        match core().detail(id, occ_ms as i64) {
            Ok(Some(v)) => q(v.to_string()),
            Ok(None) => q("null"),
            Err(e) => err(e),
        }
    }

    fn preview_json(&self, text: &QString) -> QString {
        result_json(Ok(core().preview(&text.to_string())))
    }

    fn create_text(&self, text: &QString) -> QString {
        match core().create_from_text(&text.to_string()) {
            Ok(e) => q(json!({ "id": e.id, "start": e.start * 1000, "title": e.title }).to_string()),
            Err(e) => err(e),
        }
    }

    fn create_range(&self, start_ms: f64, end_ms: f64, all_day: bool) -> QString {
        match core().create_range(start_ms as i64, end_ms as i64, all_day) {
            Ok(e) => q(json!({ "id": e.id, "start": e.start * 1000 }).to_string()),
            Err(e) => err(e),
        }
    }

    fn move_event(&self, id: i64, occ_start_ms: f64, start_ms: f64, end_ms: f64) -> QString {
        match core().move_event(id, occ_start_ms as i64, start_ms as i64, end_ms as i64) {
            Ok(()) => q(""),
            Err(e) => q(e.to_string()),
        }
    }

    fn update_event(&self, id: i64, patch_json: &QString) -> QString {
        let p: Value = serde_json::from_str(&patch_json.to_string()).unwrap_or(Value::Null);
        let mut c = core();
        let r = (|| -> anyhow::Result<()> {
            let mut e = c.event(id)?.ok_or_else(|| anyhow::anyhow!("no such event"))?;
            if let Some(s) = p["title"].as_str() {
                e.title = s.into();
            }
            if let Some(s) = p["location"].as_str() {
                e.location = s.into();
            }
            if let Some(s) = p["description"].as_str() {
                e.description = s.into();
            }
            if let Some(n) = p["calendarId"].as_i64() {
                e.calendar_id = n;
            }
            if p.get("reminder").is_some() {
                e.reminder = p["reminder"].as_i64().filter(|m| *m >= 0).map(|m| m as i32);
            }
            if let Some(b) = p["allDay"].as_bool() {
                e.all_day = b;
            }
            if let (Some(s), Some(en)) = (p["startMs"].as_f64(), p["endMs"].as_f64()) {
                e.start = (s as i64).div_euclid(1000);
                e.end = (en as i64).div_euclid(1000);
            }
            if let Some(k) = p["repeat"].as_str() {
                let wd = kronoa_core::time::Zone::from_tzid(&e.tzid).naive(e.start).date();
                e.rrule = kronoa_core::recur::rule_for_kind(k, chrono::Datelike::weekday(&wd));
            }
            c.update(e, "edit")
        })();
        match r {
            Ok(()) => q(""),
            Err(e) => q(e.to_string()),
        }
    }

    fn delete_event(&self, id: i64) -> QString {
        match core().delete(id) {
            Ok(t) => q(t),
            Err(e) => q(format!("!{e}")),
        }
    }

    fn duplicate_event(&self, id: i64) -> QString {
        match core().duplicate(id) {
            Ok(e) => q(json!({ "id": e.id, "start": e.start * 1000 }).to_string()),
            Err(e) => err(e),
        }
    }

    fn set_rsvp(&self, id: i64, answer: &QString) -> QString {
        match core().set_rsvp(id, &answer.to_string()) {
            Ok(()) => q(""),
            Err(e) => q(e.to_string()),
        }
    }

    fn undo(&self) -> QString {
        match core().undo() {
            Ok(Some(s)) => q(s),
            _ => q(""),
        }
    }

    fn accounts_json(&self) -> QString {
        result_json(core().accounts_json())
    }

    fn test_account(&self, kind: &QString, url: &QString, username: &QString, password: &QString) {
        let (k, u, n, p) = (kind.to_string(), url.to_string(), username.to_string(), password.to_string());
        // the signal needs a handle on the object: see `Backend::qt_thread` in the QML-facing wrapper
        let thread = self.qt_thread();
        std::thread::spawn(move || {
            let r = kronoa_core::sync::test_account(&k, &u, &n, &p);
            let (ok, msg) = match r {
                Ok(n) => (true, n.to_string()),
                Err(e) => (false, e.to_string()),
            };
            let _ = thread.queue(move |mut obj: Pin<&mut qobject::Backend>| obj.as_mut().account_tested(ok, QString::from(&msg)));
        });
    }

    fn add_account(&self, kind: &QString, name: &QString, url: &QString, username: &QString, password: &QString, color: &QString) -> QString {
        match core().add_account(&kind.to_string(), &name.to_string(), &url.to_string(), &username.to_string(), &password.to_string(), &color.to_string()) {
            Ok(id) => q(json!({ "id": id }).to_string()),
            Err(e) => err(e),
        }
    }

    fn remove_account(&self, key: &QString) -> QString {
        match core().remove_account(&key.to_string()) {
            Ok(()) => q(""),
            Err(e) => q(e.to_string()),
        }
    }

    fn sync_now(&self) {
        if std::env::var_os("KRONOA_OFFLINE").is_some() {
            return;
        }
        let (paths, cfg) = {
            let c = core();
            (c.paths.clone(), c.config.clone())
        };
        if cfg.accounts.is_empty() {
            return;
        }
        let thread = self.qt_thread();
        std::thread::spawn(move || {
            let res = kronoa_core::sync::sync_all(&paths, &cfg);
            let summary: Vec<Value> = res.into_iter().map(|(k, r)| json!({ "account": k, "ok": r.is_ok(), "message": r.unwrap_or_else(|e| e) })).collect();
            let s = Value::Array(summary).to_string();
            let _ = thread.queue(move |mut obj: Pin<&mut qobject::Backend>| obj.as_mut().synced(QString::from(&s)));
        });
    }

    fn check_reminders(&self) -> QString {
        let (due, enabled) = {
            let c = core();
            (c.due_reminders(kronoa_core::time::now()).unwrap_or_default(), c.config.notifications)
        };
        if enabled && !std::env::args().any(|a| a.starts_with("--autotest")) {
            for r in &due {
                let when = chrono::DateTime::from_timestamp(r.start_ms / 1000, 0)
                    .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
                    .unwrap_or_default();
                let body = if r.location.is_empty() { when } else { format!("{when} · {}", r.location) };
                let title = r.title.clone();
                std::thread::spawn(move || {
                    let _ = notify_rust::Notification::new().appname("Kronoa").icon("kronoa").summary(&title).body(&body).show();
                });
            }
        }
        result_json(Ok(due))
    }

    fn open_url(&self, url: &QString) {
        let u = url.to_string();
        if u.starts_with("http://") || u.starts_with("https://") {
            let _ = std::process::Command::new("xdg-open").arg(u).spawn();
        }
    }
}
