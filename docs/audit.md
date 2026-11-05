# Audit / Audit

[English](#english)

## Magyar

A `midas_server::audit` csak hozzáfűzhető alapauditot ad. Két külön lánc van: az ügyfélhez kötött `tenant_audit` és a `platform_audit`. Az utóbbi a globális technikusidentitás eseményeit és az ügyfélhez nem köthető eseményeket tárolja, például a belépést vagy egy nem létező ügyfélre szóló kérést. A kezelőfelület, a külső ellenőrzőpont és a megőrzési szabályok külön feladatok.

### Rögzített események

| Művelet | Lánc | Tartalom |
| --- | --- | --- |
| `auth.sign_in` | platform | Sikeres belépés munkamenettel, illetve hibás hitelesítő adat; mód és forráscím. A technikus csak létező fióknál szerepel. |
| `auth.sign_out`, `session.revoke` | platform | Kijelentkezés és saját munkamenet visszavonása |
| `access.role_create` | platform | Globális szerepdefiníció neve és szabályai |
| `access.membership_set` | ügyfél | Ügyféltagság engedélyezése vagy letiltása |
| `access.role_assign`, `access.role_unassign` | ügyfél | Szerephozzárendelés és hatóköre |
| `access.override_set`, `access.override_remove` | ügyfél | Egyedi engedély vagy tiltás és hatóköre |
| `remote.request` | ügyfél vagy platform | Távvezérlési kérés jogosultsági döntése: mód, UAC, unattended, cél és eredmény |

Minden bejegyzés tartalmazza a végrehajtót, ha van: technikus és munkamenet. A megbízható belső beállítás `Actor::SYSTEM` jelöléssel kerül be. A jogosultságváltozás és az auditbejegyzés ugyanabban a tranzakcióban jön létre; sikertelen auditírásnál a változás sem marad meg. Engedélyező távvezérlési döntést vagy új belépési munkamenetet a szerver nem ad vissza, ha az audit nem íródott be; a munkamenetet ilyenkor visszavonja.

A hibás formátumú és a próbálkozáskorlát miatt elutasított kéréseket nem rögzítjük egyenként, így a napló nem nőhet korlátlanul. Az egyszerű olvasási jogosultság-ellenőrzések sem kerülnek auditba. A passkey-belépés hibájánál a technikus azonosítója nem ismert, ezért csak a mód és a forrás szerepel.

### Titkok kizárása

Az eseményeket a szerver csak típusos belső értékekből állítja össze, mérethatárral. Nem kerül auditba jelszó, TOTP-kód, helyreállító kód, munkamenet-azonosító süti, CSRF-érték, passkey-válasz vagy nem létező fióknál a beküldött belépési név. A munkamenet nyilvános UUID-je nem hitelesítő adat.

### Hozzáfűzés és integritás

A futtatófiók csak olvashat és meghatározott oszlopokba szúrhat be. Módosítást, törlést és csonkolást a jogosultságok és adatbázis-triggerek is tiltanak. Az adatbázis-kapcsolat ellenőrzése elutasítja azt a futtatófiókot, amely módosíthatná vagy törölhetné az auditot, illetve ha az ügyfélaudit RLS-e nincs kényszerítve.

Az adatbázis-trigger láncon belüli tranzakciós zár alatt adja a sorszámot, az időbélyeget, az előző hash-t és a SHA-256 hash-t. A hívó ezeket nem adhatja meg. A `verify_tenant` és `verify_platform` újraszámolja a láncot, jelzi az első hibás bejegyzést, és visszaadja a lánc hosszát és utolsó hash-ét. Így kimutatható a közbenső átírás és törlés. A lánc végének csonkolását és egy rendszergazda által újraszámolt láncot csak a külső, elkülönített jogosultságú ellenőrzőpont mutathatja ki; ez a MIDAS-079 feladata.

### Keresés

A `search_tenant` és a `verify_tenant` az adott ügyfélre teljes körű `audit.read` jogot és érvényes munkamenetet kér. Szűrhető műveletre, eredményre, végrehajtóra, érintett technikusra, gépre, időszakra, és sorszám szerint lapozható; legfeljebb 500 találatot ad. A platformaudit keresése és ellenőrzése belső, megbízható hívás, amíg nincs külön platformszintű jogosultság.

### Ellenőrzés

A `just test-database` valódi PostgreSQL-en ellenőrzi a belépések, kijelentkezés, jogosultságváltozások és távvezérlési döntések kereshetőségét, a titkok hiányát, az RLS-t, a tiltott módosítást, törlést és csonkolást, a párhuzamos hozzáfűzést, a hibás auditírás miatti elutasítást, valamint az átírás és törlés kimutatását.

## English

`midas_server::audit` provides an append-only baseline audit with two chains: tenant-bound `tenant_audit` and `platform_audit`. The platform chain stores global operator identity events and events that cannot be attributed to a tenant, such as sign-ins or a request for a non-existent tenant. UI, external checkpoints and retention rules are separate work.

### Recorded events

| Action | Chain | Content |
| --- | --- | --- |
| `auth.sign_in` | platform | Successful sign-in with session, or invalid credentials; method and source. The operator is recorded only for an existing account. |
| `auth.sign_out`, `session.revoke` | platform | Sign-out and revocation of an own session |
| `access.role_create` | platform | Global role definition name and rules |
| `access.membership_set` | tenant | Tenant membership enabled or disabled |
| `access.role_assign`, `access.role_unassign` | tenant | Role assignment and its scope |
| `access.override_set`, `access.override_remove` | tenant | Individual allow or deny and its scope |
| `remote.request` | tenant or platform | Remote request eligibility decision: mode, UAC, unattended, target and result |

Each record includes the acting operator and session where applicable. Trusted internal provisioning records `Actor::SYSTEM`. Permission changes and their audit records share one transaction, so a failed audit write rolls back the change. The server never returns an allowing remote decision or a new sign-in session whose audit write failed; such a session is revoked.

Malformed and rate-limited requests are not recorded individually so audit growth stays bounded. Plain read authorization checks are not audited. A failed passkey sign-in does not reveal the operator, so only method and source are recorded.

### Excluding secrets

Events are built only from typed internal values with a size limit. Passwords, TOTP codes, recovery codes, session cookies, CSRF values, passkey responses and submitted login names of non-existent accounts are never stored. The public session UUID is not a credential.

### Append-only storage and integrity

The runtime role can only read and insert specific columns. Grants and database triggers block updates, deletes and truncation. Database connection validation rejects a runtime role that could update or delete audit records, or a tenant audit without forced RLS.

A database trigger assigns sequence, timestamp, previous hash and SHA-256 hash under a per-chain transaction lock; callers cannot supply them. `verify_tenant` and `verify_platform` recompute the chain, report the first invalid record and return chain length and head hash. This detects rewriting and deletion inside the chain. Tail truncation and a chain recomputed by a database administrator can only be detected with the external checkpoint under separate access control planned in MIDAS-079.

### Search

`search_tenant` and `verify_tenant` require a valid session and tenant-wide `audit.read`. Filters cover action, outcome, actor, subject operator, device and time range, with sequence-based paging and at most 500 results. Platform audit search and verification are trusted internal calls until platform-level authority exists.

### Verification

`just test-database` uses real PostgreSQL to verify searchable sign-ins, sign-outs, permission changes and remote decisions, absence of secrets, RLS, refused update/delete/truncate, concurrent appends, refusal when the audit write fails, and detection of rewriting and deletion.
