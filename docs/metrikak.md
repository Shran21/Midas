# Jelenlét és metrikák / Presence and metrics

[English](#english)

## Magyar

2026. november 7. · MIDAS-025 · PERF-02, PERF-05

### Jelenlét

- Az agent keepalive-üzenete (alapértelmezés: 30 másodpercenként) egyben a heartbeat ([agentprotokoll](agent-protokoll.md)). A szerver minden jelnél és a kapcsolat elején frissíti az eszköz `last_seen_at` időpontját, és online-nak jelöli.
- A szerver 30 másodpercenként offline-nak jelöli azokat az eszközöket, amelyektől három időköznyi ideje (alapértelmezés: 90 s) nem érkezett jel. Rövid kimaradás így nem okoz állapotváltást.
- Minden online/offline váltás bekerül a `device_connectivity` naplóba; erre épül a riasztás (MIDAS-029).

### Metrikák

- Az agent percenként mintát vesz: CPU-terhelés (ezrelékben), felhasznált és teljes memória, a nem cserélhető lemezek foglaltsága, valamint az előző minta óta küldött és fogadott hálózati bájtok (loopback nélkül).
- **Puffer:** korlátos, legfeljebb egy napnyi (1440) minta. Teli állapotban a legrégebbi esik ki, és az eldobott minták száma a következő csomagban a szerverhez kerül (`devices.metrics_dropped`). A puffer percenként az agent védett állapotmappájába (`metrics.pb`) is íródik, így az agent újraindulása sem törli.
- **Feltöltés:** egyszerre egy, legfeljebb 60 mintás csomag lehet úton. A következő csak a szerver nyugtája után indul, kiesés után így a felgyűlt minták fokozatosan érkeznek vissza. A szerver csak tárolás után nyugtáz. A nyugtázatlan minta a pufferben marad, és a következő kapcsolatban újra elmegy. A szerver az ismételt mintát az (ügyfél, eszköz, időpont) kulcs alapján kiszűri.
- **Fogadás:** a szerver csak percre igazított, legfeljebb 48 órás és legfeljebb 5 percnyire jövőbeli, belsőleg ellentmondásmentes mintát fogad el; a hibás mintát kihagyja és naplózza. Tárolási hiba esetén nem nyugtáz, és bontja a kapcsolatot, hogy az agent újraküldjön.

### Összesítés és megőrzés

- Óránként: a perces adatokból óránkénti összesítés készül (darabszám, CPU átlag és maximum, memória átlag és maximum, lemez maximum, hálózati összeg). Az utolsó 48 órát minden futás újraszámolja, mert eddig érkezhet késő minta.
- A perces adat 30 napig marad meg, de csak akkor törlődik, ha az órája már szerepel az összesítésben. Az óránkénti összesítés 13 hónapig marad meg.
- Minden tábla ügyfélszintű RLS alatt van; a karbantartás ügyfelenként, az ügyfél saját kontextusában fut.

### Ellenőrzés

- Az agent-core egységtesztjei a puffer korlátját és az eldobásszámlálót, a csomagméretet, a nyugta előtti megtartást, az újraindulás utáni visszatöltést és a valódi mintavételt vizsgálják.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel, mTLS-sel és megszakítható hálózattal a következőket igazolja:
  - 130 felgyűlt minta csomagonként, hiánytalanul megérkezik, és az eszköz online;
  - néma kapcsolatnál három kimaradt jel után offline lesz;
  - a kiesés alatt keletkezett minták visszaállás után ismétlés nélkül beérkeznek, és az állapotnapló offline–online váltást mutat;
  - összesítés előtt a 30 napnál régebbi perces adat nem törlődik, utána igen, az összesítés értékei helyesek, a 13 hónapnál régebbi óránkénti adat törlődik.

### Ismert korlátok

- A megőrzési idő minden ügyfélre azonos (30 nap / 13 hónap); az ügyfélenkénti beállítás későbbi feladat.
- A metrikák megjelenítése (MIDAS-031) és az offline riasztás (MIDAS-029) külön feladat.

## English

November 7, 2026 · MIDAS-025 · PERF-02, PERF-05

### Presence

- The agent keepalive (every 30 seconds by default) is also the heartbeat ([agent protocol](agent-protokoll.md#english)). On every signal and at session start the server updates the device's `last_seen_at` and marks it online.
- Every 30 seconds the server marks devices offline when no signal arrived for three intervals (90 s by default), so a brief gap causes no state change.
- Every online/offline change is written to `device_connectivity`, which alerting (MIDAS-029) builds on.

### Metrics

- The agent samples once a minute: CPU load (per mille), used and total memory, usage of non-removable disks, and network bytes sent and received since the previous sample (excluding loopback).
- **Buffer:** bounded to one day (1440 samples). When full, the oldest sample is dropped and the drop count reaches the server with the next batch (`devices.metrics_dropped`). Every minute the buffer is also written to the agent's protected state folder (`metrics.pb`), so an agent restart does not lose it.
- **Upload:** at most one batch of up to 60 samples is in flight. The next one starts only after the server's acknowledgement, so after an outage the backlog arrives gradually. The server acknowledges only after storing. An unacknowledged sample stays in the buffer and is resent on the next connection; the server ignores repeats by the (customer, device, time) key.
- **Ingest:** the server accepts only minute-aligned, consistent samples at most 48 hours old and at most 5 minutes in the future; invalid samples are skipped and logged. On a storage failure it does not acknowledge and closes the session so the agent resends.

### Rollup and retention

- Hourly: minute data is rolled up per hour (count, CPU average and maximum, memory average and maximum, disk maximum, network totals). Every run recomputes the last 48 hours, the window in which late samples can arrive.
- Minute data is kept for 30 days and deleted only once its hour exists in the rollup. Hourly rollups are kept for 13 months.
- Every table is under per-customer RLS; housekeeping runs per customer in that customer's context.

### Verification

- agent-core unit tests cover the buffer bound and drop counter, batch size, retention until acknowledged, reload after restart and real sampling.
- The database suite, with real PostgreSQL, mTLS and a cuttable network, verifies the following:
  - 130 buffered samples arrive completely, batch by batch, and the device is online;
  - with a silent link the device goes offline after three missed signals;
  - samples taken during the outage arrive without repeats after recovery, and the connectivity log shows offline then online;
  - before the rollup, minute data older than 30 days is kept, afterwards it is deleted; rollup values are correct, and hourly data older than 13 months is deleted.

### Known limits

- Retention is the same for every customer (30 days / 13 months); per-customer settings are later work.
- Displaying metrics (MIDAS-031) and offline alerts (MIDAS-029) are separate tasks.
