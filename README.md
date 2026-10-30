<p align="center"><img src="docs/logo.png" alt="GG-Retro"></p>

# GG-Retro — fork webs1821

Serwer dla oryginalnego Gadu-Gadu 6.0/6.1, oparty na [projekcie macbury](https://github.com/macbury/gg-retro). Ten fork poprawia kolejkę offline, zgodność protokołu i wdrożenie publiczne. Licencja autora: [LICENSE](LICENSE).

## Korzystanie z serwera

1. Otwórz **https://gadu-gadu.k13.pl** i wybierz „Załóż konto”. Kod zaproszenia otrzymasz od właściciela serwera.
2. Opcjonalnie wpisz własny wolny numer (1–16777215). Puste pole przydziela kolejne wolne numery od 112233; niższe są dostępne wyłącznie przez ręczny wybór. Zapisz przydzielony numer. Użyj osobnego hasła: 8–15 znaków ASCII, bez spacji.
3. Zainstaluj klienta GG 6.1 z zakładki „Pobierz”.
4. Patcherem na stronie zmodyfikuj **gg.exe z katalogu instalacji**, nie instalator `gg61.exe`. Domena `gadu-gadu.k13.pl` mieści się w polach klienta (16 znaków). Patcher działa lokalnie w przeglądarce i nie wysyła pliku na serwer.
5. Zapisz zmodyfikowaną kopię w katalogu klienta, uruchom ją i wybierz logowanie na istniejący numer. Wpisz numer i hasło otrzymane na tej stronie.

Numery z oficjalnej sieci GG nie są automatycznie kontami na tym serwerze. Rejestracja wewnątrz starego klienta, katalog publiczny, odzyskiwanie hasła e-mailem i DCC/obrazki nie są obsługiwane.

**GG6 przesyła rozmowy bez szyfrowania i używa starego mechanizmu uwierzytelniania.** HTTPS chroni rejestrację, ale nie połączenie komunikatora. To usługa do rozmów retro, nie do przesyłania poufnych danych ani ponownego używania ważnych haseł.

## Wiadomości i obecność

- Każda zaakceptowana wiadomość jest najpierw zapisywana w SQLite (WAL, `synchronous=FULL`). Potwierdzenie `Queued` oznacza trwałe przyjęcie przez serwer, nie przeczytanie przez odbiorcę; stosujemy je również dla aktywnych odbiorców.
- Wiadomości offline przetrwają restart procesu. Po zalogowaniu trafiają do odbiorcy w kolejności zapisu, po odebraniu jego listy kontaktów i blokad. Zapisany czas, polskie znaki, klasa i formatowanie są zachowane.
- Dostarczenie oznaczane jest po każdym udanym zapisie do TCP, osobno dla każdej wiadomości. GG6 nie daje potwierdzenia odbioru przez aplikację: awaria między zapisem TCP a zapisem do bazy może spowodować powtórzenie, a zerwane połączenie po przyjęciu danych przez stos TCP nie dowodzi wyświetlenia wiadomości. Nie obiecujemy exactly-once ani potwierdzeń przeczytania.
- Limit to 1000 oczekujących wiadomości na odbiorcę i 50 000 wszystkich rekordów kolejki. Pełna skrzynka zwraca `MboxFull`, a nie sukces. Oczekujące wiadomości nie wygasają automatycznie. Dostarczone rekordy są usuwane po godzinie.
- Ponowne logowanie zastępuje poprzednią sesję. Dostarczanie i sprzątanie sesji danego konta są serializowane.
- Działają dodawanie/usuwanie kontaktów, import/eksport listy na serwer, blokowanie nadawcy, niewidoczność i tryb „tylko dla znajomych”. Listy eksportowane są prywatne dla konta; limit 256 KiB.

## Budowanie i testy

Testowana wersja Rust: **1.99.1**. Serwer buduje się jako `gg-retro`:

```sh
cargo build --locked --release -p gg-server
cargo test --locked --workspace
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/wire_integration.py
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/registration_integration.py
node tests/patcher.test.cjs
cargo install cargo-audit --version 0.22.2 --locked
sh tests/check-audit-graph.sh
```

Testy integracyjne tworzą wyłącznie tymczasową bazę i fikcyjne konta. Obejmują m.in. 205 wiadomości po nagłym zatrzymaniu procesu, kolejność, blokady przed synchronizacją, przejęcie sesji, prywatność statusów, import listy i granicę pojemności skrzynki. Testy kodeka sprawdzają też skrócone i nieznane ramki w obu kierunkach.

`Cargo.lock` zawiera opcjonalne RSA/MySQL z SQLx, mimo że nie są włączone ani kompilowane. Wyjątek `RUSTSEC-2023-0071` w `.cargo/audit.toml` jest połączony z testem ich nieobecności w grafie budowania. Włączenie MySQL wymaga ponownego audytu. Pozostają informacyjne ostrzeżenia o nieutrzymywanych bibliotekach `basic-toml`, `paste`, `ttf-parser`; parser fontu dostaje wyłącznie osadzony font CAPTCHA, nie pliki od klientów.

## Wdrożenie

Przykłady znajdują się w [deploy](deploy). Nie zawierają haseł ani kluczy. `advertised_ip` musi być publicznym IPv4, pod którym router przekierowuje TCP 8074. `hostname` służy stronie i patcherowi; `public_scheme` powinno być `https`.

1. Zbuduj binarkę Linux amd64 i przygotuj katalog budowania z plikiem `gg-retro` i `deploy/Dockerfile.runtime`.
2. Zbuduj obraz: `docker build -f Dockerfile.runtime -t gg-retro:0.4.1 .`.
3. W `/opt/gg-retro` umieść `compose.yaml`, konfigurację z `config.example.toml` oraz katalogi `data` i `secrets`.
4. Wygeneruj kryptograficznie losowe **32 surowe bajty** do `secrets/password.key` i pulę kodów: `python3 deploy/generate_invites.py --count 100 --output secrets/registration-invite.txt`. Nie zapisuj ich w Git ani w logach. Katalog `secrets` powinien być dostępny tylko administratorowi, a montowane pliki czytelne dla UID 10001; `data` musi należeć do UID/GID 10001.
5. W `.env` ustaw `GG_BIND_IP` na IP hosta Containers i opcjonalnie `GG_RETRO_IMAGE`. Domyślne wiązanie portów to bezpieczny loopback.
6. Zastosuj `egress-guard.sh` jako root i uruchamiaj go po starcie Dockera (OpenRC/systemd). Reguły dotyczą wyłącznie mostu `br-gg-retro`; zachowują odpowiedzi na połączenia przychodzące.
7. Uruchom `docker compose up -d`. Sprawdź `/healthz` i stan kontenera.
8. Skonfiguruj nginx według przykładu, z prawidłowym certyfikatem. Port HTTP backendu 18074 powinien być dostępny wyłącznie z routera. Publiczne TCP 8074 przekieruj na host Containers i dopuść w zaporze VM; nie kieruj go do serwera WWW. Dodaj odpowiedni hairpin NAT dla LAN/VPN.
9. Skonfiguruj codzienne uruchomienie `backup.py` przez root. Tworzy spójne kopie SQLite i kopiuje niezbędny klucz oraz konfigurację; zachowuje 14 ostatnich kopii. Kopie są prywatne i nie są montowane w kontenerze aplikacji.

Kontener nie działa jako root, ma system plików tylko do odczytu, usunięte capabilities, limit 256 MiB i 0,5 CPU. Aplikacja ogranicza ramki, połączenia, tempo pakietów i rozmiar kolejki. nginx ogranicza rejestrację i zapisuje ścieżki bez parametrów zapytań. Sam nginx/Fail2ban dla HTTP nie chroni połączeń przekierowanych przez DNAT — ograniczenia GG działają również w aplikacji.

### Hasła i istniejąca baza

Wersja upstream przechowywała hasła jawnie. GG6 wymaga odzyskania hasła do obliczenia odpowiedzi na losowe wyzwanie, dlatego fork stosuje XChaCha20-Poly1305 z osobnym kluczem i powiązaniem szyfrogramu z numerem konta. Utrata klucza uniemożliwia logowanie; kopia samej bazy nie wystarcza. Wiadomości oczekujące i kontakty są przechowywane w bazie i wymagają ochrony uprawnień plików/kopii.

Przed migracją istniejącej bazy wykonaj jej pełną, spójną kopię. Uruchomienie z `migrate_plaintext_passwords = true` jednorazowo szyfruje stare hasła w transakcji, wykonuje VACUUM i czyści WAL. Potem wyłącz tę opcję. Bez niej serwer odmawia uruchomienia na jawnych hasłach. Nie można cofnąć się do upstream na zmigrowanej bazie bez przywrócenia kopii sprzed migracji.

Plik zaproszeń zawiera po jednym kodzie w każdym wierszu. Każdy kod pozwala na **3 udane rejestracje**. Baza przechowuje wyłącznie skróty SHA-256 kodów oraz trwałe liczniki użyć. Rejestracja konta, zużycie kodu/CAPTCHA i przydzielenie numeru są jedną transakcją. Błędna próba (także zajęty numer lub e-mail) nie zużywa zaproszenia ani numeru automatycznego. Równoczesne rejestracje nie przekraczają limitu.

Po zmianie pliku odtwórz kontener (`docker compose up -d --force-recreate gg-retro`), aby odczytał aktualny plik. Usunięty kod zostaje wyłączony. Przywrócenie kodu i restart nie zerują licznika, a usunięcie konta nie zwraca użycia. Pusty plik zamyka rejestrację bez blokowania logowania istniejących kont. Liczniki kodów i kolejny numer są częścią kopii SQLite; odtworzenie starszej kopii cofa także te liczniki, dlatego przy odtwarzaniu po utracie nowszych rejestracji należy unieważnić dawne kody i wydać nową pulę.

Ręcznie wybrany wysoki numer nie przesuwa numeracji automatycznej. Zajęte numery są pomijane, a istniejące konta zachowują numery po migracji. Nowa numeracja ma własny trwały licznik od 112233, niezależny od dawnego losowania i SQLite AUTOINCREMENT. Limit 16777215 wynika z 24-bitowego pola GG6.

Zmiana puli zaproszeń nie zmienia haseł istniejących kont. Klucza do haseł nie wolno podmieniać bez procedury migracji szyfrogramów.
