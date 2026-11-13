# Kronoa

A fast, native desktop calendar: Rust engine (`kronoa-core`), Qt 6 / QML interface (cxx-qt).
Local calendars, CalDAV accounts, ICS subscriptions, quick-add in English and Polish
("meeting tomorrow 2pm 1h weekly"), reminders with desktop notifications and a tray icon.

## Build & install (per user, no sudo)
    cargo build --release -p kronoa
    scripts/install-user.sh          # binary, icons, .desktop into ~/.local

Requires Qt 6 (Quick, QuickControls2, Widgets) and a Rust toolchain.
Keys: Ctrl+K quick add, T today, D/W/M views, Del delete, Ctrl+Z undo.
Config: see `config.example.toml`. Screenshot test: `QT_QPA_PLATFORM=offscreen kronoa --autotest=DIR`.

## Google Calendar
Settings → Accounts → Add account → **Sign in with Google**. Every Google calendar appears as its own
calendar (with its colour and a checkbox); events are read incrementally (sync tokens) and
created / edited / deleted from Kronoa, including all-day events, recurring series and time zones.
Google requires each app to use an OAuth client, so create your own (free, ~5 minutes):

1. <https://console.cloud.google.com> → create a project → *APIs & Services* → *Library* → enable **Google Calendar API**.
2. *OAuth consent screen*: user type External, add yourself under *Test users* (or publish the app).
3. *Credentials* → *Create credentials* → *OAuth client ID* → application type **Desktop app**.
4. Put the client id and secret into `~/.config/kronoa/config.toml`:

        [google]
        client_id = "123-abc.apps.googleusercontent.com"
        client_secret = "GOCSPX-..."

   (or set `KRONOA_GOOGLE_CLIENT_ID` / `KRONOA_GOOGLE_CLIENT_SECRET`). Never commit these.
5. Click the button; the browser opens, sign in and allow access. Kronoa listens once on
   `http://127.0.0.1:<random port>` (PKCE), stores the refresh token in the keyring and refreshes access automatically.
   *Disconnect* revokes it. Apps in "Testing" mode get refresh tokens that expire after 7 days - publish the app to avoid that.

License: MIT OR Apache-2.0.
