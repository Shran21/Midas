# Felügyelet nélküli szerveres hozzáférés / Unattended server access

[English](#english)

## Magyar

A MIDAS-041 első megvalósítása a szerveres szabályzat tárolását, adminisztrációs API-ját és belső döntési szolgáltatását tartalmazza. Távoli kép- vagy beviteli kapcsolatot még nem indít. A webpanel szabályzatszerkesztője, az agentoldali munkamenet-felderítés bekötése, az aláírt, egyszer használható felhatalmazás és a tényleges helyi jóváhagyás még szükséges; a feladat ezért nem lezárt.

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

### Munkamenet-döntés

A belső `Authorizer::check_unattended_request` megköveteli mindkét kapcsolót, a szerverprofilt, a megtekintési és vezérlési jogokat, a külön unattended jogot, öt percen belüli passkey-hitelesítést, az indoklást és a kijelölt munkamenetet. A rendszergazdai mód és az UAC további külön jogosultságot kér. A jelenlegi jogosultságokat minden döntésnél újraolvassa.

Az indoklás nem lehet üres, legfeljebb 512 UTF-8 bájtos lehet, és nem tartalmazhat vezérlőkaraktert. Ez a célmunkamenet adataival együtt az auditbejegyzés mérethatárán belül marad.

A jelenlétet csak az igazolt agentcsatorna megfigyelése adhatja; a böngészőből nem fogad jelenlétállítást. A megfigyelés az ügyfélhez, géphez, agentkapcsolathoz és az operációs rendszer munkamenetének egyedi generációjához kötött. Harminc másodpercnél régebbi vagy jövőbeli megfigyelés, ismeretlen jelenlét, visszavont kapcsolat és eltérő kijelölés nem adhat pozitív döntést. A döntés újraellenőrzi a konkrét agenttanúsítvány lejáratát és visszavonását is. Újracsatlakozáskor új kapcsolat-azonosító keletkezik.

A foglalt cél `LocalConsentRequired` eredményt ad, amiből nem indulhat unattended kapcsolat. A natív megfigyelőnek a zárolt és a leválasztott, de bejelentkezett felhasználó munkamenetét is foglaltnak kell jelentenie. Üres cél esetén az aktuális karbantartási ablak is kötelező. Az `Eligible` eredmény legfeljebb 30 másodpercig, de az ablak, a böngészős munkamenet és a friss passkey határidejénél nem tovább érvényes. A kérés eredménye, módja, UAC-igénye, érvényes indoklása, kijelölt célja és lejárata auditálható.

Ez szabályzatdöntés, nem végrehajtható felhatalmazás. A későbbi aláírt felhatalmazás kiadásánál és felhasználásánál újra kell ellenőrizni a jogosultságot, szabályzatot, célmunkamenetet, lejáratot és visszavonást. Nincs HTTP-végpont, amely böngésző által megadott „üres munkamenet” alapján kapcsolatot indítana.

## English

The first MIDAS-041 implementation provides server-policy storage, an administration API and an internal decision service. It does not start remote media or input. The web-panel policy editor, native agent session discovery, signed single-use grants and actual local consent still need integration; the task remains open.

### Configuration

Customer and individual server opt-ins are separate and default to off. Sites and groups cannot enable access on their behalf. Enabling a server requires an explicit maintenance window; enabling the customer does not change the device switch. The server profile always requires reboot confirmation and subsequent connectivity verification. The reboot workflow must enforce these requirements when implemented.

| Endpoint | Read | Change |
| --- | --- | --- |
| `GET/PUT /api/tenants/{id}/unattended-policy` | Tenant-wide `access.manage` | The same permission and a fresh passkey; enabling also requires tenant-wide `remote.unattended` |
| `GET/PUT /api/tenants/{id}/devices/{device}/unattended-policy` | `device.read` on the server | `access.manage` on the server and a fresh passkey; enabling also requires `remote.unattended` on the server |

The server checks the actor's held permission independently of the opt-in being edited. Applicable denials take precedence. Disabling does not require unattended permission but still requires access management and a fresh passkey. Writes enforce CSRF protection. Authorization, the change and its audit entry share a consistent database transaction; audit failure rolls back the change.

The tenant body is `{"enabled":true}`. The device body has `enabled` and `maintenance_window` with `starts_at` and `ends_at`, as illustrated above. Timestamps are UTC Unix seconds. The start is inclusive and the end exclusive; enabling requires an end in the future. Windows are currently one-off, without recurrence. `within_maintenance_window` describes time eligibility only, not authorization to execute an operation.

### Session decision

The internal `Authorizer::check_unattended_request` requires both opt-ins, a server device, viewing and control permissions, dedicated unattended permission, passkey authentication within five minutes, a reason and explicit session selection. Administrative mode and UAC require their additional permissions. Every decision reads current authorization.

The reason must be nonblank, at most 512 UTF-8 bytes, and contain no control characters. This keeps it and the target-session metadata within the audit record's size limit.

Presence comes from the verified agent channel, never a browser assertion. The observation binds customer, device, agent connection and the OS session's unique generation. Observations at least 30 seconds old or in the future, unknown presence, revoked connections and mismatched selections cannot authorize access. Each decision also rechecks the specific agent certificate's expiry and revocation. Reconnection creates a new connection identifier.

An occupied target returns `LocalConsentRequired`, which cannot start an unattended session. The native observer must also report locked and disconnected sessions with signed-in users as occupied. A vacant target additionally requires the current maintenance window. `Eligible` expires within 30 seconds and no later than the maintenance window, browser session or fresh-passkey deadline. The decision, mode, UAC request, valid reason, selected target and expiry are audited.

This is a policy decision, not an executable grant. Issuing and consuming the later signed grant must revalidate permissions, policy, target session, expiry and revocation. There is no HTTP endpoint that starts a connection from a browser-supplied claim of a vacant session.
