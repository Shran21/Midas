# Jogosultságkezelés / Authorization

[English](#english)

## Magyar

A `midas_server::authorization::Authorizer` belső szolgáltatás aktuális böngészős munkamenet, ügyfél, célgép és kért művelet alapján dönt. Alapállapotban minden tiltott. A modul még nem indít távoli műveletet és nem állít ki aláírt felhatalmazást. A jogosultságváltozások és a távvezérlési döntések [auditba](audit.md) kerülnek. Az adminisztrációs API és a webpanel hozzáférés-kezelése az alábbiak szerint működik; a [szerveres unattended szabályzat](felugyelet-nelkuli-hozzaferes.md) külön leírja az új konfigurációs és munkamenet-döntési réteget.

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

A munkamenet első ellenőrzése után a döntés új adatbázis-állapotból, egy következetes, csak olvasható tranzakcióban készül, amely a munkamenetet is újraellenőrzi. A kért engedélyeket közös lekérdezés értékeli ki; nincs háttérben futó jogosultság-polling vagy elavuló engedélycache. Egy hozzárendelés törlése, csoporttagság változása, szerep kikapcsolása vagy ügyféltagság letiltása a következő döntésben érvényesül. Az adatbázis-kapcsolat ellenőrzése minden ügyfélhez kötött tábla kényszerített RLS-ét megköveteli.

### Ügyfélkörnyezet

A `tenants` a munkamenet technikusának engedélyezett tagságait listázza. Ehhez egy tranzakción belüli technikuskörnyezet csak olvasásra látja a saját tagságait és azok ügyfeleinek nevét; más ügyféladathoz, íráshoz vagy más technikus tagságához nem ad hozzáférést. A környezetet csak ellenőrzött munkamenet állítja be, és a kapcsolat visszaadásakor törlődik.

A `tenant_context` az ügyfél nevét és azokat az engedélyeket adja vissza, amelyekre bármely hatókörben van megadás, és nincs ügyfélszintű tiltás. Ez csak a navigációt igazítja; minden művelet saját, aktuális jogosultság-ellenőrzést kap. Nem tag, letiltott tag és ismeretlen ügyfél ugyanazt a választ kapja.

### Adminisztráció

A platformadminisztrátor a globális technikusidentitás külön jelzője; csak megbízható beállítás (később az első beállítás) adhatja meg, önkiszolgáló módon nem szerezhető. Új ügyfelet hozhat létre, amelynek ő lesz engedélyezett tagja és teljes körű Administratorja, valamint új technikusfiókot, egyszeri kezdeti helyreállító kódokkal. A kódokat csak a létrehozási válasz tartalmazza; az adatbázis csak lenyomatot tárol, az audit pedig titok nélkül rögzíti a létrehozást.

Az ügyfélszintű `access.manage` joggal rendelkező kezelő a tagokat listázhatja, létező technikust pontos felhasználónévvel vehet fel, tagságot engedélyezhet vagy letilthat, szerepet és egyedi engedélyt vagy tiltást rendelhet ügyfél-, telephely-, csoport- vagy géphatókörrel, és telephelyet hozhat létre. Minden változáshoz öt percen belüli passkey-belépés, CSRF-token és ugyanabban a tranzakcióban írt auditbejegyzés kell. A beállítások megtekintéséhez nem kell friss passkey.

Jogosultság nem bővíthető a kezelő saját köre fölé: szerep vagy engedély adásához, tiltás eltávolításához és korábbi jogokkal rendelkező tag újraengedélyezéséhez a kezelőnek az érintett engedélyekkel az egész ügyfélben rendelkeznie kell; ellenkező esetben `escalation_denied` a válasz. Tiltás hozzáadása és jog elvétele mindig megengedett. Nem tag, ismeretlen ügyfél, ismeretlen felhasználónév és nem létező elem egységesen `not_found`. A globális szerepdefiníciók továbbra is csak belső beállítással módosíthatók.

| Végpont | Jogosultság |
| --- | --- |
| `POST /api/admin/tenants`, `POST /api/admin/operators` | Platformadminisztrátor, friss passkey |
| `GET /api/roles` | Bejelentkezett technikus |
| `GET /api/tenants/{id}/members`, `GET /api/tenants/{id}/sites` | Ügyfélszintű `access.manage` |
| `POST /api/tenants/{id}/members`, `PUT /api/tenants/{id}/members/{operator}` | `access.manage`, friss passkey |
| `POST /api/tenants/{id}/assignments`, `DELETE …/assignments/{id}` | `access.manage`, friss passkey, eszkalációvédelem |
| `POST /api/tenants/{id}/overrides`, `DELETE …/overrides/{id}` | `access.manage`, friss passkey, eszkalációvédelem |
| `POST /api/tenants/{id}/sites` | `access.manage`, friss passkey |
| `POST /api/tenants/{id}/enrollment-tokens`, `POST …/devices/{id}/approve`, `…/reject`, `…/revoke` | `device.enroll`, friss passkey ([beiratkozás](beiratkozas.md)) |
| `GET /api/tenants/{id}/devices/pending`, `GET …/devices/decisions` | Ügyfélszintű `device.enroll` |
| `PUT /api/tenants/{id}/directory-connector` | `directory.discover`, friss passkey ([címtár-connector](cimtar-connector.md)) |
| `GET /api/tenants/{id}/directory-connector` | Ügyfélszintű `directory.discover` |
| `GET /api/tenants/{id}/directory/computers` | Ügyfélszintű `directory.discover` (felderített gépek és egyeztetésük) |
| `GET /api/tenants/{id}/devices/{device}/inventory`, `…/health`, `…/policy` | `device.read` az eszközre ([leltár](leltar.md), [állapotellenőrzések](allapotellenorzes.md), [szabályzatok](szabalyzatok.md)) |
| `GET /api/tenants/{id}/devices`, `GET …/devices/{device}` | `device.read` eszközönként; minden eszköz a technikus érvényes jogaival együtt jön ([eszközoldal](eszkozoldal.md)) |
| `GET /api/tenants/{id}/devices/{device}/metrics`, `…/alerts` | `monitor.read` az eszközre |
| `POST /api/tenants/{id}/devices/{device}/refresh` | `monitor.read` az eszközre, kapcsolódó agent (offline gépnél 409 `device_offline`) |
| `GET /api/tenants/{id}/policies` | Ügyfélszintű `policy.manage` |
| `PUT /api/tenants/{id}/policies/{szint}/{hatókör}/{kulcs}`, `DELETE …` | `policy.manage`, friss passkey |
| `GET /api/tenants/{id}/groups` | Ügyfélszintű `access.manage` |
| `POST /api/tenants/{id}/groups`, `PUT …/groups/{csoport}/devices/{eszköz}`, `DELETE …` | `access.manage`, friss passkey (a csoporttagság a szerepek hatókörét is meghatározza) |
| `GET /api/tenants/{id}/alerts`, `GET …/alerts/open` | Ügyfélszintű `monitor.read` ([riasztások](riasztasok.md)) |
| `POST /api/tenants/{id}/alerts/{riasztás}/acknowledge` | `alert.acknowledge` az eszközre |
| `GET /api/tenants/{id}/alert-recipients` | Ügyfélszintű `policy.manage` |
| `PUT /api/tenants/{id}/alert-recipients` | `policy.manage`, friss passkey |
| `GET /api/tenants/{id}/alert-channels` | Ügyfélszintű `policy.manage` (a webhook URL nélkül) |
| `POST /api/tenants/{id}/alert-channels`, `DELETE …/{csatorna}`, `POST …/{csatorna}/test` | `policy.manage`, friss passkey |

A webpanel Hozzáférés nézete a tagokat, szerepeket, szabályokat és telephelyeket kezeli; a platformadminisztrátor Ügyfelek nézete új ügyfelet és technikust hoz létre, a kódokat egyszer mutatja meg. A technikus saját passkeyjének regisztrációja az első beállítási folyamattal együtt készül.

### Engedélyek

| Azonosító | Művelet | Friss passkey |
| --- | --- | --- |
| `device.read` | Eszközadatok megtekintése | Nem |
| `device.enroll` | Agentbeiratkozás: token, jóváhagyás, elutasítás, visszavonás | Igen |
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
| `directory.discover` | Címtár-connector kijelölése és felderítés | Igen |
| `access.manage` | Ügyfélhez tartozó hozzáférés kezelése | Igen |
| `policy.manage` | Ügyfél-, telephely- és csoportszabályzatok kezelése | Igen |
| `alert.acknowledge` | Riasztás nyugtázása | Nem |
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

A `set_membership`, `create_role`, `assign_role`, `override_permission`, `remove_assignment` és `remove_override` megbízható belső beállítási műveletek. Meghívásuk önmagában nem jogosítja fel a hívót. A végrehajtót a hívó adja meg, a változás és az auditbejegyzés egy tranzakcióban jön létre. Későbbi HTTP-bekötésük előtt megfelelő adminisztratív engedély, friss hitelesítés és CSRF-védelem szükséges. Egy tiltás eltávolítása is jogosultságbővítés lehet. Nincs automatikus első adminisztrátor vagy minden ügyfélre kiterjedő megkerülés.

A `just test-database` valódi PostgreSQL-en és valódi szoftveres WebAuthn-aláírással ellenőrzi a szerepeket, egyedi szabályokat, a tiltási elsőbbséget, ügyfél/telephely/csoport/gép szűrést, azonos gépazonosító melletti ügyfélhatárt, csoporttagság változását, letiltott tagságot, lejárt/rossz típusú hitelesítést, visszavont munkamenetet, valamint az UAC/admin/unattended kombinációkat. Külön negatív próba védi a beépített szerepek módosítását, a másik ügyfélhez írást és a kényszerített RLS kikapcsolását.

## English

The internal `midas_server::authorization::Authorizer` evaluates a current browser session, tenant, target device and requested operation. Default access is denied. It does not execute remote actions or issue signed grants. Permission changes and remote decisions are [audited](audit.md#english). The administration API and web-panel access management operate as described below; the [unattended server policy](felugyelet-nelkuli-hozzaferes.md#english) describes the new configuration and session-decision layer separately.

### Roles and scope

| Role | Default grants |
| --- | --- |
| Observer | Device and monitoring reads |
| Technician | Observer grants plus normal screen viewing/control |
| Administrator | All catalog permissions within the explicitly assigned tenant and scope |

Administrator never implies cross-tenant membership, bypasses a denial or replaces fresh passkey verification. Built-in roles/permissions are immutable to the runtime database identity; changes require migrations. Custom roles contain allows and denials. Role names are unique case-insensitively, so built-in names cannot be reused. Definitions are global, while assignments are tenant scoped. A tenant's `access.manage` does not automatically authorize changing global definitions; future administration must require distinct platform authority.

Memberships, role assignments and individual overrides use forced tenant RLS. Scope may cover a tenant, site, group or device. Device site/group membership is resolved from actual inventory, never caller assertions. Composite foreign keys preserve tenant ownership of scopes.

Every applicable denial wins over every allow, including narrower device grants and Administrator assignments. A decision without a device covers the whole tenant: it needs a tenant-scope allow and any narrower-scope denial rejects it. `remote.*` permissions always require a concrete device. Disabling a role removes its allows but keeps its denials; remove the assignment to lift them. Missing grants, unknown permissions and wildcards are denied. After the initial session check, decisions read fresh state in a consistent read-only transaction that rechecks the session, batching requested permissions in one query. There is no background policy polling or stale allow cache. Assignment removal, group changes, disabled roles and disabled memberships affect the next decision. Database validation requires forced RLS on every tenant table.

### Tenant context

`tenants` lists the session operator's enabled memberships. A transaction-local operator context can read only its own memberships and those tenants' names; it grants no other tenant data, writes or other operators' memberships. Only a validated session sets it, and pooled connections clear it.

`tenant_context` returns the tenant name and permissions granted at any scope without a tenant-wide denial. It only adapts navigation; every operation keeps its own current authorization check. Non-members, disabled members and unknown tenants receive the same answer.

### Administration

Platform administrator is a separate flag on the global operator identity. Only trusted provisioning (later the first-run setup) sets it; it cannot be obtained self-service. Platform administrators create tenants, becoming an enabled member and tenant-wide Administrator, and technician accounts with one-time initial recovery codes. Only the creation response contains the codes; the database stores digests and the audit records the creation without secrets.

An operator with tenant-wide `access.manage` can list members, add an existing technician by exact login, enable or disable memberships, assign roles and individual allows or denials at tenant, site, group or device scope, and create sites. Every change requires a passkey sign-in within five minutes, a CSRF token and an audit record in the same transaction. Viewing the configuration does not need a fresh passkey.

Managers cannot exceed their own access: granting a role or allow, removing a denial and re-enabling a member with existing grants require holding every affected permission tenant-wide, otherwise the answer is `escalation_denied`. Adding denials and removing grants are always allowed. Non-members, unknown tenants, unknown logins and missing objects all receive `not_found`. Global role definitions remain internal-only.

| Endpoint | Requirement |
| --- | --- |
| `POST /api/admin/tenants`, `POST /api/admin/operators` | Platform administrator, fresh passkey |
| `GET /api/roles` | Signed-in operator |
| `GET /api/tenants/{id}/members`, `GET /api/tenants/{id}/sites` | Tenant-wide `access.manage` |
| `POST /api/tenants/{id}/members`, `PUT /api/tenants/{id}/members/{operator}` | `access.manage`, fresh passkey |
| `POST /api/tenants/{id}/assignments`, `DELETE …/assignments/{id}` | `access.manage`, fresh passkey, escalation guard |
| `POST /api/tenants/{id}/overrides`, `DELETE …/overrides/{id}` | `access.manage`, fresh passkey, escalation guard |
| `POST /api/tenants/{id}/sites` | `access.manage`, fresh passkey |
| `POST /api/tenants/{id}/enrollment-tokens`, `POST …/devices/{id}/approve`, `…/reject`, `…/revoke` | `device.enroll`, fresh passkey ([enrollment](beiratkozas.md#english)) |
| `GET /api/tenants/{id}/devices/pending`, `GET …/devices/decisions` | Tenant-wide `device.enroll` |
| `PUT /api/tenants/{id}/directory-connector` | `directory.discover`, fresh passkey ([directory connector](cimtar-connector.md#english)) |
| `GET /api/tenants/{id}/directory-connector` | Tenant-wide `directory.discover` |
| `GET /api/tenants/{id}/directory/computers` | Tenant-wide `directory.discover` (discovered computers and their matching) |
| `GET /api/tenants/{id}/devices/{device}/inventory`, `…/health`, `…/policy` | `device.read` for the device ([inventory](leltar.md#english), [health checks](allapotellenorzes.md#english), [policies](szabalyzatok.md#english)) |
| `GET /api/tenants/{id}/devices`, `GET …/devices/{device}` | `device.read` per device; each device comes with the operator's effective permissions ([device pages](eszkozoldal.md#english)) |
| `GET /api/tenants/{id}/devices/{device}/metrics`, `…/alerts` | `monitor.read` for the device |
| `POST /api/tenants/{id}/devices/{device}/refresh` | `monitor.read` for the device, connected agent (409 `device_offline` otherwise) |
| `GET /api/tenants/{id}/policies` | Tenant-wide `policy.manage` |
| `PUT /api/tenants/{id}/policies/{level}/{scope}/{key}`, `DELETE …` | `policy.manage`, fresh passkey |
| `GET /api/tenants/{id}/groups` | Tenant-wide `access.manage` |
| `POST /api/tenants/{id}/groups`, `PUT …/groups/{group}/devices/{device}`, `DELETE …` | `access.manage`, fresh passkey (group membership also scopes roles) |
| `GET /api/tenants/{id}/alerts`, `GET …/alerts/open` | Tenant-wide `monitor.read` ([alerts](riasztasok.md#english)) |
| `POST /api/tenants/{id}/alerts/{alert}/acknowledge` | `alert.acknowledge` for the device |
| `GET /api/tenants/{id}/alert-recipients` | Tenant-wide `policy.manage` |
| `PUT /api/tenants/{id}/alert-recipients` | `policy.manage`, fresh passkey |
| `GET /api/tenants/{id}/alert-channels` | Tenant-wide `policy.manage` (without the webhook URL) |
| `POST /api/tenants/{id}/alert-channels`, `DELETE …/{channel}`, `POST …/{channel}/test` | `policy.manage`, fresh passkey |

The web panel's Access view manages members, roles, rules and sites; the platform administrator's Customers view creates tenants and technicians and shows the codes once. Technicians register their own passkey with the first-run setup flow.

### Permission catalog

| Identifier | Operation | Fresh passkey |
| --- | --- | --- |
| `device.read` | Inventory reads | No |
| `device.enroll` | Agent enrollment: tokens, approval, rejection, revocation | Yes |
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
| `directory.discover` | Directory connector designation and discovery | Yes |
| `access.manage` | Tenant access administration | Yes |
| `policy.manage` | Customer, site and group policy administration | Yes |
| `alert.acknowledge` | Alert acknowledgement | No |
| `audit.read` | Tenant audit reads | No |

Fresh currently means a verified passkey login within five minutes, with a still-valid session and credential version. Password+TOTP and recovery never qualify, including for Administrators. Future request-bound step-up verification and short-lived execution grants remain separate work.

### Remote eligibility

`check_remote_request` requires a concrete device and checks `device.read`, `remote.view` and `remote.control`; administrative mode additionally requires `remote.admin`. UAC separately requires `remote.uac.interact` in either mode and a Windows target. Unattended requests require `remote.unattended`, a server target and both tenant/device `unattended_enabled` flags; both default to false.

The same device and policy conditions apply when `authorize` is called with `remote.uac.interact` or `remote.unattended` alone. The result is only a policy decision. It does not prove absence of a local user or replace authenticated OS-session targeting, consent, reason, expiry, audit or an agent-validated grant. It cannot by itself start capture, input, UAC interaction or commands. Normal-mode UAC permission does not elevate the local user's Windows account.

### Provisioning and verification

`set_membership`, `create_role`, `assign_role`, `override_permission`, `remove_assignment` and `remove_override` are trusted internal provisioning calls. They do not authorize their caller. Callers supply the actor; each change and its audit record share one transaction. Public integration requires administrative permission, fresh authentication and CSRF. Removing a denial can increase access. There is no automatically provisioned administrator or cross-tenant bypass.

`just test-database` uses real PostgreSQL and software-generated WebAuthn signatures to verify roles/overrides, denial precedence, tenant/site/group/device scope, identical device IDs across tenants, changing group membership, disabled memberships, expired/wrong-method authentication, revoked sessions and normal/admin/UAC/unattended cases. Negative tests also cover built-in role mutation, cross-tenant writes and missing forced RLS.
