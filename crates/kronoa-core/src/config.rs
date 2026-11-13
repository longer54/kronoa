//! Paths and the `config.toml` file.
//!
//! Default profile: `~/.config/kronoa/config.toml` and `~/.local/share/kronoa/kronoa.db`.
//! `--profile NAME`: `~/.config/kronoa/profiles/NAME/…` and `~/.local/share/kronoa/profiles/NAME/…`.
//! The XDG variables are respected (`XDG_CONFIG_HOME`, `XDG_DATA_HOME`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// auto | en | pl
    pub language: String,
    /// system | light | dark | navy
    pub theme: String,
    /// monday | sunday | saturday
    pub week_start: String,
    /// 24h | 12h | system
    pub time_format: String,
    /// day | week | month
    pub default_view: String,
    /// First hour shown when the week view opens.
    pub start_hour: u32,
    pub show_weekends: bool,
    /// Minutes before; negative = no default reminder.
    pub default_reminder: i32,
    pub notifications: bool,
    /// Minutes between background syncs.
    pub sync_interval: u32,
    /// Store passwords in a 0600 file when no Secret Service keyring is available.
    pub allow_plaintext_secrets: bool,
    /// Calendar for new events: "<account id>/<calendar name>" or "" (the first writable one).
    pub default_calendar: String,
    #[serde(rename = "account")]
    pub accounts: Vec<Account>,
    /// Your own Google OAuth client ("Desktop app"); see the README.
    #[serde(skip_serializing_if = "GoogleConfig::is_empty")]
    pub google: GoogleConfig,
}

/// The OAuth client used for "Sign in with Google". Never committed; the refresh token is in the keyring.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
}

impl GoogleConfig {
    pub fn is_empty(&self) -> bool {
        self.client_id.is_empty() && self.client_secret.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Account {
    /// Stable short id, e.g. "home".
    pub id: String,
    /// caldav | ics | google
    pub kind: String,
    pub name: String,
    pub url: String,
    pub username: String,
    /// `keyring:kronoa/<profile>/<id>` (never the secret itself).
    pub secret_ref: String,
    /// For ICS subscriptions: the colour of the calendar.
    pub color: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            language: "auto".into(),
            theme: "system".into(),
            week_start: "monday".into(),
            time_format: "24h".into(),
            default_view: "week".into(),
            start_hour: 7,
            show_weekends: true,
            default_reminder: 10,
            notifications: true,
            sync_interval: 15,
            allow_plaintext_secrets: false,
            default_calendar: String::new(),
            accounts: vec![],
            google: GoogleConfig::default(),
        }
    }
}

/// Where this profile keeps its files.
#[derive(Clone, Debug)]
pub struct Paths {
    pub profile: String,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn for_profile(profile: &str) -> Paths {
        let profile = sanitize(profile);
        let cfg = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config")).join("kronoa");
        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".local/share")).join("kronoa");
        if profile.is_empty() || profile == "default" {
            Paths { profile: "default".into(), config_dir: cfg, data_dir: data }
        } else {
            Paths {
                config_dir: cfg.join("profiles").join(&profile),
                data_dir: data.join("profiles").join(&profile),
                profile,
            }
        }
    }
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("kronoa.db")
    }
    pub fn secrets_file(&self) -> PathBuf {
        self.data_dir.join("secrets.toml")
    }
}

/// Profile names are path components: keep letters, digits, `-` and `_`.
pub fn sanitize(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect()
}

impl Config {
    pub fn load(paths: &Paths) -> Config {
        std::fs::read_to_string(paths.config_file())
            .ok()
            .and_then(|s| match toml::from_str::<Config>(&s) {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("kronoa: {} is not valid ({e}); using defaults", paths.config_file().display());
                    None
                }
            })
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        std::fs::create_dir_all(&paths.config_dir)?;
        let text = format!("# Kronoa settings (written by the app; safe to edit while it is closed)\n{}", toml::to_string_pretty(self)?);
        let tmp = paths.config_file().with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, paths.config_file())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../config.example.toml");
        let c: Config = toml::from_str(text).expect("config.example.toml");
        assert_eq!(c.language, "auto");
        assert!(c.accounts.iter().all(|a| a.url.contains("example")));
    }

    #[test]
    fn profile_names_are_sanitized() {
        assert_eq!(sanitize("../evil name"), "evilname");
        let p = Paths::for_profile("work");
        assert!(p.db_file().ends_with("profiles/work/kronoa.db"));
    }
}
