# Böngészős munkamenetek / Browser sessions

[English](#english)

## Magyar

A HTTPS-szerver már kiszolgálja a passkey-, jelszó+TOTP- és helyreállítókód-belépést, valamint a saját munkamenetek megtekintését és visszavonását. A webpanel belépőképernyője, az első adminisztrátor létrehozása és az ügyfélre/eszközre szűkített RBAC külön feladat. Nincs nyilvános regisztráció, alapértelmezett fiók vagy általános rendszergazdai hozzáférés pusztán a belépés miatt.

### Bekapcsolás

A [PostgreSQL-migrációkat](adatbazis.md) külön migrációs fiókkal kell alkalmazni. A futó szerver korlátozott adatbázisfiókot használ. A [HTTPS-beállítás](szerver.md) mellé az alábbi konfiguráció szükséges:

| Beállítás | Jelentés |
| --- | --- |
| `MIDAS_AUTH_ORIGIN` / `--auth-origin` | A végleges HTTPS-origin, például `https://midas.example.com`; nincs útvonal, query vagy fragment |
| `MIDAS_AUTH_KEY_FILE` / `--auth-key-file` | A TOTP-titkosítás meglévő, védett kulcsfájlja; pontosan 32 nyers véletlen bájt, nem hexadecimális szöveg |
| `MIDAS_SESSION_SECONDS` | Abszolút munkamenet-élettartam másodpercben; alapérték 43200, azaz 12 óra |
| `MIDAS_SESSION_IDLE_SECONDS` | Inaktivitási lejárat másodpercben; alapérték 1800, azaz 30 perc; legfeljebb az abszolút élettartam |
| `MIDAS_SESSION_SOCKET_CAPACITY` | Az egy példányon nyitva tartható munkamenet-állapotkapcsolatok kapacitása; alapérték 128 |

A két hitelesítési beállítás együtt kötelező. Adatbázis vagy HTTPS nélkül a bekapcsolás hibát okoz. A kulcs nem lehet üres vagy csupa nulla, nincs automatikus újragenerálás. Unix alatt a csoport és más felhasználók nem férhetnek a fájlhoz. Windows alatt a telepítés feladata a szolgáltatásfiókra és az illetékes rendszergazdákra szűkített ACL; ezt a jelenlegi fájlbetöltő nem vizsgálja. A kulcs helyét és mentését az adatbázistól elkülönítve kell védeni. Kulcsrotáció még nincs.

A munkamenet-élettartam biztonsági szabályzat; a kapcsolatkapacitás visszaélés elleni védelem. Egyik sem a távsegítő videó, a gép RAM-ja vagy a platform általános erőforrásplafonja. Az egyszerű `just dev` előnézet továbbra is hitelesítés nélküli loopback fejlesztői felület, mintadatokkal. Nem kapcsolódik éles gépekhez.

### Azonosító és CSRF

A böngésző véletlen, 256 bites azonosítót kap a `__Host-midas-session` sütiben: `Secure; HttpOnly; SameSite=Strict; Path=/`, Domain nélkül. Az adatbázis csak ennek SHA-256 lenyomatát tárolja. A publikus munkamenet-UUID önmagában nem hitelesítő adat. Nincs bearer token a localStorage-ban, URL-ben vagy WebSocket-alprotokollban.

Minden sikeres belépés új azonosítót ad. Ha a böngésző korábbi munkamenetet küldött, azt ugyanabban az adatbázis-tranzakcióban töröljük. Munkamenet csak legfeljebb egyperces, még aktuális belső hitelesítési eredményből jöhet létre. Az abszolút lejárat nem hosszabbodik meg. A kérések minden alkalommal ellenőrzik a fiók engedélyezett állapotát, a hitelesítők verzióját és a két lejáratot. Az aktivitás időpontját legfeljebb percenként írjuk újra; rövid szabályzatnál ennél sűrűbben. Az automatikus WebSocket-ellenőrzés nem hosszabbítja az inaktivitást.

A `GET /api/session` adja vissza a munkamenet adatait és a `csrf_token` értéket. Utóbbi HMAC-SHA256-tal, a véletlen sütititokból és külön kontextusból származik: lapfrissítés és több böngészőlap között stabil, de más munkamenettel nem használható. Módosító kéréshez pontos `Origin` és `X-CSRF-Token` fejléc kell. A tokenellenőrzés állandó idejű MAC-ellenőrzést használ. Az előzetes belépési kérések is pontos Origint, JSON-t és a szerveroldali próbálkozáskorlátot igénylik. Nincs engedékeny CORS.

A hitelesítési válaszok `Cache-Control: no-store` fejlécet kapnak. A kettőzött munkamenetsütit és Origint, a hibás formátumú azonosítót és az API query paramétereit elutasítjuk. A naplóban útvonalsablon, státuszkód és időtartam szerepel, a fejléc, jelszó, kód és üzenettörzs nem.

### Végpontok

| Metódus és útvonal | Kérés / eredmény |
| --- | --- |
| `POST /api/auth/passkey/start` | `{login}` → `{id, options}` és rövid életű `__Host-midas-login` kötősüti |
| `POST /api/auth/passkey/finish` | `{id, response}` WebAuthn-válasszal és a kötősütivel → új munkamenet |
| `POST /api/auth/password` | `{login, password, code}` → új munkamenet, ha a jelszó és a TOTP is helyes |
| `POST /api/auth/recovery` | `{login, code}` → új munkamenet, a kód egyszeri elfogyasztásával |
| `GET /api/session` | Saját munkamenet és CSRF-token |
| `GET /api/sessions` | Saját aktív munkamenetek, legfeljebb a legújabb 100; titkok nélkül |
| `POST /api/session/logout` | Aktuális munkamenet visszavonása és sütik törlése |
| `DELETE /api/sessions/{id}` | Saját megadott munkamenet visszavonása; más felhasználóé nem törölhető |
| `GET /api/session/socket` | Hitelesített WebSocket az aktuális munkamenet állapotához |

A `response` a böngésző WebAuthn-eredményének JSON-reprezentációja. Az API nem végez passkey-regisztrációt, jelszócserét vagy fiókadminisztrációt. A passkey-indítás minden alkalommal új kötősütit ad; ugyanabban a böngészőben párhuzamosan indított belépési folyamatból csak a legutóbbi kötés marad használható. Sikertelen vagy megszakadt belépést újra kell indítani.

A próbálkozáskorlát a TCP-kapcsolat tényleges partnercímét használja. Az `X-Forwarded-For` nem megbízható forrás; proxy mögött ezért a proxy címe lesz a közös forrás. Megbízhatóproxy-konfiguráció külön fejlesztés. A felhasználónévhez kötött passkey-opciók hitelesítőlistája továbbra is felfedhet fiókbeállítást: a közös hibaüzenet és a tartós korlátozás mérsékli, de nem szünteti meg ezt a felderítési lehetőséget.

### Nyitott kapcsolatok és visszavonás

A WebSocket pontos Origint és érvényes munkamenetsütit kér. Az első üzenet öt másodpercen belül `{ "csrf_token": "…" }`; csak sikeres ellenőrzés után küld munkamenetadatot. Ezután `{ "type": "session" }` kérhet aktuális adatot. Nincs eszközvezérlő vagy rendszergazdai parancs ezen a csatornán.

Minden alkalmazásüzenet előtt új adatbázisos ellenőrzés történik. A helyi kijelentkezés/visszavonás külön értesítéssel lezárja a kapcsolatot. Másik példányon vagy közvetlenül az adatbázisban történt változást az aktív kapcsolaton 15 másodpercenként végzett ellenőrzés veszi észre; a következő üzenet már előbb elutasítható. Adatbázis- vagy értesítési hibánál a kapcsolat megszűnik. A hálózati írások öt másodperces időkorlátot kapnak. A csatorna 4 KiB-os üzenetkerete és tíz másodpercenként legfeljebb 30 bejövő üzenete az állapotcsatornát védi, nem médiaátviteli beállítás.

A már megkezdett HTTP-válasz nem hívható vissza. A későbbi privilegizált műveleti rétegnek a végrehajtás határán is ellenőriznie kell az aktuális munkamenetet, ügyfélt, eszközt, engedélyt és szükség esetén a friss passkey-megerősítést. A mostani modul nem helyettesíti az RBAC-t, az auditot vagy a távoli műveleti felhatalmazásokat. Fiókletiltáskor a későbbi adminisztrációs műveletnek a verziót is növelnie kell, hogy újraengedélyezéskor a régi munkamenet se éledjen fel.

### Ellenőrzés

A `just test` a CSRF-kötést, szigorú tokenformátumot, origint és konfigurációt is vizsgálja. A `just test-database` valódi PostgreSQL-en ellenőrzi a HTTP-belépést mindhárom móddal, a sütiket, CSRF-elutasításokat, azonosítócserét, tulajdonosi szűrést, lejáratot, letiltást, verzióváltást és új példányból látható visszavonást. Valódi loopback WebSocketen próbálja a hibás origint/CSRF-et, a nyitott kapcsolat kijelentkezését és külső adatbázisos visszavonását. A tesztkapcsolat HTTP; az alkalmazás bekapcsolási szabálya és a külön TLS-tesztek igazolják az éles HTTPS-követelményt. Fizikai passkey és a webpanel teljes böngészős belépése a későbbi integráció próbája.

Források: [OWASP munkamenetek](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html), [CSRF](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html), [WebSocket-biztonság](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html).

## English

The HTTPS server provides passkey, password+TOTP and recovery-code login, plus inspection and revocation of an operator's own browser sessions. The sign-in UI, first-administrator setup and tenant/device RBAC remain separate work. There is no open registration, default account or implied administrative access.

### Configuration

Apply the [database migrations](adatbazis.md#english) with the separate migration identity. The running server uses the restricted runtime role. Alongside [HTTPS](szerver.md#english), configure:

| Setting | Meaning |
| --- | --- |
| `MIDAS_AUTH_ORIGIN` / `--auth-origin` | Final HTTPS origin without path, query or fragment |
| `MIDAS_AUTH_KEY_FILE` / `--auth-key-file` | Protected existing TOTP encryption key file: exactly 32 raw random bytes, not hexadecimal text |
| `MIDAS_SESSION_SECONDS` | Absolute lifetime in seconds; default 43200 / 12 hours |
| `MIDAS_SESSION_IDLE_SECONDS` | Idle expiry in seconds; default 1800 / 30 minutes; no greater than absolute lifetime |
| `MIDAS_SESSION_SOCKET_CAPACITY` | Concurrent session-status sockets per instance; default 128 |

Both authentication settings are required together. Missing database or HTTPS fails startup. Empty/all-zero keys are rejected and never regenerated. Unix permissions must exclude group/other access. Windows deployments must restrict ACLs to the service identity and responsible administrators; the current loader does not inspect Windows ACLs. Protect and back up the key separately from the database. Key rotation is not implemented.

Lifetime is security policy; socket capacity is an abuse control. Neither limits video, host RAM or general platform resources. `just dev` remains a loopback preview with sample data, without enabled authentication or managed devices.

### Sessions and CSRF

The browser receives a random 256-bit `__Host-midas-session` cookie with `Secure; HttpOnly; SameSite=Strict; Path=/` and no Domain. PostgreSQL stores only its SHA-256 digest. A separate public session UUID does not authenticate. No bearer token is stored in localStorage, URLs or WebSocket subprotocols.

Successful login issues a fresh secret and atomically deletes the supplied previous session. Issuance consumes an internal authentication result no older than one minute and checks its credential version. Absolute expiry never extends. Every authorization validates account state, credential version, absolute expiry and idle expiry against current database state. Activity writes are throttled to once a minute, or more often for shorter policies. Automatic WebSocket validation never renews inactivity.

`GET /api/session` returns session metadata and `csrf_token`. The latter uses HMAC-SHA256 keyed by the random cookie secret with a distinct context. It survives refreshes/tabs, remains stable across instances, and is unusable with another session. Unsafe requests require the exact Origin and `X-CSRF-Token`; MAC verification is constant-time. Pre-authentication login also requires exact Origin, JSON and durable attempt limits. No permissive CORS is enabled.

Responses use `Cache-Control: no-store`. Ambiguous duplicate session cookies/Origin headers, malformed tokens and API query parameters are rejected. Request logs contain route templates, status and duration, never headers, credentials, codes or bodies.

### API

| Method and route | Request / result |
| --- | --- |
| `POST /api/auth/passkey/start` | `{login}` → `{id, options}` and short-lived `__Host-midas-login` binding cookie |
| `POST /api/auth/passkey/finish` | `{id, response}` with WebAuthn JSON and binding cookie → session |
| `POST /api/auth/password` | `{login, password, code}` → session after both factors verify |
| `POST /api/auth/recovery` | `{login, code}` → session, consuming the code once |
| `GET /api/session` | Current session metadata and CSRF token |
| `GET /api/sessions` | Latest 100 active own sessions, without secrets |
| `POST /api/session/logout` | Revoke current session and clear cookies |
| `DELETE /api/sessions/{id}` | Revoke an own session; another operator's session cannot be deleted |
| `GET /api/session/socket` | Authenticated current-session status WebSocket |

Credential enrollment, password changes and account administration are not public endpoints. Passkey start rotates the binding cookie, so only the latest simultaneous login ceremony in one browser retains its binding. Restart interrupted/failed ceremonies.

Rate limits use the actual TCP peer address. `X-Forwarded-For` is ignored; behind a proxy all clients therefore share its source quota. Explicit trusted-proxy support is future work. Username-bound passkey options still disclose credential configuration; uniform errors and persistent limits mitigate, but do not eliminate, this discovery risk.

### Revocation and open connections

The WebSocket requires exact Origin and a valid cookie. Its first message must be `{ "csrf_token": "…" }` within five seconds; metadata is released only after verification. Later `{ "type": "session" }` requests current metadata. No device or administrative commands exist on this channel.

Each application message rechecks the database. Local logout/revocation notifies and closes open sockets. Active-only checks every 15 seconds detect changes from another instance or directly in the database; the next message can be rejected sooner. Database/notification errors close the connection. Writes have a five-second timeout. The 4 KiB frame/message limit and 30 incoming messages per ten seconds protect this status channel and are not media settings.

An already-started HTTP response cannot be recalled. Future privileged operations must validate live session, tenant, device, permission and fresh passkey requirements at the execution boundary. This module does not replace RBAC, audit or remote-operation grants. Future account-disable operations must also increment credential version so re-enabling cannot revive old sessions.

### Verification

`just test` covers CSRF binding, strict token parsing, origins and configuration. `just test-database` uses actual PostgreSQL for HTTP login through all three methods, cookie flags, CSRF rejection, token rotation, ownership checks, expiry, disable/version changes and revocation visible from another instance. Real loopback sockets test origin/CSRF rejection, logout while connected and external database revocation. Test sockets use HTTP; startup validation and separate TLS tests establish the production HTTPS requirement. Physical authenticators and complete browser UI sign-in remain later integration checks.

References: [OWASP sessions](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html), [CSRF](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html), [WebSockets](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html).
