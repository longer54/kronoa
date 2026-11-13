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

License: MIT OR Apache-2.0.
