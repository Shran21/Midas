# Riasztások és értesítések / Alerts and notifications

[English](#english)

## Magyar

2026. november 8. · MIDAS-029 · UI-07

### Mikor nyílik riasztás

| Fajta | Súlyosság | Nyitás | Zárás |
| --- | --- | --- | --- |
| `device.offline` | kritikus | Az aktív eszköz a szabályzatban megadott ideje (`alerts.offline_minutes`, alapból 5 perc) nem jelentkezett. Csak már egyszer kapcsolódott eszközre vonatkozik. | Az eszköz újra online, vagy már nem aktív (visszavonták). |
| `health.<ellenőrzés>` | az ellenőrzés állapota szerint figyelmeztetés vagy kritikus | Egy állapotellenőrzés `warning` vagy `critical` eredményt ad ([állapotellenőrzések](allapotellenorzes.md)). | Az ellenőrzés `ok` vagy `unavailable` lesz, vagy kimarad a jelentésből. |

Az offline-söprés a szerveren 30 másodpercenként fut, a jelenlétet jelző söprés után.

### Összevonás

- Eszközönként és fajtánként egyszerre egy nyitott riasztás lehet; ezt adatbázis-index is kikényszeríti. Ismétlődéskor a meglévő riasztás frissül: az utolsó észlelés ideje, a súlyosság és a leírás.
- Ha a riasztás a megszűnése után 10 percen belül újra jelentkezik, ugyanaz a riasztás nyílik újra, és nő az előfordulások száma. Így egy villogó kapcsolat egy riasztás marad, új értesítés nélkül.
- Értesítés csak új riasztásnál készül, címzettenként egy.

### E-mail

- Címzettek: `GET` és `PUT /api/tenants/{ügyfél}/alert-recipients` törzzsel `{"emails": ["ops@example.com"]}` (`policy.manage`, a módosításhoz friss passkey). Legfeljebb 20 cím, szigorú formai ellenőrzéssel. Az audit csak a címek számát rögzíti, magukat a címeket nem.
- A kimenő levelek sorba kerülnek. A szerver 30 másodpercenként egyszerre legfeljebb 20-at küld, és küldés előtt öt percre lefoglalja őket, így összeomlás után sem vesznek el.
- Átmeneti hibánál a küldés 1, 5, 15, 60 és 240 perc múlva ismétlődik; hat sikertelen próba vagy végleges (5xx) elutasítás után a levél `failed` állapotú lesz. A riasztáslista riasztásonként mutatja az elküldött, a várakozó és a sikertelen értesítések számát.
- A levél magyarul és angolul nevezi meg az ügyfelet, az eszközt, a riasztás fajtáját, súlyosságát, leírását és kezdetét (UTC). Hozzáférési adatot vagy hivatkozást nem tartalmaz.
- SMTP-beállítások: `--smtp-host`, `--smtp-port` (alapból 587), `--smtp-security` (`starttls` az alapértelmezés, `tls`, vagy `none`), `--smtp-from`, valamint választhatóan `--smtp-user` és `--smtp-password-file`. Mindegyik `MIDAS_SMTP_*` környezeti változóval is megadható.
  - A `none` csak ugyanazon a gépen futó továbbítóhoz használható, és jelszóval nem.
  - A jelszó csak fájlból olvasható, parancssorban nem adható meg.
  - Az SMTP-kiszolgáló tanúsítványát a beépített Mozilla-gyökérlista ellenőrzi.
  - SMTP nélkül a szerver figyelmeztet, a levelek pedig a sorban várnak.

### Kezelés

- `GET /api/tenants/{ügyfél}/alerts`: a legutóbbi 200 riasztás. `GET …/alerts/open`: a nyitottak. Mindkettőhöz ügyfélszintű `monitor.read` kell.
- `POST /api/tenants/{ügyfél}/alerts/{riasztás}/acknowledge` (`alert.acknowledge` az eszközre): a riasztás egyszer nyugtázható, a második kísérletre 409 a válasz. A nyugtázás auditált (`alert.acknowledge`, a riasztással és az eszközzel), a nyugtázó és az időpont a riasztásnál látszik. A nyugtázás nem zárja le a riasztást; az a feltétel megszűnésekor záródik.
- Az `alert.acknowledge` jog az Administrator és a Technician szerepé.

### Ellenőrzés

- Egységtesztek: a címek ellenőrzése, a visszalépéses újrapróbálás, a levél tárgya és az érvénytelen címzett elutasítása.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel, mTLS-sel és helyi SMTP-fogadóval igazolja a következőket:
  - az agent jelentette kritikus és figyelmeztető ellenőrzések riasztást nyitnak, majd maguktól záródnak;
  - 30 perces szabályzati késleltetésnél a 10 perce offline eszközre nem nyílik riasztás, alapértéken igen;
  - az ismételt söprés nem nyit új riasztást;
  - egy kiesésre pontosan egy e-mail megy a valódi SMTP-kliensen át, az újabb kézbesítés nem küld többet;
  - visszatéréskor a riasztás záródik, gyors újbóli kiesésnél ugyanaz nyílik újra két előfordulással, új e-mail nélkül;
  - a nyugtázás egyszer sikerül és auditált, más ügyfél kezelője sem a listát, sem a nyugtázást nem éri el.

### Ismert korlátok

- Csak e-mail-csatorna van; a Teams- és Slack-végpontok a MIDAS-030 részei.
- Riasztásonként egy értesítés készül; eszkalálás, ismétlő emlékeztető és csendes időszak még nincs.
- A címzettek ügyfélszintűek; telephelyre vagy csoportra szűkített címzettlista még nincs.
- A webpanel Riasztások nézete és az eszközoldal mutatja és nyugtázza a riasztásokat ([eszközoldal](eszkozoldal.md)).

## English

November 8, 2026 · MIDAS-029 · UI-07

### When an alert opens

| Kind | Severity | Opens | Resolves |
| --- | --- | --- | --- |
| `device.offline` | critical | An active device has not been seen for the policy delay (`alerts.offline_minutes`, 5 minutes by default). Only devices that have connected at least once. | The device is online again or no longer active (revoked). |
| `health.<check>` | warning or critical, from the check state | A health check reports `warning` or `critical` ([health checks](allapotellenorzes.md#english)). | The check becomes `ok` or `unavailable`, or is missing from the report. |

The offline sweep runs on the server every 30 seconds, after the presence sweep.

### Deduplication

- A device has at most one open alert per kind, enforced by a database index. A repeat updates the open alert: last seen time, severity and summary.
- If the condition returns within 10 minutes of resolving, the same alert reopens and its occurrence count grows. A flapping connection therefore stays one alert, without new notifications.
- Notifications are created only for a new alert, one per recipient.

### E-mail

- Recipients: `GET` and `PUT /api/tenants/{tenant}/alert-recipients` with body `{"emails": ["ops@example.com"]}` (`policy.manage`, fresh passkey for changes). At most 20 addresses, strictly validated. The audit keeps only their count, not the addresses.
- Outgoing messages are queued. Every 30 seconds the server sends up to 20 at a time and leases them for five minutes before sending, so a crash does not lose them.
- Transient failures retry after 1, 5, 15, 60 and 240 minutes; after six failed attempts or a permanent (5xx) refusal the message becomes `failed`. The alert list shows the number of sent, pending and failed notifications per alert.
- The message names the customer, device, alert kind, severity, summary and start time (UTC) in Hungarian and English. It contains no credentials or links.
- SMTP settings: `--smtp-host`, `--smtp-port` (587 by default), `--smtp-security` (`starttls` by default, `tls`, or `none`), `--smtp-from`, and optionally `--smtp-user` and `--smtp-password-file`. Each also has a `MIDAS_SMTP_*` environment variable.
  - `none` is accepted only for a relay on the same machine, and never with a password.
  - The password is read from a file only, never from the command line.
  - The SMTP server certificate is checked against the built-in Mozilla root list.
  - Without SMTP the server warns and messages wait in the queue.

### Handling

- `GET /api/tenants/{tenant}/alerts`: the latest 200 alerts. `GET …/alerts/open`: the open ones. Both need tenant-wide `monitor.read`.
- `POST /api/tenants/{tenant}/alerts/{alert}/acknowledge` (`alert.acknowledge` for the device): an alert can be acknowledged once; a second attempt answers 409. Acknowledgement is audited (`alert.acknowledge`, with the alert and device), and the acknowledging operator and time show on the alert. Acknowledging does not resolve the alert; it resolves when the condition clears.
- `alert.acknowledge` belongs to the Administrator and Technician roles.

### Verification

- Unit tests cover address validation, retry backoff, the message subject and refusing an invalid recipient.
- The database suite, with real PostgreSQL, mTLS and a local SMTP receiver, verifies the following:
  - critical and warning checks reported by the agent open alerts that later resolve on their own;
  - with a 30-minute policy delay a device offline for 10 minutes gets no alert, with the default it does;
  - repeated sweeps open nothing new;
  - one outage sends exactly one e-mail through the real SMTP client, and further deliveries send no more;
  - coming back resolves the alert, and a quick new outage reopens the same alert with two occurrences and no new e-mail;
  - acknowledgement succeeds once and is audited, and another customer's operator reaches neither the list nor the acknowledgement.

### Known limits

- E-mail is the only channel; Teams and Slack endpoints belong to MIDAS-030.
- One notification per alert; escalation, reminders and quiet hours are not available yet.
- Recipients are per customer; site- or group-scoped recipient lists are not available yet.
- The web panel Alerts view and the device page show and acknowledge alerts ([device pages](eszkozoldal.md#english)).
