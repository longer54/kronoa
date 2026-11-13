# Kronoa

Szybki, natywny kalendarz na pulpit: silnik w Rust (`kronoa-core`), interfejs Qt 6 / QML (cxx-qt).
Kalendarze lokalne, konta CalDAV, subskrypcje ICS, szybkie dodawanie po polsku i angielsku
(„spotkanie jutro 14:00 1h co tydzień”), przypomnienia z powiadomieniami i ikona w zasobniku.

## Budowa i instalacja (dla użytkownika, bez sudo)
    cargo build --release -p kronoa
    scripts/install-user.sh          # program, ikony i .desktop do ~/.local

Wymaga Qt 6 i Rusta. Skróty: Ctrl+K dodawanie, T dziś, D/W/M widoki, Del usuń, Ctrl+Z cofnij.
Test ze zrzutami: `QT_QPA_PLATFORM=offscreen kronoa --autotest=KATALOG`.

Licencja: MIT lub Apache-2.0.
