# Jogosultságkezelés / Authorization

[English](#english)

## Magyar

A `midas_server::authorization::Authorizer` belső szolgáltatás aktuális böngészős munkamenet, ügyfél, célgép és kért művelet alapján dönt. Alapállapotban minden tiltott. A modul még nem indít távoli műveletet, nem állít ki aláírt felhatalmazást, és nincs nyilvános jogosultságszerkesztő végpontja. A kezelőfelület és az auditált adminisztráció bekötése külön feladat.

### Szerepek és hatókör

| Szerep | Alapengedélyek |
| --- | --- |
| Observer | Eszközadatok és monitorozás olvasása |
| Technician | Az Observer jogai, továbbá normál képernyőmegtekintés és vezérlés |
| Administrator | Az engedélykatalógus összes művelete, a hozzárendelt ügyfélen és hatókörön belül |

Az Administrator sem léphet át másik ügyfélhez kifejezett tagság és hozzárendelés nélkül. A kiemelt műveleteknél ugyanúgy friss passkey kell, és az egyedi tiltás rá is érvényes. A beépített szerepeket és engedélyeiket a futtatófiók adatbázisszinten sem írhatja át; változtatásuk migrációhoz tartozik.

Az egyedi szerepek engedő és tiltó szabályokat tartalmazhatnak. A szerep neve egyedi, kis- és nagybetűtől függetlenül, ezért beépített szerep neve sem használható újra. A szerepdefiníciók platformszintűek; a hozzárendelés mindig ügyfélhez tartozik. Globális szerep módosítása nem következik automatikusan egy ügyfél `access.manage` jogából: a későbbi adminisztrációs rétegnek ehhez külön platformszintű felhatalmazást kell megkövetelnie.

A tagság, szerephozzárendelés és felhasználói felülbírálás ügyfél-RLS alatt áll. A hatókör teljes ügyfél, egy telephely, egy eszközcsoport vagy egy gép lehet. A telephelyet és csoporttagságot a szerver a tényleges eszközleltárból olvassa; a kliens nem állíthatja magáról, hogy másik csoportba tartozó gépet kezel. Az összetett idegen kulcsok a hatókörhöz is megőrzik az ügyfélhatárt.

Minden alkalmazható tiltás elsőbbséget élvez minden engedéllyel szemben. Például egy teljes ügyfélre szóló `remote.control` tiltást nem ír felül a gépre megadott engedély; egy csoporthoz rendelt tiltó szerepet sem kerül meg az Administrator szerep. Célgép nélküli döntés a teljes ügyfélre szól: ehhez ügyfélszintű engedély kell, és bármely szűkebb hatókörű tiltás is elutasítja. A `remote.*` engedélyek mindig konkrét célgépet igényelnek. Kikapcsolt szerep engedései megszűnnek, tiltásai viszont érvényben maradnak; a tiltás a hozzárendelés törlésével szüntethető meg. Ha egy engedélyhez nincs alkalmazható megadás, a kérés elutasított. Ismeretlen engedély és wildcard nem használható.

A munkamenet első ellenőrzése után a döntés új adatbázis-állapotból, egy következetes, csak olvasható tranzakcióban készül, amely a munkamenetet is újraellenőrzi. A kért engedélyeket közös lekérdezés értékeli ki; nincs háttérben futó jogosultság-polling vagy elavuló engedélycache. Egy hozzárendelés törlése, csoporttagság változása, szerep kikapcsolása vagy ügyféltagság letiltása a következő döntésben érvényesül. Az adatbázis-kapcsolat ellenőrzése már mind a nyolc ügyfélhez kötött tábla kényszerített RLS-ét megköveteli.

### Engedélyek

| Azonosító | Művelet | Friss passkey |
| --- | --- | --- |
| `device.read` | Eszközadatok megtekintése | Nem |
| `monitor.read` | Monitorozási adatok megtekintése | Nem |
| `remote.view` | Képernyő megtekintése | Nem |
| `remote.control` | Normál távoli bevitel | Nem |
| `remote.admin` | Rendszergazdai távvezérlési mód | Igen |
| `remote.uac.interact` | Windows UAC kezelése bármelyik módban | Igen |
| `remote.unattended` | Felügyelet nélküli szerveres kérés | Igen |
| `terminal.execute` | Háttérterminál | Igen |
| `files.read` | Háttér-fájlolvasás | Nem |
| `files.write` | Háttér-fájlmódosítás | Igen |
| `script.execute` | Szkriptfuttatás | Igen |
| `updates.manage` | Frissítések kezelése | Igen |
| `domain.join` | Tartományba léptetés | Igen |
| `access.manage` | Ügyfélhez tartozó hozzáférés kezelése | Igen |
| `audit.read` | Ügyfélhez tartozó audit olvasása | Nem |

A frissesség jelenleg legfeljebb ötperces, ellenőrzött passkey-belépést jelent. TOTP vagy helyreállító kód nem teljesíti, még Administrator szereppel sem. A munkamenet aktuális állapota és a hitelesítők verziója is érvényes kell legyen. A későbbi, konkrét művelethez kötött újbóli megerősítés és a rövid életű végrehajtási felhatalmazás nem része ennek a modulnak.

### Távvezérlési kérés

A `check_remote_request` egy célgéphez ellenőrzi a kérés indításának jogosultsági előfeltételeit:

1. Mindig kell `device.read`, `remote.view` és `remote.control`.
2. A rendszergazdai módhoz hozzáadódik `remote.admin`.
3. Az UAC kérése külön `remote.uac.interact` jogot igényel, normál módban is. Csak Windows-célgépnél értelmezett.
4. A felügyelet nélküli kéréshez `remote.unattended`, szerver típusú célgép, valamint az ügyfél és a konkrét gép `unattended_enabled` beállítása is kell. Mindkét kapcsoló alapból hamis.

Ugyanezek a gép- és szabályzatfeltételek az `authorize` hívásra is vonatkoznak, ha az önmagában `remote.uac.interact` vagy `remote.unattended` jogot kér. Az eredmény csak jogosultsági döntés. Nem bizonyítja, hogy nincs helyi felhasználó, és nem helyettesíti a cél OS-munkamenet hitelesített azonosítását, a beleegyezést, az indoklást, a lejáratot, az auditot vagy az agent által ellenőrzött felhatalmazást. Ezek nélkül az eredmény nem használható kép, bevitel, UAC vagy háttérparancs indítására. Normál kapcsolatban az UAC-jog nem változtatja át a felhasználó Windows-fiókját rendszergazdává.

### Belső adminisztráció és tesztek

A `set_membership`, `create_role`, `assign_role`, `override_permission`, `remove_assignment` és `remove_override` megbízható belső beállítási műveletek. Meghívásuk önmagában nem jogosítja fel a hívót. Későbbi HTTP-bekötésük előtt megfelelő adminisztratív engedély, friss hitelesítés, CSRF-védelem és audit szükséges. Egy tiltás eltávolítása is jogosultságbővítés lehet. Nincs automatikus első adminisztrátor vagy minden ügyfélre kiterjedő megkerülés.

A `just test-database` valódi PostgreSQL-en és valódi szoftveres WebAuthn-aláírással ellenőrzi a szerepeket, egyedi szabályokat, a tiltási elsőbbséget, ügyfél/telephely/csoport/gép szűrést, azonos gépazonosító melletti ügyfélhatárt, csoporttagság változását, letiltott tagságot, lejárt/rossz típusú hitelesítést, visszavont munkamenetet, valamint az UAC/admin/unattended kombinációkat. Külön negatív próba védi a beépített szerepek módosítását, a másik ügyfélhez írást és a kényszerített RLS kikapcsolását.

## English

The internal `midas_server::authorization::Authorizer` evaluates a current browser session, tenant, target device and requested operation. Default access is denied. It does not execute remote actions, issue signed grants or expose public privilege-management endpoints. UI and audited administration integration remain separate work.

### Roles and scope

| Role | Default grants |
| --- | --- |
| Observer | Device and monitoring reads |
| Technician | Observer grants plus normal screen viewing/control |
| Administrator | All catalog permissions within the explicitly assigned tenant and scope |

Administrator never implies cross-tenant membership, bypasses a denial or replaces fresh passkey verification. Built-in roles/permissions are immutable to the runtime database identity; changes require migrations. Custom roles contain allows and denials. Role names are unique case-insensitively, so built-in names cannot be reused. Definitions are global, while assignments are tenant scoped. A tenant's `access.manage` does not automatically authorize changing global definitions; future administration must require distinct platform authority.

Memberships, role assignments and individual overrides use forced tenant RLS. Scope may cover a tenant, site, group or device. Device site/group membership is resolved from actual inventory, never caller assertions. Composite foreign keys preserve tenant ownership of scopes.

Every applicable denial wins over every allow, including narrower device grants and Administrator assignments. A decision without a device covers the whole tenant: it needs a tenant-scope allow and any narrower-scope denial rejects it. `remote.*` permissions always require a concrete device. Disabling a role removes its allows but keeps its denials; remove the assignment to lift them. Missing grants, unknown permissions and wildcards are denied. After the initial session check, decisions read fresh state in a consistent read-only transaction that rechecks the session, batching requested permissions in one query. There is no background policy polling or stale allow cache. Assignment removal, group changes, disabled roles and disabled memberships affect the next decision. Database validation now requires forced RLS on all eight tenant tables.

### Permission catalog

| Identifier | Operation | Fresh passkey |
| --- | --- | --- |
| `device.read` | Inventory reads | No |
| `monitor.read` | Monitoring reads | No |
| `remote.view` | Screen viewing | No |
| `remote.control` | Normal remote input | No |
| `remote.admin` | Administrative support mode | Yes |
| `remote.uac.interact` | Windows UAC in either mode | Yes |
| `remote.unattended` | Unattended server request | Yes |
| `terminal.execute` | Background terminal | Yes |
| `files.read` | Background file reads | No |
| `files.write` | Background file changes | Yes |
| `script.execute` | Script execution | Yes |
| `updates.manage` | Update management | Yes |
| `domain.join` | Domain joining | Yes |
| `access.manage` | Tenant access administration | Yes |
| `audit.read` | Tenant audit reads | No |

Fresh currently means a verified passkey login within five minutes, with a still-valid session and credential version. Password+TOTP and recovery never qualify, including for Administrators. Future request-bound step-up verification and short-lived execution grants remain separate work.

### Remote eligibility

`check_remote_request` requires a concrete device and checks `device.read`, `remote.view` and `remote.control`; administrative mode additionally requires `remote.admin`. UAC separately requires `remote.uac.interact` in either mode and a Windows target. Unattended requests require `remote.unattended`, a server target and both tenant/device `unattended_enabled` flags; both default to false.

The same device and policy conditions apply when `authorize` is called with `remote.uac.interact` or `remote.unattended` alone. The result is only a policy decision. It does not prove absence of a local user or replace authenticated OS-session targeting, consent, reason, expiry, audit or an agent-validated grant. It cannot by itself start capture, input, UAC interaction or commands. Normal-mode UAC permission does not elevate the local user's Windows account.

### Provisioning and verification

`set_membership`, `create_role`, `assign_role`, `override_permission`, `remove_assignment` and `remove_override` are trusted internal provisioning calls. They do not authorize their caller. Public integration requires administrative permission, fresh authentication, CSRF and audit. Removing a denial can increase access. There is no automatically provisioned administrator or cross-tenant bypass.

`just test-database` uses real PostgreSQL and software-generated WebAuthn signatures to verify roles/overrides, denial precedence, tenant/site/group/device scope, identical device IDs across tenants, changing group membership, disabled memberships, expired/wrong-method authentication, revoked sessions and normal/admin/UAC/unattended cases. Negative tests also cover built-in role mutation, cross-tenant writes and missing forced RLS.
