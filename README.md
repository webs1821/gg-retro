<p align="center"><img src="docs/logo.png" alt="GG-Retro"></p>

# GG-Retro — fork webs1821

Serwer dla oryginalnego Gadu-Gadu 6.0/6.1, oparty na [projekcie macbury](https://github.com/macbury/gg-retro). Ten fork poprawia kolejkę offline, zgodność protokołu i wdrożenie publiczne. Licencja autora: [LICENSE](LICENSE).

## Co dodano i poprawiono w tym forku

Względem pierwotnego projektu macbury:

- **Opisy offline (0.7.0):** opis „niedostępny” i opcjonalny czas powrotu są zachowywane po zamknięciu klienta oraz restarcie serwera, razem z blokadami i ustawieniami widoczności.
- **Ponowne łączenie (0.7.0):** restart zamyka TCP bez pakietu wymuszonego wylogowania. Zmiana na „niedostępny” kończy sesję, a zalogowanie drugiego klienta nadal rozłącza poprzedni. Obecność jest publikowana dopiero po otrzymaniu listy kontaktów i prywatności.
- **Naprawione doręczanie offline:** trwała kolejka, kolejność, większe partie po zalogowaniu, poprawne flagi i formatowanie, brak ponownego wysyłania przy zwykłym relogowaniu; zapis przed potwierdzeniem.
- **Konferencje GG6 (0.6.0):** zachowanie listy uczestników w rozmowach grupowych, również w kolejce offline, przy formatowaniu tekstu i obrazkach. Odpowiedzi zachowują kontekst grupy, a serwer nie powiela wysyłki wykonywanej przez klienta.
- **Obrazki (0.6.0):** klasyczny tryb GG6 w rozmowach prywatnych i konferencjach, transfer na żądanie od zalogowanego nadawcy, części binarne, kontrola rozmiaru i CRC32. Serwer nie przechowuje trwałych kopii obrazków.
- **Kontakty i prywatność:** trwały import/eksport listy, dodawanie/usuwanie obserwacji, blokady przed dostarczeniem pierwszej wiadomości, poprawiona niewidoczność i tryb „tylko znajomi”.
- **Sesje i protokół:** bezpieczne zastępowanie starego połączenia, poprawione bitmaski, obsługa rozszerzeń i odrzucanie niepełnych/przerośniętych ramek.
- **Nowa rejestracja WWW:** wybór wolnego numeru 1–16777215 albo kolejne numery od 112233; zaproszenia po 3 udane rejestracje, trwałe liczniki i atomowa obsługa równoczesnych prób.
- **Ochrona danych i usługi:** szyfrowanie haseł XChaCha20-Poly1305 z osobnym kluczem, jawna migracja starych haseł, limity połączeń/pakietów/kolejki i brak sekretów w logach.
- **Gotowy klient i instalator:** generowane dla domeny z konfiguracji; stary kreator NSIS 2 zachowuje oryginalny skrypt, zasoby i pliki poza podmienionym gg.exe. Patcher poprawnie zachowuje końcówki URL i rozmiar EXE.
- **Przenośne wdrożenie:** domena w TOML, generator nginx, opcjonalny katalog plików do pobrania z kontrolą domeny i SHA-256, ograniczony kontener, blokada połączeń wychodzących, spójne kopie SQLite i healthcheck.
- **Testy i CI:** testy kodeka, sesji, rejestracji i wyścigów, prawdziwe TCP/HTTP, restart/awaria, audyt zależności, weryfikacja paczki klienta i test publicznego dostępu.

## Korzystanie z serwera

1. Otwórz stronę swojego serwera i wybierz „Załóż konto”. Kod zaproszenia otrzymasz od administratora.
2. Opcjonalnie wpisz dawny, wolny numer (1–16777215). Puste pole przydziela kolejny wolny numer od 112233; niższe numery są dostępne tylko ręcznie. Użyj osobnego hasła: 8–15 znaków ASCII, bez spacji.
3. W „Pobierz” wybierz gotowy instalator, jeśli administrator go udostępnił. W nowej instalacji nie trzeba potem patchować programu.
4. Jeśli masz już GG 6.1, zamknij je i pobierz gotowy `gg.exe`. Otwórz folder instalacji przez właściwości skrótu, zachowaj poprzedni plik jako `gg.exe.bak`, a pobrany zapisz **w folderze instalacji, obok DLL, emots i skins**. Uruchom go stamtąd.
5. Alternatywnie zainstaluj oryginalne GG 6.1 i użyj „Patchera” na **gg.exe z folderu instalacji**, nie na instalatorze. Domena musi mieć najwyżej 16 znaków ASCII. Patcher działa lokalnie w przeglądarce.
6. W komunikatorze wybierz istniejący numer i wpisz numer oraz hasło zarejestrowane na tym serwerze.

Numery z oficjalnej sieci GG nie są automatycznie kontami na tym serwerze. Rejestracja wewnątrz starego klienta, katalog publiczny, odzyskiwanie hasła e-mailem i DCC nie są obsługiwane.

**GG6 przesyła rozmowy bez szyfrowania i używa starego mechanizmu uwierzytelniania.** HTTPS chroni rejestrację, ale nie rozmowy. Używaj osobnego hasła i nie przesyłaj poufnych danych.

## Domena i gotowe pliki klienta

Domena jest ustawiana przez `hostname` w konfiguracji TOML (lub `GG_HOSTNAME`). Kod serwera, testy patchera i szablon nginx nie zawierają adresu operatora. [deploy/config.example.toml](deploy/config.example.toml) zawiera neutralny przykład; [deploy/site.toml](deploy/site.toml) zawiera tylko publiczną domenę bieżącego wdrożenia webs1821, używaną do budowania klienta i testu publicznego. Osoba stawiająca własny serwer powinna przygotować własny, prywatny `config.toml` na podstawie neutralnego przykładu i ustawić w nim `hostname`, `advertised_ip`, certyfikat i adres backendu nginx.

```sh
# Python 3.11+ i Node.js; wszystkie pliki wynikowe poza Git.
python3 tools/build_client.py --config deploy/site.toml --output dist/client
python3 deploy/render_nginx.py --config config.toml --output dist/nginx.conf
```

Generator klienta korzysta z oryginalnego instalatora GG 6.1 o przypiętym SHA-256. Weryfikuje CRC NSIS, wyciąga oryginalny gg.exe, używa tego samego patchera co przeglądarka i zastępuje wyłącznie ten plik w strumieniu instalatora. Odtwarza długość strumienia i CRC, po czym ponownie sprawdza, że pozostała zawartość jest identyczna. Nie uruchamia żadnego EXE. Opcjonalne `--patched-exe plik.exe` pozwala użyć dostarczonego klienta, ale tylko gdy jest identyczny z oczekiwaną modyfikacją dla podanej domeny.

Skopiuj wynik (`gg.exe`, `gg-setup.exe`, `manifest.toml`) do `/opt/gg-retro/downloads` i ustaw `client_downloads_dir = "/app/downloads"`. Compose montuje ten katalog tylko do odczytu. Pliki nie są osadzane w binarce serwera; zmiana domeny wymaga wygenerowania nowej paczki i restartu. Niezgodna domena lub suma kontrolna powoduje odmowę startu zamiast udostępnienia błędnego klienta. Pusta wartość wyłącza gotowe pliki i zostawia oryginalny instalator oraz patcher.

Gotowy instalator zachowuje zachowanie i kreator oryginalnego GG 6.1; nie jest nowym instalatorem systemowym ani aktualizacją bibliotek klienta. Automatyczne testy sprawdzają CRC, zawartość i serwowanie paczki, a nie przebieg interaktywnej instalacji na wszystkich wersjach Windows. Prawa do klienta GG i jego zasobów pozostają przy pierwotnych autorach; licencja serwera nie zastępuje licencji klienta.

## Wiadomości i obecność

- Każda zaakceptowana wiadomość jest najpierw zapisywana w SQLite (WAL, `synchronous=FULL`). Potwierdzenie `Queued` oznacza trwałe przyjęcie przez serwer, nie przeczytanie przez odbiorcę; stosujemy je również dla aktywnych odbiorców.
- Wiadomości offline przetrwają restart procesu. Po zalogowaniu trafiają do odbiorcy w kolejności zapisu, po odebraniu jego listy kontaktów i blokad. Zapisany czas, polskie znaki, klasa i formatowanie są zachowane.
- Dostarczenie oznaczane jest po każdym udanym zapisie do TCP, osobno dla każdej wiadomości. GG6 nie daje potwierdzenia odbioru przez aplikację: awaria między zapisem TCP a zapisem do bazy może spowodować powtórzenie, a zerwane połączenie po przyjęciu danych przez stos TCP nie dowodzi wyświetlenia wiadomości. Nie obiecujemy exactly-once ani potwierdzeń przeczytania.
- Limit to 1000 oczekujących wiadomości na odbiorcę i 50 000 wszystkich rekordów kolejki. Pełna skrzynka zwraca `MboxFull`, a nie sukces. Oczekujące wiadomości nie wygasają automatycznie. Dostarczone rekordy są usuwane po godzinie.
- Ponowne logowanie zastępuje poprzednią sesję. Dostarczanie i sprzątanie sesji danego konta są serializowane.
- Działają dodawanie/usuwanie kontaktów, import/eksport listy na serwer, blokowanie nadawcy, niewidoczność i tryb „tylko dla znajomych”. Listy eksportowane są prywatne dla konta; limit 256 KiB.

## Opisy offline i ponowne łączenie

W GG6 wybierz **Niedostępny z opisem**, np. „Wrócę wieczorem”. Serwer zapisze opis (do 70 znaków CP1250) oraz czas powrotu, jeśli klient go wysyła. Opis pozostaje widoczny także dla później logujących się kontaktów i przetrwa restart serwera. W tej samej operacji zapisywane są reguły prywatności: blokady, kontakty „zawsze niedostępny” i tryb „tylko znajomi”. Dane nie są wystawiane przez stronę WWW.

Zwykłe „Niedostępny”, pusty opis albo kolejne poprawne logowanie usuwają poprzedni opis offline. Nie zapisujemy automatycznie opisów dostępności ani niewidoczności po utracie połączenia. Do publikacji opisu wymagamy pełnej listy kontaktów; samo uwierzytelnienie nie ujawnia jeszcze opisu obserwatorom. Opis i reguły są częścią kopii bazy.

Restart (SIGTERM/SIGINT) kończy TCP w zwykły sposób. Pakiet `GG_DISCONNECTING` oznaczający wymuszone wylogowanie jest używany przy zastąpieniu sesji przez inny klient, a nie podczas prac serwisowych. To rozróżnienie pozwala klientowi ponawiać połączenie; jego harmonogram prób zależy od wersji i ustawień GG. Zmiana na niedostępny również zamyka sesję. Dla GG6 nie wysyłamy potwierdzenia rozłączenia z późniejszych wersji protokołu.

Testy używają prawdziwego TCP i sprawdzają pakiety przy restarcie, ponowną autoryzację, kolejkę offline oraz prywatność przed i po restarcie. Nie zastępują ręcznego sprawdzenia automatycznego łączenia w konkretnym oryginalnym kliencie GG. Opis rozłączeń: [protokół libgadu, sekcja 1.11](https://libgadu.github.io/protocol/#ch1.11).

## Obrazki w klasycznym GG

Obsługa obrazków korzysta z opisu [protokołu libgadu, sekcje 1.6.2.2 i 1.6.3](https://libgadu.github.io/protocol/). Wiadomość niesie położenie obrazka, rozmiar i CRC32; osobne pakiety z pustym tekstem przenoszą żądanie i dane binarne. Implementacja jest własna, napisana na podstawie specyfikacji. Nie wymaga DCC ani dodatkowych portów.

**Pierwsze pobranie wymaga zalogowanego nadawcy, którego klient nadal ma obrazek.** Odbiorca wysyła żądanie, a serwer przekazuje je nadawcy. Dane są składane i sprawdzane w pamięci, przesyłane wyłącznie do sesji, która ich zażądała, po czym bufor jest zwalniany. Serwer nie pobiera obrazków z wyprzedzeniem, nie przechowuje ich w SQLite ani w plikach i nie ponawia żądań automatycznie po ponownym zalogowaniu nadawcy.

Tekst i opis obrazka (położenie, rozmiar, CRC32) nadal mogą czekać w kolejce offline i przetrwać restart. Potwierdzenie `Queued` dotyczy tej wiadomości, nie bajtów obrazka. Jeżeli odbiorca zaloguje się po wylogowaniu nadawcy, otrzyma wiadomość, ale próba pobrania grafiki zwróci brak obrazka. Po powrocie nadawcy klient odbiorcy może ponowić żądanie. Obrazki już pobrane mogą pozostać w lokalnej historii klienta; każde nowe pobranie przez serwer ponownie wymaga nadawcy.

Żądanie jest dozwolone tylko adresatowi konkretnej wiadomości, z uwzględnieniem blokad. Baza przechowuje wyłącznie te uprawnienia i identyfikatory obrazków. Znajomość CRC32 nie daje innemu kontu dostępu. Transfery są związane z obiema sesjami i anulowane po rozłączeniu; częściowe lub uszkodzone obrazy nie trafiają do odbiorców. Pakiety sterujące obrazkami nie tworzą pustych wiadomości w historii.

Limity w tej wersji:

- Do **255 KiB na obrazek** (albo mniej, jeżeli klient zgłasza mniejszy limit) i 4 różne obrazki w jednej wiadomości.
- Do 4096 rekordów opisujących obrazki na serwer, 512 osobno na nadawcę i na odbiorcę. Wyczerpanie limitu odrzuca całą nową wiadomość jako `MboxFull`; zapis uprawnień i wiadomości jest atomowy także przy równoczesnym wysyłaniu.
- Do 64 buforów transferu naraz, 4 od nadawcy i 8 do adresata; 60 sekund na transfer. Gotowy bufor również zajmuje limit, dopóki nie przejmie go sesja odbiorcy. Fragment mieści najwyżej 1909 bajtów opcji protokołu. Obowiązuje również limit 20 żądań obrazków na minutę na sesję oraz ogólne limity pakietów.
- Opis obrazka i uprawnienie do jego pobrania mają taki sam czas życia jak wiadomość. Po dostarczeniu wiadomości są usuwane po godzinie, przy sprzątaniu co 5 minut. Później serwer nie pozwoli ponownie pobrać tego obrazu na podstawie tej wiadomości; historia klienta może nadal mieć własną kopię.

Kopie bazy obejmują tekst i identyfikatory obrazków, ale nie ich zawartość. Nie ma publicznych adresów HTTP do pobierania obrazków. Serwer nie dekoduje ani nie konwertuje grafiki: format, który potrafi wyświetlić dany klient, zależy od klienta.

Testy TCP potwierdzają wymianę PNG/BMP, wiele fragmentów, zachowanie kolejki opisów po restarcie, brak automatycznego pobierania i kopii serwerowych, izolację kont, limity i błędne transfery. Testy automatyczne nie obejmują interaktywnego interfejsu oryginalnego GG 6.0/6.1 — dostępność przycisku wysyłania do osoby offline i ponawianie pobierania zależą od klienta.

## Konferencje GG6

Klient GG wysyła osobny pakiet do każdego uczestnika, z listą pozostałych osób w atrybutach wiadomości. Serwer zachowuje tę listę i przekazuje pakiet wyłącznie wskazanemu adresatowi; ponowna wysyłka do całej listy powodowałaby duplikaty. To format opisany w sekcji 1.6.1 [protokołu libgadu](https://libgadu.github.io/protocol/).

Obsługiwane są grupy do **20 osób** (nadawca, adresat i do 18 innych uczestników), wiadomości offline, polskie znaki, formatowanie oraz obrazki. Po restarcie wiadomość nadal zawiera kontekst konferencji. Lista nie stanowi potwierdzenia, że pozostałe osoby odebrały wiadomość lub dołączyły do rozmowy — konferencje GG6 nie mają zarządzanych przez serwer pokoi ani zaproszeń do pokoju.

Każde doręczenie podlega zwykłym blokadom i limitom kolejki. Samo wymienienie kogoś w liście nie wysyła mu wiadomości ani nie daje dostępu do obrazka; prawo pobrania powstaje dopiero dla faktycznego adresata osobnej, zaakceptowanej wiadomości. Odrzucane są duplikaty numerów, nieprawidłowe długości, zbyt duże listy i wymienienie nadawcy lub adresata jako dodatkowej osoby. Wyjściowy pakiet umieszcza informacje o konferencji przed formatowaniem, zgodnie z GG6.

## Budowanie i testy

Testowana wersja Rust: **1.99.1**. Serwer buduje się jako `gg-retro`:

```sh
cargo build --locked --release -p gg-server
cargo test --locked --workspace
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/wire_integration.py
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/registration_integration.py
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/images_integration.py
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/conferences_integration.py
GG_RETRO_BINARY=target/debug/gg-retro python3 tests/presence_integration.py
node tests/patcher.test.cjs
python3 tests/client_package_test.py
GG_RETRO_BINARY="$PWD/target/release/gg-retro" python3 tests/downloads_integration.py
cargo install cargo-audit --version 0.22.2 --locked
sh tests/check-audit-graph.sh
```

Testy integracyjne tworzą wyłącznie tymczasową bazę i fikcyjne konta. Obejmują m.in. 205 wiadomości po nagłym zatrzymaniu procesu, kolejność, blokady przed synchronizacją, przejęcie sesji, prywatność statusów, import listy i granicę pojemności skrzynki. Testy kodeka sprawdzają też skrócone i nieznane ramki w obu kierunkach.

`Cargo.lock` zawiera opcjonalne RSA/MySQL z SQLx, mimo że nie są włączone ani kompilowane. Wyjątek `RUSTSEC-2023-0071` w `.cargo/audit.toml` jest połączony z testem ich nieobecności w grafie budowania. Włączenie MySQL wymaga ponownego audytu. Pozostają informacyjne ostrzeżenia o nieutrzymywanych bibliotekach `basic-toml`, `paste`, `ttf-parser`; parser fontu dostaje wyłącznie osadzony font CAPTCHA, nie pliki od klientów.

## Wdrożenie

Przykłady znajdują się w [deploy](deploy). Nie zawierają haseł ani kluczy. `advertised_ip` musi być publicznym IPv4, pod którym router przekierowuje TCP 8074. `hostname` służy stronie i patcherowi; `public_scheme` powinno być `https`.

1. Zbuduj binarkę Linux amd64 i przygotuj katalog budowania z plikiem `gg-retro` i `deploy/Dockerfile.runtime`.
2. Zbuduj obraz: `docker build -f Dockerfile.runtime -t gg-retro:0.7.0 .`.
3. W `/opt/gg-retro` umieść `compose.yaml`, konfigurację z `config.example.toml` oraz katalogi `data` i `secrets`.
4. Wygeneruj kryptograficznie losowe **32 surowe bajty** do `secrets/password.key` i pulę kodów: `python3 deploy/generate_invites.py --count 100 --output secrets/registration-invite.txt`. Nie zapisuj ich w Git ani w logach. Katalog `secrets` powinien być dostępny tylko administratorowi, a montowane pliki czytelne dla UID 10001; `data` musi należeć do UID/GID 10001.
5. W `.env` ustaw `GG_BIND_IP` na IP hosta Containers i opcjonalnie `GG_RETRO_IMAGE`. Domyślne wiązanie portów to bezpieczny loopback.
6. Zastosuj `egress-guard.sh` jako root i uruchamiaj go po starcie Dockera (OpenRC/systemd). Reguły dotyczą wyłącznie mostu `br-gg-retro`; zachowują odpowiedzi na połączenia przychodzące.
7. Uruchom `docker compose up -d`. Sprawdź `/healthz` i stan kontenera.
8. Wygeneruj konfigurację nginx przez `deploy/render_nginx.py` z tego samego pliku TOML, z prawidłowym certyfikatem. Port HTTP backendu 18074 powinien być dostępny wyłącznie z routera. Publiczne TCP 8074 przekieruj na host Containers i dopuść w zaporze VM; nie kieruj go do serwera WWW. Dodaj odpowiedni hairpin NAT dla LAN/VPN.
9. Skonfiguruj codzienne uruchomienie `backup.py` przez root. Tworzy spójne kopie SQLite i kopiuje niezbędny klucz oraz konfigurację; zachowuje 14 ostatnich kopii. Kopie są prywatne i nie są montowane w kontenerze aplikacji.

Kontener nie działa jako root, ma system plików tylko do odczytu, usunięte capabilities, limit 256 MiB i 0,5 CPU. Aplikacja ogranicza ramki, połączenia, tempo pakietów i rozmiar kolejki. nginx ogranicza rejestrację i zapisuje ścieżki bez parametrów zapytań. Sam nginx/Fail2ban dla HTTP nie chroni połączeń przekierowanych przez DNAT — ograniczenia GG działają również w aplikacji.

### Hasła i istniejąca baza

Wersja 0.7.0 dodaje migrację `20261031000000_offlinePresence.sql`: osobną tabelę opisów offline wraz z regułami widoczności. Migracja nie zmienia istniejących kont, haseł ani wiadomości. Opisy i reguły obejmuje spójna kopia SQLite; aktualizacja z 0.6.0 wykonuje tylko tę jedną nową migrację.

Wersja upstream przechowywała hasła jawnie. GG6 wymaga odzyskania hasła do obliczenia odpowiedzi na losowe wyzwanie, dlatego fork stosuje XChaCha20-Poly1305 z osobnym kluczem i powiązaniem szyfrogramu z numerem konta. Utrata klucza uniemożliwia logowanie; kopia samej bazy nie wystarcza. Wiadomości, identyfikatory obrazków i kontakty są przechowywane w bazie i wymagają ochrony uprawnień plików/kopii.

Wersja 0.6.0 dodaje migracje `20261030020000_images.sql` i `20261030030000_classicImages.sql`, bez zmiany opublikowanych wcześniej migracji. Końcowa tabela `message_images` zawiera tylko identyfikator wiadomości, rozmiar i CRC32. Druga migracja usuwa kolumny zawartości i nazwy pliku ze wcześniejszej eksperymentalnej wersji gałęzi, zachowując uprawnienia. Dawne kopie tej wersji eksperymentalnej mogą nadal zawierać obrazki. Przed aktualizacją zachowaj spójną kopię bazy i klucza. Przy wycofaniu wersji przywróć bazę sprzed migracji (późniejsze wiadomości i rejestracje zostaną wtedy cofnięte); nie uruchamiaj starszej binarki na nowym schemacie.

Migracja `20261030040000_conferences.sql` dodaje do wiadomości opcjonalną kolumnę z listą uczestników. Istniejące wiadomości prywatne zachowują treść i nie stają się konferencjami. Przy aktualizacji z 0.4.2 wykonywane są wszystkie trzy nowe migracje. Przed otwarciem nowej wersji dla użytkowników sprawdź migrację na spójnej kopii bazy; nie przywracaj automatycznie starej kopii po przyjęciu nowych wiadomości lub rejestracji.

Przed migracją istniejącej bazy wykonaj jej pełną, spójną kopię. Uruchomienie z `migrate_plaintext_passwords = true` jednorazowo szyfruje stare hasła w transakcji, wykonuje VACUUM i czyści WAL. Potem wyłącz tę opcję. Bez niej serwer odmawia uruchomienia na jawnych hasłach. Nie można cofnąć się do upstream na zmigrowanej bazie bez przywrócenia kopii sprzed migracji.

Plik zaproszeń zawiera po jednym kodzie w każdym wierszu. Każdy kod pozwala na **3 udane rejestracje**. Baza przechowuje wyłącznie skróty SHA-256 kodów oraz trwałe liczniki użyć. Rejestracja konta, zużycie kodu/CAPTCHA i przydzielenie numeru są jedną transakcją. Błędna próba (także zajęty numer lub e-mail) nie zużywa zaproszenia ani numeru automatycznego. Równoczesne rejestracje nie przekraczają limitu.

Po zmianie pliku odtwórz kontener (`docker compose up -d --force-recreate gg-retro`), aby odczytał aktualny plik. Usunięty kod zostaje wyłączony. Przywrócenie kodu i restart nie zerują licznika, a usunięcie konta nie zwraca użycia. Pusty plik zamyka rejestrację bez blokowania logowania istniejących kont. Liczniki kodów i kolejny numer są częścią kopii SQLite; odtworzenie starszej kopii cofa także te liczniki, dlatego przy odtwarzaniu po utracie nowszych rejestracji należy unieważnić dawne kody i wydać nową pulę.

Ręcznie wybrany wysoki numer nie przesuwa numeracji automatycznej. Zajęte numery są pomijane, a istniejące konta zachowują numery po migracji. Nowa numeracja ma własny trwały licznik od 112233, niezależny od dawnego losowania i SQLite AUTOINCREMENT. Limit 16777215 wynika z 24-bitowego pola GG6.

Zmiana puli zaproszeń nie zmienia haseł istniejących kont. Klucza do haseł nie wolno podmieniać bez procedury migracji szyfrogramów.
