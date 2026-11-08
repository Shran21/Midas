# Agent erőforrásmérése / Agent resource measurement

[English](#english)

## Magyar

2026. november 8. · MIDAS-039 · PERF-01, PERF-02, PERF-03, PERF-04 · Mérési jegyzőkönyv

### Cél és módszer

A mérés a valódi Windows-agent és a tálcaprogram erőforrás-használatát rögzíti, külön a háttérben futó agentre és a megnyitott helyi felületre. A MIDAS felületéhez tartozó WebView2-folyamatokat is számba veszi. Előre megadott CPU-, GPU-, memória- vagy adatforgalmi plafon nincs; az eredmény az optimalizálás kiindulópontja.

- Eszköz: `python .github/scripts/agent-profile.py` (alapból 24 óra, percenkénti minta; a tálca állapotablaka az első óra végén, majd hatóránként 10 percre megnyílik). Előfeltétel: `python -m pip install psutil`.
- A szkript eldobható helyi PostgreSQL-t és release-szervert indít agentfigyelővel, előre jóváhagyott beiratkozási tokent hoz létre, majd elindítja:
  - a valódi `midas-agent.exe run --data …` parancsot konzolmódban, saját adatkönyvtárral, a bejelentkezett felhasználó jogával; szolgáltatást nem telepít;
  - a `midas-tray.exe` tálcaprogramot, amely ötmásodpercenként lekérdezi az agent állapotcsövét.
- A forgalmat egy helyi továbbító méri az agent és a szerver között. A TLS-kapcsolatot változatlanul továbbítja, csak a bájtokat számolja irányonként, és az újracsatlakozásokat.
- Minden mintánál rögzíti az agent, a tálcaprogram és az általuk indított WebView2-folyamatok CPU-idejét, munkakészletét, privát memóriáját, csúcs-munkakészletét, leíróinak és szálainak számát, valamint lemez-I/O-ját. Rögzíti továbbá a forgalmat, a tápellátást (hálózat vagy akkumulátor), a gép teljes CPU-terhelését és órajelét. Ötpercenként azt is, hogy a szerver online-nak látja-e az eszközt, és hány mérés-, leltár- és állapotsort tárolt tőle.
- A mintákat soronként azonnal kiírja, így megszakadt futás után is összesíthetők: `python .github/scripts/agent-profile.py --summarize`.
- A futás alatt a szkript a Windowsnak jelzi, hogy dolgozik (`SetThreadExecutionState`), így tétlen gép sem alszik el. Ez nem módosít beállítást, és a folyamattal együtt megszűnik. Lecsukott fedél vagy kézi alvás ellen nem véd.

### Eltérés a szolgáltatásként futó agenttől

- Konzolmódban az agent a felhasználó jogával fut, nem LocalSystemként. Ugyanazt a kapcsolat-, mérés-, leltár- és ellenőrzéskódot futtatja. A BitLocker-ellenőrzés felhasználói joggal `access_denied` eredményt adhat.
- A szolgáltatás naplót fájlba ír, a konzolmód a szabványos kimenetre; a mérés ezt fájlba irányítja.

### Eredmények

A futás 2026. november 8-án 20:47 és 23:42 között zajlott: 2,9 óra, 169 percenkénti minta, kimaradás nélkül. A projektgazda úgy döntött, hogy a 24 órás futás helyett ez a mérés elegendő. A futást ezért az agent leállításával zártam le, és a szkript a meglévő mintákból összesített. A nyers eredmény: [`agent-profil-2026-11-08.json`](agent-profil-2026-11-08.json).

| Mérőszám | Háttéragent | Tálcaprogram, zárt ablak (159 minta) | Tálcaprogram, nyitott állapotablak (10 minta) |
| --- | --- | --- | --- |
| CPU, egy mag %-ában (medián / 95. percentilis / maximum) | 0,05% / 0,23% / 0,43% | 0,00% / 0,03% / 0,03% | 0,00% / 0,15% / 0,15% |
| Átlagos CPU a teljes futásra | 0,07% (7,4 CPU-másodperc 2,9 óra alatt) | 0,7 CPU-másodperc | 0,1 CPU-másodperc |
| Munkakészlet (medián / maximum) | 27,2 / 28,1 MiB | 13,4 / 13,4 MiB | 13,0 / 13,0 MiB |
| Privát memória (medián / maximum) | 11,9 / 12,7 MiB | 1,9 / 1,9 MiB | 1,9 / 1,9 MiB |
| Leírók (első / utolsó / maximum) | 305 / 306 / 343 | 135 / 161 / 161 | 136 / 161 / 163 |
| Szálak (első / utolsó / maximum) | 13 / 11 / 17 | 2 / 2 / 5 | 3 / 2 / 4 |
| Lemezírás | 0,03 MiB | 0 | 0 |

- **Forgalom:** 35,7 KB ment a szerverre és 19,2 KB jött vissza, egyetlen kapcsolaton, újracsatlakozás nélkül. Óránként ez kb. 12 KiB fel és 6,5 KiB le, egy napra vetítve kb. 290 KiB és 156 KiB. Ebben benne vannak a percenkénti mérések, a keepalive-ok, a leltár és az állapotjelentések.
- **Szerveroldal:** az eszköz minden mintánál online volt, `host.desktop` képességgel. A szerver 170 mérési, 72 leltár- és 5 állapotsort tárolt tőle.
- **Leírók és memória:** az agent leírószáma, munkakészlete és privát memóriája a futás alatt nem nőtt. A tálcaprogram leírószáma az állapotablak első megnyitásakor egyszer 135-ről 161-re nőtt (az ablak erőforrásai), utána a második és a harmadik órában változatlan maradt.
- **WebView2:** egyik mintában sem volt MIDAS-folyamathoz tartozó `msedgewebview2.exe`. A jelenlegi tálcaprogram natív Windows-ablak, WebView nélkül, ezért a megnyitott felület sem indít WebView2-t.
- **Tápellátás:** a minták 49%-a hálózati tápról, 51%-a akkumulátorról készült (az akkumulátor 73%-ról 96%-ra töltődött). A gép teljes CPU-terhelésének mediánja 11,5% volt, az órajelé 1,7 GHz.

### Lelet és javítás

A mérőszkript első, négyperces próbája azt mutatta, hogy a tálcaprogram minden elutasított állapotlekérdezésnél egy leírót megtartott, vagyis ötmásodpercenként egyet. Konzolmódban az agent nem szolgáltatásként fut, ezért a tálca minden lekérdezést elutasít; élesben ugyanez történne, ha egy idegen folyamat foglalná el a csőnevet. Ok: a tokio a regisztrált, majd eldobott csövet csak a reaktor következő futásakor zárja, az elutasító ágban pedig a reaktor nem futott tovább. A tálca most előbb regisztráció nélkül nyitja meg és ellenőrzi a csövet. Integrációs teszt őrzi: a régi kód 200 elutasított lekérdezés után 182 leírót tartott meg, a javított 20 alattit. A fenti mérés már a javított programmal készült.

### Értékelés

- A háttéragent ezen a gépen egy mag kb. 0,07%-át, 27 MiB munkakészletet és 12 MiB privát memóriát használ, napi kb. 450 KiB forgalommal. Erőforrásplafont nem írunk elő; ez az optimalizálás kiindulópontja.
- A tálcaprogram zárt és nyitott állapotban is 2 MiB alatti privát memóriával és gyakorlatilag nulla CPU-val fut. Állandó, könnyű felületként megfelel a PERF-04 követelménynek, folyamatosan nyitott WebView nélkül.
- Az agent mérésküldése adja a forgalom nagy részét. Ha a napi forgalom számítana (például mobilnetes gépeknél), a mintaküldés kötegelése lehet a következő lépés.

### Korlátok

- A futás 2,9 órás, nem 24 órás; ezt a projektgazda elfogadta. A teljes leltár-újraegyeztetés (24 óránként) és a címtárvizsgálat (6 óránként, csak címtárfiókkal) ebben az időben nem futott le.
- Az agent konzolmódban, felhasználói joggal futott. A tálca ezért elutasította a nem szolgáltatásként futó agent csövét, és „nem fut” állapotot mutatott; szolgáltatásnál a tálca egy kb. 40 bájtos állapotsort is olvas.
- A gépen közben más munka is futott (fordítás, tesztek), ez a gép teljes terhelését növelte, de az agent saját CPU-idejét csak kis mértékben befolyásolhatta.
- Élő távsegítést ez a mérés nem tartalmaz; az a távsegítés fázisában mérendő.

## English

November 8, 2026 · MIDAS-039 · PERF-01, PERF-02, PERF-03, PERF-04 · Measurement record

### Goal and method

The measurement records the resource use of the real Windows agent and tray program, separately for the background agent and the open local interface. It also accounts for WebView2 processes belonging to the MIDAS interface. No CPU, GPU, memory or traffic ceiling is preset; the result is a baseline for optimization.

- Tool: `python .github/scripts/agent-profile.py` (by default 24 hours with one sample per minute; the tray status view opens for 10 minutes at the end of the first hour, then every six hours). Prerequisite: `python -m pip install psutil`.
- The script starts a throwaway local PostgreSQL and a release server with the agent listener, creates a preapproved enrollment token, then starts:
  - the real `midas-agent.exe run --data …` in console mode, with its own data directory and the signed-in user's rights; it installs no service;
  - the `midas-tray.exe` tray program, which queries the agent's status pipe every five seconds.
- A local relay between the agent and the server measures traffic. It forwards the TLS connection unchanged and only counts bytes per direction and reconnections.
- Every sample records CPU time, working set, private memory, peak working set, handle and thread counts and disk I/O of the agent, the tray program and any WebView2 processes they start. It also records traffic, the power source (mains or battery), total machine CPU load and clock. Every five minutes it also records whether the server sees the device online and how many sample, inventory and health rows it stored for it.
- Samples are written line by line at once, so an interrupted run can still be summarized: `python .github/scripts/agent-profile.py --summarize`.
- During the run the script tells Windows that it is working (`SetThreadExecutionState`), so an idle machine does not sleep. This changes no setting and ends with the process. It does not prevent sleep from a closed lid or a manual sleep.

### Difference from the agent running as a service

- In console mode the agent runs with the user's rights rather than as LocalSystem. It runs the same connection, sampling, inventory and check code. The BitLocker check may answer `access_denied` with user rights.
- The service logs to a file and console mode to standard output; the measurement redirects it to a file.

### Results

The run took place on November 8, 2026 between 20:47 and 23:42: 2.9 hours, 169 samples one minute apart, without gaps. The project owner decided that this measurement is sufficient instead of a 24-hour run. The run was therefore ended by stopping the agent, and the script summarized the samples it had. Raw result: [`agent-profil-2026-11-08.json`](agent-profil-2026-11-08.json).

| Metric | Background agent | Tray program, window closed (159 samples) | Tray program, status view open (10 samples) |
| --- | --- | --- | --- |
| CPU as % of one core (median / 95th percentile / maximum) | 0.05% / 0.23% / 0.43% | 0.00% / 0.03% / 0.03% | 0.00% / 0.15% / 0.15% |
| Average CPU over the whole run | 0.07% (7.4 CPU seconds in 2.9 hours) | 0.7 CPU seconds | 0.1 CPU seconds |
| Working set (median / maximum) | 27.2 / 28.1 MiB | 13.4 / 13.4 MiB | 13.0 / 13.0 MiB |
| Private memory (median / maximum) | 11.9 / 12.7 MiB | 1.9 / 1.9 MiB | 1.9 / 1.9 MiB |
| Handles (first / last / maximum) | 305 / 306 / 343 | 135 / 161 / 161 | 136 / 161 / 163 |
| Threads (first / last / maximum) | 13 / 11 / 17 | 2 / 2 / 5 | 3 / 2 / 4 |
| Disk writes | 0.03 MiB | 0 | 0 |

- **Traffic:** 35.7 KB went to the server and 19.2 KB came back, over one connection with no reconnection. That is about 12 KiB up and 6.5 KiB down per hour, or about 290 KiB and 156 KiB per day. It includes the per-minute samples, keepalives, inventory and health reports.
- **Server side:** the device was online in every sample, with the `host.desktop` capability. The server stored 170 sample, 72 inventory and 5 health rows for it.
- **Handles and memory:** the agent's handle count, working set and private memory did not grow during the run. The tray program's handle count rose once, from 135 to 161, when the status view first opened (the window's resources), and stayed unchanged through the second and third hours.
- **WebView2:** no sample contained an `msedgewebview2.exe` belonging to a MIDAS process. The current tray program is a native Windows window without a WebView, so the open interface starts no WebView2 either.
- **Power:** 49% of the samples were taken on mains power and 51% on battery (the battery charged from 73% to 96%). The machine's total CPU load had a median of 11.5%, at a 1.7 GHz clock.

### Finding and fix

The first four-minute trial of the measurement script showed the tray program keeping one handle for every refused status query, that is, one every five seconds. In console mode the agent does not run as a service, so the tray refuses every query; in production the same would happen if a foreign process took the pipe name. Cause: tokio closes a registered pipe that was dropped only when the reactor next runs, and on the refusal path the reactor never ran again. The tray now opens and checks the pipe without registering it first. An integration test guards it: the old code kept 182 handles after 200 refused queries, the fixed code fewer than 20. The measurement above already used the fixed program.

### Assessment

- On this machine the background agent uses about 0.07% of one core, a 27 MiB working set and 12 MiB of private memory, with about 450 KiB of traffic per day. No resource ceiling is set; this is the baseline for optimization.
- The tray program runs with less than 2 MiB of private memory and practically no CPU, whether its window is closed or open. As a permanent lightweight interface it meets PERF-04 without a permanently open WebView.
- The agent's sample uploads make up most of the traffic. If daily traffic mattered (for example on mobile-data machines), batching sample uploads could be the next step.

### Limits

- The run lasted 2.9 hours, not 24; the project owner accepted this. The full inventory reconciliation (every 24 hours) and the directory scan (every 6 hours, only with a directory account) did not run in that time.
- The agent ran in console mode with user rights. The tray therefore refused the pipe of an agent not running as a service and showed "not running"; with the service the tray also reads a status line of about 40 bytes.
- Other work ran on the machine meanwhile (builds, tests). It raised the machine's total load but could affect the agent's own CPU time only slightly.
- This measurement contains no live remote support; that is measured in the remote-support phase.
