# Felügyelet nélküli szerveres hozzáférés / Unattended server access

[English](#english)

## Magyar

A MIDAS-041 jelenlegi megvalósítása a szerveres szabályzat tárolását, webes szerkesztőjét, adminisztrációs API-ját, belső döntési szolgáltatását és a Windows-munkamenetek hiteles agentcsatornán történő lekérdezését tartalmazza. Távoli kép- vagy beviteli kapcsolatot még nem indít. A munkamenet-kijelölő webes felület, a natív megfigyelés bekötése a döntésbe, az aláírt, egyszer használható felhatalmazás és a tényleges helyi jóváhagyás még szükséges; a feladat ezért nem lezárt.

### Beállítás

Az ügyfél és az adott szerver kapcsolója külön, alapból kikapcsolt. Telephely vagy csoport nem engedélyezheti helyettük a hozzáférést. A szerver bekapcsolásához konkrét karbantartási időablak kell; az ügyfél engedélyezése a gép saját kapcsolóját nem módosítja. A szabályzat szerverprofilja mindig előírja az újraindítás megerősítését és az utána következő kapcsolatellenőrzést. Ezek végrehajtása az újraindítási munkafolyamat feladata lesz.

| Végpont | Olvasás | Módosítás |
| --- | --- | --- |
| `GET/PUT /api/tenants/{id}/unattended-policy` | Ügyfélszintű `access.manage` | Ugyanez, friss passkey; engedélyezéskor ügyfélszintű `remote.unattended` is |
| `GET/PUT /api/tenants/{id}/devices/{device}/unattended-policy` | `device.read` a szerveren | `access.manage` a szerveren, friss passkey; engedélyezéskor `remote.unattended` is a szerveren |

Az engedélyezéshez szükséges saját jogot a szerver a kapcsoló állapotától függetlenül ellenőrzi. Az alkalmazható tiltás elsőbbséget élvez. Kikapcsoláshoz nem kell unattended jog, de hozzáférés-kezelési jog és friss passkey igen. Az írások CSRF-védettek. A jogosultság, a módosítás és az audit egy következetes adatbázis-tranzakcióhoz tartozik; sikertelen audit esetén a módosítás visszagördül.

Ügyfélszintű kérés: `{"enabled":true}`. Gépszintű kérés:

```json
{
  "enabled": true,
  "maintenance_window": {
    "starts_at": 1794355200,
    "ends_at": 1794358800
  }
}
```

Az időpontok UTC Unix-másodpercek. Az intervallum eleje beleértendő, a vége kizárt; a bekapcsolási kérés idején a végpontnak jövőbelinek kell lennie. Jelenleg egyszeri ablak tárolható, ismétlődő ütemezés még nincs. A `within_maintenance_window` kizárólag az időablakról tájékoztat, nem jelenti egy távoli művelet engedélyezését.

### Webes beállítás

Az ügyfél kapcsolója a **Hozzáférés** oldalon, ügyfélszintű hozzáférés-kezelési joggal érhető el. Az adott szerver kapcsolója és időablaka az **Eszközök → kiválasztott szerver → Szabályzat és csoportok** lapon található. Gépszintű kezelési joggal szerkeszthető; csak megtekintési joggal olvasható. Munkaállomáson ez a szerkesztő nem jelenik meg.

Az időablak a böngésző helyi időzónáját használja, a mezők alatt UTC-időpont is látszik. A nem létező vagy az óraátállításkor kétszer előforduló helyi időpont elutasított. A szerver kikapcsolása törli a tárolt időablakot. A mentés szükség esetén passkey-megerősítést kér; a megszakított hitelesítés megőrzi a vázlatot. A panel csak visszaolvasott eredményt jelez sikeresként. Az automatikus frissítés nem írja felül a vázlatot; az észlelt párhuzamos változás elvetést és újbóli áttekintést kér. A mentés előtti ellenőrzés nem adatbázisszintű verziózár: egyidejű API-írások közül az utolsó érvényesül, mindegyik külön auditbejegyzéssel.

A pillanatnyi időablak-állapot az utolsó lekérés idejére vonatkozik. Sem ez, sem a két bekapcsolt kapcsoló nem jelenti egy távoli kapcsolat engedélyezését. A felület magyarul és angolul, világos és sötét módban használható.

### Windows-munkamenetek lekérdezése

A `POST /api/tenants/{id}/devices/{device}/remote-sessions` a bejelentkezett kezelő `device.read` és `remote.view` jogát ellenőrzi a célgépen. Origin- és CSRF-védelem szükséges. A kérés a gép már hitelesített mTLS-kapcsolatán jut el az agenthez; a böngésző nem adhat meg felhasználói jelenlétet vagy agentválaszt. A válasz `connection_id` mezőt és `sessions` listát tartalmaz. Egy elem például:

```json
{
  "selection": {
    "connection_id": "01646509-2e0a-4ebc-886b-a14c9674c971",
    "os_session_id": "3",
    "generation": "5461e30a-55f9-4309-bb64-567e392dccd6"
  },
  "presence": "occupied",
  "account": "EXAMPLE\\operator",
  "station": "RDP-Tcp#2",
  "console": false,
  "locked": true
}
```

A Windows-szolgáltatás kérésre használja a WTS API-t, nincs folyamatos munkamenet-lekérdezés. A zárolt vagy leválasztott, de bejelentkezett munkamenet foglalt. Üres csak az ismert állapotú, csatlakoztatott cél lehet, ha nincs felhasználónév, tartomány és bejelentkezési idő. Más állapot ismeretlen; lekérdezési hiba, változó lista vagy hibás adat esetén a teljes eredmény elutasított. A 0. munkamenet és az RDP-figyelő nem választható cél. A generáció az észlelt adatok megváltozásakor, eltűnéskor vagy szolgáltatási munkamenet-eseménynél megújul. Sikertelen megfigyelés után sem marad használatban a korábbi generáció.

Egyszerre egy natív lekérdezés futhat, újracsatlakozáson át is. A szerver egyszeri kérésazonosítóhoz és az adott kapcsolathoz köti a választ; nyolc másodperc után elveti. Hiányzó képesség, megszakadás, párhuzamos kérés vagy több alkalmas agentkapcsolat esetén a végpont `409` választ ad. Új kapcsolat új azonosítót kap. A protokoll legfeljebb 128 munkamenetet fogad egy válaszban a meglévő 64 KiB-os üzenethatáron belül; túlcsorduláskor nem szolgáltat részleges listát.

Válaszadás előtt a szerver ismét ellenőrzi a böngészős munkamenetet, jogosultságot, gépregisztrációt és a konkrét agenttanúsítványt. A sikeres lekérdezés auditja a kapcsolat azonosítóját és a munkamenetek számát tárolja, felhasználónevet nem. Auditálási hiba esetén a végpont nem adja ki a listát. A válasz megfigyelés, nem vezérlési engedély; későbbi végrehajtás előtt új megfigyelés és célazonosság-ellenőrzés kell.

A `remote.sessions` képességet jelenleg csak az eseményértesítésekre feliratkozott Windows-szolgáltatás hirdeti. A Windows előtérben futó módja és a Linux-agent még nem támogatja; ezekből nem keletkezik hamis „üres” eredmény. A webes célkijelölő még nem készült el. Az automatizált próbák a protokollt, állapotosztályozást, mTLS-kapcsolatot, visszajátszás elutasítását, jogosultság- és tanúsítvány-visszavonást, időtúllépést, auditálási hibát és újracsatlakozást vizsgálják. Valódi SYSTEM-szolgáltatásos, több RDP-felhasználós, zárolási, kijelentkezési és UAC-próba még szükséges; a fejlesztői gépre ehhez nem települ szolgáltatás.

Natív szerződés: [WTS-munkamenetállapotok](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ne-wtsapi32-wts_connectstate_class), [munkamenet-információ és zárolási jelzők](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w).

### Munkamenet-döntés

A belső `Authorizer::check_unattended_request` megköveteli mindkét kapcsolót, a szerverprofilt, a megtekintési és vezérlési jogokat, a külön unattended jogot, öt percen belüli passkey-hitelesítést, az indoklást és a kijelölt munkamenetet. A rendszergazdai mód és az UAC további külön jogosultságot kér. A jelenlegi jogosultságokat minden döntésnél újraolvassa.

Az indoklás nem lehet üres, legfeljebb 512 UTF-8 bájtos lehet, és nem tartalmazhat vezérlőkaraktert. Ez a célmunkamenet adataival együtt az auditbejegyzés mérethatárán belül marad.

A jelenlétet csak az igazolt agentcsatorna megfigyelése adhatja; a böngészőből nem fogad jelenlétállítást. A megfigyelés az ügyfélhez, géphez, agentkapcsolathoz és az operációs rendszer munkamenetének egyedi generációjához kötött. Harminc másodpercnél régebbi vagy jövőbeli megfigyelés, ismeretlen jelenlét, visszavont kapcsolat és eltérő kijelölés nem adhat pozitív döntést. A döntés újraellenőrzi a konkrét agenttanúsítvány lejáratát és visszavonását is. Újracsatlakozáskor új kapcsolat-azonosító keletkezik.

A foglalt cél `LocalConsentRequired` eredményt ad, amiből nem indulhat unattended kapcsolat. A natív megfigyelőnek a zárolt és a leválasztott, de bejelentkezett felhasználó munkamenetét is foglaltnak kell jelentenie. Üres cél esetén az aktuális karbantartási ablak is kötelező. Az `Eligible` eredmény legfeljebb 30 másodpercig, de az ablak, a böngészős munkamenet és a friss passkey határidejénél nem tovább érvényes. A kérés eredménye, módja, UAC-igénye, érvényes indoklása, kijelölt célja és lejárata auditálható.

Ez szabályzatdöntés, nem végrehajtható felhatalmazás. A későbbi aláírt felhatalmazás kiadásánál és felhasználásánál újra kell ellenőrizni a jogosultságot, szabályzatot, célmunkamenetet, lejáratot és visszavonást. Nincs HTTP-végpont, amely böngésző által megadott „üres munkamenet” alapján kapcsolatot indítana.

## English

The current MIDAS-041 implementation provides server-policy storage, a web editor, an administration API, an internal decision service and Windows session queries over the authenticated agent channel. It does not start remote media or input. The web session selector, native observation integration into the decision, signed single-use grants and actual local consent remain; the task stays open.

### Configuration

Customer and individual server opt-ins are separate and default to off. Sites and groups cannot enable access on their behalf. Enabling a server requires an explicit maintenance window; enabling the customer does not change the device switch. The server profile always requires reboot confirmation and subsequent connectivity verification. The reboot workflow must enforce these requirements when implemented.

| Endpoint | Read | Change |
| --- | --- | --- |
| `GET/PUT /api/tenants/{id}/unattended-policy` | Tenant-wide `access.manage` | The same permission and a fresh passkey; enabling also requires tenant-wide `remote.unattended` |
| `GET/PUT /api/tenants/{id}/devices/{device}/unattended-policy` | `device.read` on the server | `access.manage` on the server and a fresh passkey; enabling also requires `remote.unattended` on the server |

The server checks the actor's held permission independently of the opt-in being edited. Applicable denials take precedence. Disabling does not require unattended permission but still requires access management and a fresh passkey. Writes enforce CSRF protection. Authorization, the change and its audit entry share a consistent database transaction; audit failure rolls back the change.

The tenant body is `{"enabled":true}`. The device body has `enabled` and `maintenance_window` with `starts_at` and `ends_at`, as illustrated above. Timestamps are UTC Unix seconds. The start is inclusive and the end exclusive; enabling requires an end in the future. Windows are currently one-off, without recurrence. `within_maintenance_window` describes time eligibility only, not authorization to execute an operation.

### Web configuration

The customer switch is on **Access**, available with tenant-wide access-management permission. The server switch and window are under **Devices → selected server → Policy and groups**. Device-scoped access managers can edit; viewers get a read-only view. Workstations do not show this editor.

The window uses the browser's local time zone and displays UTC below each input. Nonexistent or repeated local times at clock changes are rejected. Disabling the server clears its stored window. Saving asks for passkey confirmation when needed; cancelling preserves the draft. Success requires reading back the stored result. Background refresh does not overwrite a draft; detected concurrent edits require discarding it and reviewing the current settings. The pre-save check is not a database version lock: concurrent API writes use last-writer-wins semantics, each with its own audit entry.

Window status describes the last fetch time. Neither that status nor both opt-ins authorize a remote connection. The editor supports Hungarian and English, light and dark themes.

### Querying Windows sessions

`POST /api/tenants/{id}/devices/{device}/remote-sessions` requires the signed-in operator's `device.read` and `remote.view` permissions on the target, plus Origin and CSRF validation. The request travels over the device's authenticated mTLS connection; the browser cannot supply presence or an agent response. The response contains `connection_id` and `sessions`. Each entry has `selection` (connection ID, OS session ID and generation), `presence` (`occupied`, `vacant` or `unknown`), `account`, `station`, `console` and nullable `locked`, as illustrated above.

The Windows service calls WTS on demand without continuous session polling. Locked or disconnected signed-in sessions remain occupied. Only a connected target with known flags and no username, domain or logon time may be vacant. Other states are unknown; query failures, changing enumeration or malformed data reject the entire observation. Session 0 and RDP listeners are excluded. Changed data, disappearance or a service session-change notification renews the generation. Failed observations also invalidate previous generations.

Only one native query may run, including across reconnects. The server binds each reply to a one-use request identifier and its agent connection, and discards it after eight seconds. Missing capability, disconnection, concurrent queries or multiple eligible agent connections return `409`. Reconnection changes the connection ID. The protocol accepts up to 128 sessions within the existing 64 KiB message bound; overflow never yields a partial list.

Before returning data, the server rechecks the browser session, permissions, enrollment and the specific agent certificate. Successful-query audit records contain the connection ID and session count, never account names. Audit failure prevents disclosure of the list. A response is an observation, not control authorization; later execution needs another observation and target-identity check.

Only the Windows service with session notifications enabled currently advertises `remote.sessions`. Windows foreground mode and Linux do not support it yet and cannot report false vacancy. The web target selector is pending. Automated tests cover protocol validation, classification, mTLS exchange, replay rejection, permission and certificate revocation, timeout, audit failure and reconnection. Actual SYSTEM service, multi-user RDP, lock, logoff and UAC trials remain required; no service is installed on the development machine for these checks.

Native contracts: [WTS session states](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ne-wtsapi32-wts_connectstate_class), [session information and lock flags](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w).

### Session decision

The internal `Authorizer::check_unattended_request` requires both opt-ins, a server device, viewing and control permissions, dedicated unattended permission, passkey authentication within five minutes, a reason and explicit session selection. Administrative mode and UAC require their additional permissions. Every decision reads current authorization.

The reason must be nonblank, at most 512 UTF-8 bytes, and contain no control characters. This keeps it and the target-session metadata within the audit record's size limit.

Presence comes from the verified agent channel, never a browser assertion. The observation binds customer, device, agent connection and the OS session's unique generation. Observations at least 30 seconds old or in the future, unknown presence, revoked connections and mismatched selections cannot authorize access. Each decision also rechecks the specific agent certificate's expiry and revocation. Reconnection creates a new connection identifier.

An occupied target returns `LocalConsentRequired`, which cannot start an unattended session. The native observer must also report locked and disconnected sessions with signed-in users as occupied. A vacant target additionally requires the current maintenance window. `Eligible` expires within 30 seconds and no later than the maintenance window, browser session or fresh-passkey deadline. The decision, mode, UAC request, valid reason, selected target and expiry are audited.

This is a policy decision, not an executable grant. Issuing and consuming the later signed grant must revalidate permissions, policy, target session, expiry and revocation. There is no HTTP endpoint that starts a connection from a browser-supplied claim of a vacant session.
