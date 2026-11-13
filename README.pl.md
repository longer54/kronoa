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

## Google Calendar
Ustawienia → Konta → Dodaj konto → **Zaloguj przez Google**. Każdy kalendarz Google jest osobną pozycją
(z kolorem i checkboxem); wydarzenia są pobierane przyrostowo (syncToken) i tworzone / edytowane / usuwane
z Kronoi, także całodniowe, cykliczne i ze strefami czasowymi.
Google wymaga własnego klienta OAuth — załóż go (za darmo, ok. 5 minut):

1. <https://console.cloud.google.com> → nowy projekt → *APIs & Services* → *Library* → włącz **Google Calendar API**.
2. *OAuth consent screen*: typ External, dodaj siebie w *Test users* (albo opublikuj aplikację).
3. *Credentials* → *Create credentials* → *OAuth client ID* → typ **Desktop app**.
4. Wpisz client id i secret do `~/.config/kronoa/config.toml`:

        [google]
        client_id = "123-abc.apps.googleusercontent.com"
        client_secret = "GOCSPX-..."

   (albo zmienne `KRONOA_GOOGLE_CLIENT_ID` / `KRONOA_GOOGLE_CLIENT_SECRET`). Nigdy ich nie commituj.
5. Kliknij przycisk; otworzy się przeglądarka — zaloguj się i zezwól na dostęp. Kronoa nasłuchuje jednorazowo na
   `http://127.0.0.1:<losowy port>` (PKCE), trzyma refresh token w keyringu i sama odświeża dostęp.
   *Odłącz* unieważnia token. Aplikacje w trybie „Testing” dostają tokeny wygasające po 7 dniach — opublikuj aplikację, by tego uniknąć.
