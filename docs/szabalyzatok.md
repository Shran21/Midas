# Öröklődő szabályzatok / Inherited policies

[English](#english)

## Magyar

2026. november 8. · MIDAS-028 · UI-01, SEC-01

### Öröklés

Egy beállítás értéke ebben a sorrendben dől el; a későbbi szint felülírja a korábbit:

1. beépített alapérték;
2. ügyfél (tenant);
3. az eszköz telephelye;
4. az eszköz csoportjai, növekvő prioritás szerint. Azonos prioritásnál a név, majd az azonosító dönt, így ha egy eszköz több csoport tagja, a legmagasabb prioritású csoport értéke érvényes.

A csoportszintű felülírás csak a csoport tagjaira hat. A felülírás törlésével a hatókör ismét örököl. Az eszköz érvényes beállításai mellett mindig látszik az eredet: az alapérték, illetve a szint (`tenant`, `site`, `group`), annak azonosítója és neve.

### Beállítások

| Kulcs | Tartomány | Alapérték | Hatás |
| --- | --- | --- | --- |
| `health.low_space_percent` | 1–50 | 10 | E szabad hely (%) alatt figyelmeztet a tárolóellenőrzés |
| `health.critical_space_percent` | 1–50 | 5 | E szabad hely (%) alatt kritikus; legfeljebb az előző érték |
| `health.update_warning_days` | 1–365 | 35 | Ennyi nap után figyelmeztet a frissítési ellenőrzés |
| `health.update_critical_days` | 1–365 | 60 | Ennyi nap után kritikus; legalább az előző érték |
| `health.signature_warning_days` | 1–90 | 3 | A Defender-definíciók ennyi napos kora után figyelmeztet |
| `health.signature_critical_days` | 1–90 | 7 | Ennyi nap után kritikus; legalább az előző érték |

A szerver tartományon kívüli vagy ismeretlen kulcsot nem fogad el. Ha az öröklés ellentmondó párt ad (például a kritikus határ magasabb a figyelmeztetőnél), az agent a kritikus határt a figyelmeztetőhöz igazítja. A katalógus a megosztott protokollcsomagban van, így a szerver pontosan azt ellenőrzi, amit az agent ért.

### Átadás az agentnek

- A szerver kapcsolódáskor elküldi az eszköz érvényes beállításait (`PolicyAssignment`), később pedig akkor, ha egy érték vagy a csoporttagság miatt megváltoztak.
- Az agent az új küszöbökkel azonnal újrafuttatja az ellenőrzéseket. A szabályzatot nem tárolja; újraindulás után a következő kapcsolódásig az alapértékekkel dolgozik.

### Kezelés

- `GET /api/tenants/{ügyfél}/policies` (`policy.manage`): a katalógus és az összes tárolt felülírás.
- `PUT /api/tenants/{ügyfél}/policies/{szint}/{hatókör}/{kulcs}` törzzsel `{"value": 25}`, illetve `DELETE` (`policy.manage`, friss passkey). A szint `tenant`, `site` vagy `group`; ügyfélszinten a hatókör az ügyfél azonosítója. Más ügyfél telephelye vagy csoportja nem található.
- `GET /api/tenants/{ügyfél}/devices/{eszköz}/policy` (`device.read`): az érvényes értékek az alapértékkel és az eredettel.
- Csoportok: `GET` és `POST /api/tenants/{ügyfél}/groups` (`{"name": "Szerverek", "priority": 10}`, prioritás −1000 és 1000 között), tagság: `PUT` és `DELETE …/groups/{csoport}/devices/{eszköz}`. A csoporttagság a szerepek hatókörét is meghatározza, ezért `access.manage` és friss passkey kell hozzá.
- Minden módosítás auditált: `policy.set`, `policy.clear`, `inventory.group_create`, `inventory.group_member_add`, `inventory.group_member_remove`.

### Ellenőrzés

- A szerver egységtesztje a rétegek sorrendjét, az eredetet és a tartományba szorítást vizsgálja. Az agent-core tesztjei a kulcsok kezelését, az ellentmondó párok igazítását és az azonnali újrafuttatást. A Windows-tesztek a frissítési és definíciós ítélet változását mutatják.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel és mTLS-sel igazolja a következőket:
  - a csoport felülírása csak a tag eszközt éri el, a csoporton kívüli eszköz az ügyfélértéket kapja;
  - két csoport közül a magasabb prioritású nyer;
  - az eredet szintje és neve látszik;
  - érvénytelen érték, ismeretlen kulcs és idegen hatókör elutasítva, idegen ügyfél kezelője 404-et kap;
  - a futó agent megkapja az új küszöböt, és a felülírás törlése vagy a csoporttagság megszüntetése után a következő érvényes értéket;
  - minden módosítás auditbejegyzést kap.

### Ismert korlátok

- A szabályzatkezelés ügyfélszintű jogot kér; telephelyre vagy csoportra szűkített szabályzatkezelő szerep még nincs.
- Csoport átnevezése, prioritásának módosítása és törlése, valamint telephely törlése még nem érhető el.
- A webpanel szabályzatnézete és az eszközoldal (MIDAS-031) még hátravan; addig az API mutatja az eredetet.
- Ez a kör az állapotellenőrzési küszöböket tartalmazza; a riasztási, karbantartási és újraindítási beállítások a MIDAS-029 és a MIDAS-063 feladattal bővítik a katalógust.

## English

November 8, 2026 · MIDAS-028 · UI-01, SEC-01

### Inheritance

A setting's value is decided in this order, each level overriding the earlier ones:

1. the built-in default;
2. the customer (tenant);
3. the device's site;
4. the device's groups in rising priority. Equal priorities are ordered by name, then id, so a device in several groups takes the value of its highest-priority group.

A group override reaches only the group's members. Removing an override makes the scope inherit again. The device's effective settings always show their source: the default, or the level (`tenant`, `site`, `group`) with its id and name.

### Settings

| Key | Range | Default | Effect |
| --- | --- | --- | --- |
| `health.low_space_percent` | 1–50 | 10 | Below this free space (%) the storage check warns |
| `health.critical_space_percent` | 1–50 | 5 | Below this free space (%) it is critical; at most the previous value |
| `health.update_warning_days` | 1–365 | 35 | After this many days the update check warns |
| `health.update_critical_days` | 1–365 | 60 | After this many days it is critical; at least the previous value |
| `health.signature_warning_days` | 1–90 | 3 | Defender security intelligence this many days old warns |
| `health.signature_critical_days` | 1–90 | 7 | After this many days it is critical; at least the previous value |

The server refuses out-of-range values and unknown keys. If inheritance yields a contradictory pair (for example a critical limit above the warning limit), the agent aligns the critical limit with the warning limit. The catalogue lives in the shared protocol crate, so the server validates exactly what agents understand.

### Delivery to the agent

- The server sends the device's effective settings (`PolicyAssignment`) on connecting, and again whenever a value or a group membership changes them.
- The agent re-runs its checks with the new limits at once. It does not store the policy; after a restart it uses the defaults until the next connection.

### Administration

- `GET /api/tenants/{tenant}/policies` (`policy.manage`): the catalogue and every stored override.
- `PUT /api/tenants/{tenant}/policies/{level}/{scope}/{key}` with body `{"value": 25}`, and `DELETE` (`policy.manage`, fresh passkey). The level is `tenant`, `site` or `group`; at tenant level the scope is the tenant id. Another customer's site or group is not found.
- `GET /api/tenants/{tenant}/devices/{device}/policy` (`device.read`): effective values with their default and source.
- Groups: `GET` and `POST /api/tenants/{tenant}/groups` (`{"name": "Servers", "priority": 10}`, priority between −1000 and 1000); membership: `PUT` and `DELETE …/groups/{group}/devices/{device}`. Group membership also scopes roles, so it needs `access.manage` and a fresh passkey.
- Every change is audited: `policy.set`, `policy.clear`, `inventory.group_create`, `inventory.group_member_add`, `inventory.group_member_remove`.

### Verification

- A server unit test covers layer order, source and clamping. agent-core tests cover key handling, aligning contradictory pairs and the immediate re-run. Windows tests show the update and security-intelligence verdicts changing.
- The database suite, with real PostgreSQL and mTLS, verifies the following:
  - a group override reaches only the member device, and a device outside the group gets the tenant value;
  - of two groups the higher priority wins;
  - the source level and name are shown;
  - invalid values, unknown keys and foreign scopes are refused, and another customer's operator gets 404;
  - the running agent receives the new limit, and after clearing the override or removing the membership the next effective one;
  - every change is audited.

### Known limits

- Policy administration needs a tenant-wide permission; there is no site- or group-scoped policy administrator role yet.
- Renaming groups, changing their priority, deleting groups and deleting sites are not available yet.
- The web panel's policy view and the device page (MIDAS-031) are still to come; until then the API shows the source.
- This round covers health check thresholds; MIDAS-029 and MIDAS-063 extend the catalogue with alerting, maintenance and restart settings.
