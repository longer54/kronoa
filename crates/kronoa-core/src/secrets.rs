//! Passwords: the system keyring (Secret Service on Linux), with an explicit fallback.
//!
//! Order: 1) keyring entry `kronoa` / `<profile>/<account>`; 2) only when the user allowed it
//! (`allow_plaintext_secrets = true`), a `secrets.toml` file with mode 0600 in the data dir;
//! 3) otherwise the password is kept in memory for this session only.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::config::Paths;

const SERVICE: &str = "kronoa";

static SESSION: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Where a password ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Keyring,
    File,
    Session,
}

fn user(paths: &Paths, account: &str) -> String {
    format!("{}/{}", paths.profile, account)
}

pub fn secret_ref(paths: &Paths, account: &str) -> String {
    format!("keyring:{SERVICE}/{}", user(paths, account))
}

fn read_file(paths: &Paths) -> HashMap<String, String> {
    std::fs::read_to_string(paths.secrets_file())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_file(paths: &Paths, map: &HashMap<String, String>) -> anyhow::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(&paths.data_dir)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(paths.secrets_file())?;
    f.write_all(toml::to_string(map)?.as_bytes())?;
    Ok(())
}

fn keyring_disabled() -> bool {
    std::env::var_os("KRONOA_NO_KEYRING").is_some()
}

pub fn store(paths: &Paths, account: &str, password: &str, allow_file: bool) -> Stored {
    let key = user(paths, account);
    if !keyring_disabled() {
        if let Ok(e) = keyring::Entry::new(SERVICE, &key) {
            if e.set_password(password).is_ok() && e.get_password().ok().as_deref() == Some(password) {
                return Stored::Keyring;
            }
        }
    }
    if allow_file {
        let mut m = read_file(paths);
        m.insert(key.clone(), password.to_string());
        if write_file(paths, &m).is_ok() {
            return Stored::File;
        }
    }
    SESSION.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, password.to_string());
    Stored::Session
}

pub fn load(paths: &Paths, account: &str) -> Option<String> {
    let key = user(paths, account);
    if let Some(p) = SESSION.lock().unwrap().as_ref().and_then(|m| m.get(&key).cloned()) {
        return Some(p);
    }
    if !keyring_disabled() {
        if let Ok(p) = keyring::Entry::new(SERVICE, &key).and_then(|e| e.get_password()) {
            return Some(p);
        }
    }
    read_file(paths).get(&key).cloned()
}

pub fn delete(paths: &Paths, account: &str) {
    let key = user(paths, account);
    if let Some(m) = SESSION.lock().unwrap().as_mut() {
        m.remove(&key);
    }
    if !keyring_disabled() {
        if let Ok(e) = keyring::Entry::new(SERVICE, &key) {
            let _ = e.delete_credential();
        }
    }
    let mut m = read_file(paths);
    if m.remove(&key).is_some() {
        let _ = write_file(paths, &m);
    }
}
