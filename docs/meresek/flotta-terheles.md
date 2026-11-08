# Flottaterhelés és újracsatlakozási hullám / Fleet load and reconnection wave

[English](#english)

## Magyar

2026. november 8. · MIDAS-035 · PERF-06, SEC-01 · Mérési jegyzőkönyv

### Cél és módszer

A próba azt méri, mekkora terhelést jelent a szervernek és az adatbázisnak legalább 1000 egyszerre kapcsolódó agent egy órán át, mennyi idő alatt áll helyre a flotta egy szerver-újraindítás után, és hogy a sok ügyfél adata közben is elkülönül-e.

- Eszköz: `python .github/scripts/fleet-load.py` (alapból 1000 agent, 10 ügyfél, az agentek fele az első ügyfélnél, 60 perc, hullám a 45. percben, 30 másodperces mintavétel). A szkript eldobható helyi PostgreSQL-t indít, migrál, demóadatot tölt, release-szervert indít agentfigyelővel, majd a `dev_fleet` szimulátorral beiratkoztatja és bekapcsolja az agenteket. Csak a 127.0.0.1-et használja.
- A szimulált agent a valódi `agent-core` kapcsolatot használja: előre jóváhagyott tokennel iratkozik be, mTLS-sel kapcsolódik, 30 másodpercenként keepalive-ot küld, percenként mérést tölt fel (induláskor egy óra visszamenőleges méréssel), induláskor teljes leltárt küld, és állapotellenőrzést jelent.
- Minden mintánál a szkript rögzíti:
  - az online és a beiratkozott eszközök számát, a mérés-, leltár- és állapotsorokat, valamint az adatbázis méretét;
  - a szerver, a PostgreSQL-folyamatok és a terhelést adó szimulátor CPU-használatát (100% = egy mag) és memóriáját;
  - a nagy ügyfél eszközlistájának (`GET /api/tenants/{ügyfél}/devices`) válaszidejét és méretét.
- **Újracsatlakozási hullám:** a 45. percben a szkript kilövi a szerverfolyamatot, azonnal újraindítja, és méri, mennyi idő múlva jelentkezik újra minden agent.
- **Ügyfélszigetelés:** a végén a futtatási adatbázis-szerepkörrel ügyfelenként, sorszintű szigeteléssel lekérdezi az eszközöket, a méréseket és a leltárt. Azt is ellenőrzi, hogy az API csak a tag ügyfeleknél válaszol.

### Környezet

- Gép: Intel Core i5-8350U (4 mag, 8 szál), 15,9 GiB RAM, Windows 11 Pro. A szerver, a PostgreSQL és a terhelést adó szimulátor ugyanazon a gépen fut, tehát az eredmény óvatos, felső becslés.
- PostgreSQL 18, release-szerver (`target/release`).

### Eredmények

A futás 2026. november 8-án 19:02 és 20:03 között zajlott; 113 minta készült. A nyers eredmény: [`flotta-terheles-2026-11-08.json`](flotta-terheles-2026-11-08.json).

**Kapcsolódás és helyreállás**

- Mind az 1000 agent 31 másodperc alatt beiratkozott és kapcsolódott. Ebben benne van az előre jóváhagyott beiratkozás, a tanúsítvány kiadása, 60 000 visszamenőleges mérés és a teljes leltár betöltése.
- Minden mintában 1000 eszköz volt online, a hullám előtt és után is.
- **Újracsatlakozási hullám:** a 45. percben kilőtt és újraindított szerverhez mind az 1000 agent 8,4 másodpercen belül újra jelentkezett.
- A nagy ügyfél eszközlistája mind a 113 lekérdezésnél 200-as választ adott, 500 eszközzel és 468–480 KiB mérettel.

**Terhelés**

19:34-kor a gép hálózati tápról akkumulátorra váltott (Windows Kernel-Power 105-ös esemény, a 32. perc körül). Ezután minden folyamat lassabb órajelen futott. Az állandó munkát végző szimulátor is 2,1-szer annyi CPU-t használt, ezért a két szakaszt külön közlöm. Hálózati tápon a szerver valós terhelése mérhető; az akkumulátoros szakasz lassabb gépet mutat, nem a szerver változását.

| Mérőszám (medián / 95. percentilis / maximum) | Hálózati táp, 0–31. perc, 61 minta | Akkumulátor, 33–60. perc, 51 minta |
| --- | --- | --- |
| Szerver CPU (100% = egy mag) | 4,4% / 7,6% / 12,5%* | 11,1% / 18,6% / 19,7% |
| Szerver memória (RSS) | 170 / 171 / 171 MiB | 174 / 184 / 184 MiB |
| PostgreSQL CPU, összes folyamat | 5,5% / 8,7% / 57,1%* | 15,0% / 23,4% / 29,7% |
| PostgreSQL munkakészlet, összesítve | 693 / 711 / 712 MiB | 706 / 840 / 1013 MiB |
| Szimulátor CPU (kontroll) | 1,0% / 2,1% / 27,0%* | 2,1% / 3,6% / 5,3% |
| Eszközlista válaszideje (500 eszköz) | 88 / 125 / 292 ms | 241 / 345 / 488 ms |

\* A maximum az első mintában volt, a beiratkozás, a 60 000 visszamenőleges mérés és a leltár betöltése közben. Az első minta nélkül a szerver CPU-maximuma 12,5%.

- A PostgreSQL munkakészlete folyamatonként összeadott érték, ezért a megosztott puffert többször számolja; valós memóriaigénye ennél kisebb.
- A PostgreSQL CPU-ja az élő folyamatok összege. A 30. és a 32. percben kilépő folyamatok miatt két minta negatív lett; ezeket kihagytam.
- Az adatbázis 24,7 MiB-ról 40,5 MiB-ra nőtt. A mérések 60 000-ről 119 633 sorra nőttek, ez percenként és agentenként egy minta. A leltár 17 344, az állapot 5000 sort tartalmaz.

**Ügyfélszigetelés**

- A futtatási szerepkör sorszintű szigeteléssel mind a 10 ügyfélnél csak a saját eszközeit látta: 500, 56, 55, 55, 55, 55, 56, 56, 56 és 56 eszközt. Az eszközök, a mérések és a leltár mindegyike pontosan egy ügyfélhez tartozott.
- Az API a bejelentkezett technikus két tagügyfelénél (Alfa Kft., Béta Zrt.) 200-as, a másik nyolcnál 404-es választ adott.

### Értékelés

- Ezen a 4 magos, 2018-as laptopon 1000 agent hálózati tápon a szervernek egy mag kb. 4–8%-át, a PostgreSQL-nek 5–9%-át adja. A szerver 170 MiB memóriával fut. Erőforrásplafont nem írunk elő; a számok az optimalizálás kiindulópontjai.
- A legnagyobb, jól látható tétel a teljes eszközlista: 500 eszköz kb. 480 KiB JSON és kb. 90 ms. Lehetséges következő lépés a lapozás vagy tömörítés, ha a lista több ezer eszközre nő.
- A szerver-újraindítás után a flotta 8,4 másodperc alatt helyreállt. A szórt, exponenciális újracsatlakozás ennél a méretnél nem okozott kapcsolódási torlódást.

### Korlátok

- A szimulált agentek a valódi kapcsolatkódot használják, de állandó, mesterséges adatot küldenek; a leltárt csak induláskor küldik.
- A szerver, az adatbázis és a szimulátor egy gépen futott, valódi hálózati késleltetés nélkül. Az eredmény ezért felső becslés a szerverterhelésre, alsó becslés a hálózati időkre.
- A második félidőt az akkumulátoros üzem lassította; ismételt mérést hálózati tápon érdemes végezni.
- Sok párhuzamos webpanel-munkamenet terhelését ez a próba nem méri.

## English

November 8, 2026 · MIDAS-035 · PERF-06, SEC-01 · Measurement record

### Goal and method

The trial measures the server and database load of at least 1000 agents connected at once for an hour, how long the fleet takes to recover after a server restart, and whether the data of many customers stays separated meanwhile.

- Tool: `python .github/scripts/fleet-load.py` (by default 1000 agents, 10 customers, half of the agents in the first customer, 60 minutes, a wave at minute 45, samples every 30 seconds). The script starts a throwaway local PostgreSQL, migrates, seeds demo data, starts a release server with the agent listener, then enrolls and connects the agents with the `dev_fleet` simulator. It uses only 127.0.0.1.
- A simulated agent uses the real `agent-core` connection: it enrolls with a preapproved token, connects with mTLS, sends a keepalive every 30 seconds, uploads samples every minute (with one hour of backlog at start), sends a full inventory at start and reports health checks.
- Every sample records:
  - the online and enrolled device counts, the sample, inventory and health rows, and the database size;
  - CPU (100% = one core) and memory of the server, the PostgreSQL processes and the simulator generating the load;
  - the response time and size of the large customer's device list (`GET /api/tenants/{tenant}/devices`).
- **Reconnection wave:** at minute 45 the script kills the server process, restarts it at once and measures how long until every agent has reported again.
- **Tenant isolation:** at the end it queries devices, samples and inventory per customer with the runtime database role under row-level isolation. It also checks that the API answers only for member customers.

### Environment

- Machine: Intel Core i5-8350U (4 cores, 8 threads), 15.9 GiB RAM, Windows 11 Pro. The server, PostgreSQL and the load-generating simulator run on the same machine, so the results are a cautious upper bound.
- PostgreSQL 18, release server (`target/release`).

### Results

The run took place on November 8, 2026 between 19:02 and 20:03 and produced 113 samples. Raw result: [`flotta-terheles-2026-11-08.json`](flotta-terheles-2026-11-08.json).

**Connection and recovery**

- All 1000 agents enrolled and connected within 31 seconds. This includes preapproved enrollment, certificate issuance, 60,000 backlog samples and the full inventory.
- Every sample showed 1000 devices online, before and after the wave.
- **Reconnection wave:** after the server was killed and restarted at minute 45, all 1000 agents reported again within 8.4 seconds.
- The large customer's device list answered 200 on all 113 requests, with 500 devices and 468–480 KiB.

**Load**

At 19:34 the machine switched from mains power to battery (Windows Kernel-Power event 105, around minute 32). From then on every process ran at a lower clock. The simulator, whose work is constant, also used 2.1 times as much CPU, so the two periods are reported separately. Mains power shows the server's real load; the battery period shows a slower machine, not a change in the server.

| Metric (median / 95th percentile / maximum) | Mains, minutes 0–31, 61 samples | Battery, minutes 33–60, 51 samples |
| --- | --- | --- |
| Server CPU (100% = one core) | 4.4% / 7.6% / 12.5%* | 11.1% / 18.6% / 19.7% |
| Server memory (RSS) | 170 / 171 / 171 MiB | 174 / 184 / 184 MiB |
| PostgreSQL CPU, all processes | 5.5% / 8.7% / 57.1%* | 15.0% / 23.4% / 29.7% |
| PostgreSQL working set, summed | 693 / 711 / 712 MiB | 706 / 840 / 1013 MiB |
| Simulator CPU (control) | 1.0% / 2.1% / 27.0%* | 2.1% / 3.6% / 5.3% |
| Device list response time (500 devices) | 88 / 125 / 292 ms | 241 / 345 / 488 ms |

\* The maximum came in the first sample, while enrollment, the 60,000 backlog samples and the inventory were loaded. Without the first sample the server CPU maximum is 12.5%.

- The PostgreSQL working set is summed per process and so counts shared buffers several times; the real memory need is lower.
- PostgreSQL CPU is the sum over living processes. Processes that exited at minutes 30 and 32 made two samples negative; they are left out.
- The database grew from 24.7 MiB to 40.5 MiB. Samples grew from 60,000 to 119,633 rows, one per agent per minute. Inventory holds 17,344 rows and health 5,000.

**Tenant isolation**

- Under row-level isolation the runtime role saw only each customer's own devices for all 10 customers: 500, 56, 55, 55, 55, 55, 56, 56, 56 and 56 devices. Devices, samples and inventory each belonged to exactly one customer.
- The API answered 200 for the signed-in operator's two member customers (Alfa Kft., Béta Zrt.) and 404 for the other eight.

### Assessment

- On this 4-core 2018 laptop, 1000 agents on mains power cost the server about 4–8% of one core and PostgreSQL 5–9%. The server runs with 170 MiB of memory. No resource ceiling is set; the figures are a baseline for optimization.
- The largest visible item is the full device list: 500 devices are about 480 KiB of JSON and about 90 ms. Paging or compression is a possible next step once a list grows to thousands of devices.
- After the server restart the fleet recovered within 8.4 seconds. Jittered exponential reconnection caused no connection pile-up at this size.

### Limits

- The simulated agents use the real connection code but send constant, synthetic data; they send inventory only at start.
- Server, database and simulator ran on one machine without real network latency. The results are therefore an upper bound for server load and a lower bound for network times.
- Battery operation slowed the second half; a repeat run on mains power is advisable.
- This trial does not measure the load of many concurrent web panel sessions.
