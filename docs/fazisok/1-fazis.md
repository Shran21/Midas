# 1. fázis zárása / Phase 1 closure

[English](#english)

## Magyar

2026. november 9. · Frissítve: november 10. · MIDAS-040 · Fáziszárási összesítő

### Eredmény

Az 1. fázis 17 vállalt feladata ellenőrzött Done állapotú, és mindegyik issue lezárt. A MIDAS-018, MIDAS-019, MIDAS-023 és MIDAS-024 a projektgazda 2026. november 7-i döntése szerint In review marad, amíg ő maga ellenőrzi; Done állapotuk nem feltétele a zárásnak ([terjedelemváltozások](../feladatlista.md#terjedelemváltozások)).

| ID | Feladat | Issue | Igazolt commit | Visszalépés |
| --- | --- | --- | --- | --- |
| MIDAS-021 | Verziózott agentprotokoll és újracsatlakozás | [#22](https://github.com/Shran21/Midas/issues/22) | [e9bd9ca](https://github.com/Shran21/Midas/commit/e9bd9ca6ca6695ee83c894f5835717bef8735fbb) | felülvizsgálat után javítás |
| MIDAS-022 | Agentbeiratkozás, jóváhagyás, mTLS és visszavonás | [#23](https://github.com/Shran21/Midas/issues/23) | [d194859](https://github.com/Shran21/Midas/commit/d194859cf4e3d6d05c6ea358eb2bcd12bc84ffa2) | felülvizsgálat után javítás |
| MIDAS-025 | Heartbeat, metrikák, összesítés és megőrzés | [#26](https://github.com/Shran21/Midas/issues/26) | [406688e](https://github.com/Shran21/Midas/commit/406688e2894ef1039f38c6bdbb48f56eae31e060) | — |
| MIDAS-026 | Változásalapú hardver- és szoftverleltár | [#27](https://github.com/Shran21/Midas/issues/27) | [c1360f9](https://github.com/Shran21/Midas/commit/c1360f960fd02580eb4cfe236dbe059b24177825) | felülvizsgálat után javítás |
| MIDAS-027 | Állapotellenőrzések és platformképességek | [#28](https://github.com/Shran21/Midas/issues/28) | [b240c8a](https://github.com/Shran21/Midas/commit/b240c8ab35e40576de813c2a1df51dbbb7b0b66c) | — |
| MIDAS-028 | Öröklődő ügyfél-, telephely- és csoportszabályzatok | [#29](https://github.com/Shran21/Midas/issues/29) | [efe8caa](https://github.com/Shran21/Midas/commit/efe8caab50e0da4de494b4fdd5673775b4df52b7) | — |
| MIDAS-029 | Riasztás, összevonás és e-mail | [#30](https://github.com/Shran21/Midas/issues/30) | [c616252](https://github.com/Shran21/Midas/commit/c616252cb4f52a64ea235e040ad4e7f176dcb3e3) | — |
| MIDAS-030 | Teams- és Slack-értesítési végpontok | [#31](https://github.com/Shran21/Midas/issues/31) | [bee9c46](https://github.com/Shran21/Midas/commit/bee9c4604f564829624927f02f72cc26148e2967) | — |
| MIDAS-031 | Eszközoldal, kártyák, helyi menük és élő állapot | [#32](https://github.com/Shran21/Midas/issues/32) | [11f5656](https://github.com/Shran21/Midas/commit/11f5656f7f29e987954aef44cacfb62bbe5a5fcc) | — |
| MIDAS-032 | Jogosultságot követő kereső és parancspaletta | [#33](https://github.com/Shran21/Midas/issues/33) | [af381de](https://github.com/Shran21/Midas/commit/af381de3e39e77d103d75202b51d7375f1f22cd3) | — |
| MIDAS-033 | Windows MSI és védett regisztrációs átadás | [#34](https://github.com/Shran21/Midas/issues/34) | [360c7bb](https://github.com/Shran21/Midas/commit/360c7bb0b376dcc4432508138d6f1336c96a3c80) | — |
| MIDAS-034 | Agent állapotnézet és tálcaikon | [#35](https://github.com/Shran21/Midas/issues/35) | [610f6f4](https://github.com/Shran21/Midas/commit/610f6f4c93407d755024970e4529f428bbcbb3cc) | felülvizsgálat után javítás |
| MIDAS-035 | Flottaterhelés és újracsatlakozási hullám | [#36](https://github.com/Shran21/Midas/issues/36) | [91a60f2](https://github.com/Shran21/Midas/commit/91a60f245912fb56291e31b433ad04f018cced80) | — |
| MIDAS-036 | Ügyfelenkénti, csak olvasó AD-connector | [#37](https://github.com/Shran21/Midas/issues/37) | [0652358](https://github.com/Shran21/Midas/commit/065235838a64e2af83e55e631c907316e6d1a7e2) | — |
| MIDAS-037 | AD-felderítés és agenthez egyeztetett jelöltek | [#38](https://github.com/Shran21/Midas/issues/38) | [dce0c32](https://github.com/Shran21/Midas/commit/dce0c32956741c4e15a15c6901a910a1f086de52), [9cd294a](https://github.com/Shran21/Midas/commit/9cd294acc34a16145b619d1b9f1311e279f6dbc3) | újranyitás |
| MIDAS-038 | Felderítési és regisztrációs nézet | [#39](https://github.com/Shran21/Midas/issues/39) | [9fe2183](https://github.com/Shran21/Midas/commit/9fe2183b30c63342319b8694d9f8cc0a64004951) | — |
| MIDAS-039 | 24 órás háttérmérés és szerverprofil | [#40](https://github.com/Shran21/Midas/issues/40) | [eb3cf0a](https://github.com/Shran21/Midas/commit/eb3cf0ae57789ce9f77a0a97757c951d5a888535) | — |

Az igazolt commit az, amelyre az issue eredményszakasza hivatkozik. Mindegyiken sikeres a Windows-, a Linux- és a biztonsági ellenőrzés. A MIDAS-037 két commitja az eredeti megvalósítás és az újranyitás utáni javítás. Az összesítéskor a main ([9cd294a](https://github.com/Shran21/Midas/commit/9cd294acc34a16145b619d1b9f1311e279f6dbc3)) mindhárom ellenőrzése sikeres.

### Státuszelőzmények

Az ellenőrzés a GitHub saját státuszeseményeit használta (`ProjectV2ItemStatusChangedEvent`), nem a helyi jegyzetet. Mind a 17 tétel Backlogból indult, kihagyás nélkül haladt a Ready, In progress, In review és Done állapoton, és Done állapotban zárult. A visszalépések a munkafolyamat szerint történtek:

- MIDAS-021, MIDAS-022 és MIDAS-026: In review → In progress, javítás, majd új In review → Done.
- MIDAS-034: In review → In progress, mert a Linux-CI a Windows-útvonal szétválasztásán elbukott; javítás (610f6f4), majd új In review → Done.
- MIDAS-037: Done → In progress újranyitás. Irodán kívül a tartományi gép leltárából eltűnt a címtár-azonosító; javítás (9cd294a), majd új In review → Done.

A felmentett tételek állapota: a MIDAS-018, MIDAS-023 és MIDAS-024 Backlog → Ready → In progress → In review úton jár, a MIDAS-019 kétszer visszalépett In progressbe, és ismét In review. Mindegyik issue nyitott, és a projektgazdai ellenőrzésre vár.

### Nyitott hibák és figyelmeztetések

- A november 9-i ütemezett Security-futás a korábban sikeres `ce9d69d` commiton a `libsqlite3-sys 0.37.0` függőséget jelezte (RUSTSEC-2026-0459). A csomag a zárolt listában szerepelt, az aktív PostgreSQL-fordításban nem. A javítás közvetlenül az SQLx magját és PostgreSQL-meghajtóját használja, így a nem használt SQLite-, MySQL- és makrófüggőség kikerül. A migrációk és a név szerinti sorleképezés megmaradnak; a kompatibilitást az [adatbázispróba](../adatbazis.md#ellenőrzés) ellenőrzi. A korábbi sikeres CI-eredmény a javítás ellenőrzését nem helyettesíti.
- Nincs nyitott, a terven kívüli issue. Nyitott titokriasztás: 0. Nyitott Dependabot-riasztás: 0. Kódelemzés (code scanning) nincs beállítva; a függőségeket a CI-ban a `cargo deny` és a `cargo audit`, a titkokat a gitleaks ellenőrzi.
- A fázisban talált és javított hibák:
  - A MIDAS-034 tálcaprogramja minden elutasított állapotlekérdezésnél egy leírót megtartott. A MIDAS-039 mérése találta, és ott javult (f609192), integrációs teszttel.
  - A MIDAS-037 egyeztetése irodán kívül megszakadt; az újranyitott feladat javította (9cd294a).
- Ismert korlátok, amelyek későbbi munkához tartoznak:
  - Az 500 eszközös lista kb. 480 KiB JSON; több ezer eszköznél lapozás vagy tömörítés kell ([flottamérés](../meresek/flotta-terheles.md)).
  - A Teams- és Slack-csatornának még nincs webpanelfelülete; addig az API kezeli ([riasztások](../riasztasok.md)).
  - A linuxos tartományi gép felderítéskor csak jelölt ([címtár-connector](../cimtar-connector.md)).
  - Valódi Server Core- és grafikus asztal nélküli Linux-gépen a gépprofilt még nem próbáltam; a leképezést egységtesztek igazolják ([gépprofil](../gepprofil.md)).
  - A flottamérés második fele akkumulátoros üzemben futott; hálózati tápon érdemes megismételni.

### Döntések

- **2026. november 10.:** a tízgépes, egyhetes próba kötelező marad, de a projektgazda a MIDAS-086 pilotindítása elé helyezte. Az M1 kapuját nem blokkolja; az M2 a kapu elfogadása után indulhat. A próba nem tekinthető elvégzettnek.

- **2026. november 7.:** a MIDAS-018, -019 és -020, majd a MIDAS-023 és -024 projektgazdai ellenőrzésre vár, de nem akadályozza a későbbi munkát. Sikertelen ellenőrzésnél a MIDAS-034, MIDAS-042, MIDAS-047, MIDAS-059 és az ezekre épülő munka újravizsgálandó.
- **2026. november 8.:** a MIDAS-039 24 órás agentmérését a 2,9 órás mérés váltja ki; a kapunál ez számít a 24 órás agentmérés bizonyítékának.
- **UAC-prototípus és használhatósági prototípus:** a 0. fázis zárása szerint a döntés a MIDAS-018 VM-es, illetve a MIDAS-019 résztvevős próbája után a MIDAS-040 keretében rögzítendő. Mindkét próba még a projektgazdára vár, így a döntés nincs rögzítve; a feladatok In review állapotban követik.

### A kapu feltételei

| Feltétel | Állapot | Bizonyíték |
| --- | --- | --- |
| A monitorozás, felderítés, regisztráció és terhelésmérés elfogadva | Teljesült | MIDAS-021–039 eredményszakaszai |
| Az összes vállalt feladat ellenőrzött Done | Teljesült | Státuszelőzmények fent |
| Támogatási mátrix és a következő fázis feltételei rögzítve | Teljesült | Ez a dokumentum |
| Tíz vegyes tesztgép egyhetes próbája | Áthelyezve; nem M1-kapufeltétel | A november 10-i döntés szerint a [MIDAS-086](https://github.com/Shran21/Midas/issues/87) pilotindításának kötelező előfeltétele; még nincs elvégezve |
| A 24 órás (döntés szerint 2,9 órás) agentmérés és az egyórás, 1000 agentes mérés bizonyítéka csatolt | Teljesült | [agentmérés](../meresek/agent-profil.md), [flottamérés](../meresek/flotta-terheles.md) |
| A MIDAS-018, -019, -023 és -024 ellenőrzésének állapota rögzített | Teljesült | Státuszelőzmények fent |

### Támogatási mátrix a fázis végén

| Terület | Ellenőrizve | Bizonyíték | Később |
| --- | --- | --- | --- |
| Build, lint, tesztek | Windows Server 2022 és Ubuntu 24.04 CI-futtató | Windows-, Linux- és biztonsági ellenőrzés minden igazolt commiton | — |
| Agentkapcsolat | Verziótartomány, újracsatlakozás, mTLS, visszavonás; 1000 egyidejű agent | MIDAS-021, MIDAS-022, MIDAS-035 | Valódi hálózati késleltetés |
| Windows-agent | Windows 11 Pro konzolmódban, Windows Server 2022 a CI-n; MSI-telepítés és -eltávolítás a CI-futtatón | MIDAS-026, MIDAS-027, MIDAS-033, MIDAS-039 | Szolgáltatáspróba saját gépen (MIDAS-023) |
| Linux-agent | Ubuntu 24.04 a CI-n | MIDAS-026, MIDAS-027 | Debian, Ubuntu és Rocky Linux csomag (MIDAS-024) |
| Szerver és adatbázis | PostgreSQL 18, sorszintű ügyfélszigetelés 10 ügyféllel és 1000 agenttel | MIDAS-025, MIDAS-035 | — |
| Riasztás | E-mail sor, Teams- és Slack-webhook helyi HTTPS-fogadóval | MIDAS-029, MIDAS-030 | Valódi Teams- és Slack-tér |
| AD | LDAPS-tesztcímtár 1206 géppel, valódi tartományi laptop elérhető és elérhetetlen tartományvezérlővel | MIDAS-036, MIDAS-037 | Éles címtárak a pilot előtt |
| Webpanel | Magyar és angol, világos és sötét téma, billentyűzet, Chromium, helyi próbakörnyezet | MIDAS-031, MIDAS-032, MIDAS-038, MIDAS-039 | Böngészőpróba a valódi fejlesztői szerverrel; a helyi tanúsítványt a projektgazda fogadja el |
| Gépprofil | Windows 11 (`Client`) és Windows Server (`Server`) valódi gépen | MIDAS-039 | Server Core, grafikus asztal nélküli Linux |
| Tesztflotta | — | — | Tíz vegyes tesztgép egyhetes jelentése a pilot előtt (MIDAS-086) |

### A 2. fázis indulási feltételei

- A fázisszabály szerint a 2. fázis csak a MIDAS-040 Done állapota után indulhat.
- Ezután elsőként a MIDAS-041 (felhasználó nélküli szerveres hozzáférés), a MIDAS-042 (DXGI-rögzítés és több monitor) és a MIDAS-047 (védett transzport és külön UAC-komponens) indulhat, mert minden más függőségük kész.
- Külső erőforrások, amelyeket érdemes előre biztosítani: NVIDIA-, AMD- és Intel-gép a hardveres kódolókhoz (MIDAS-043–045), TURN-kiszolgáló (MIDAS-052), valamint Windows 11-es tesztgép standard fiókkal (MIDAS-018).
- Sikertelen MIDAS-018 vagy MIDAS-019 ellenőrzésnél a MIDAS-042, MIDAS-047 és az ezekre épülő munka újravizsgálandó.

## English

November 9, 2026 · Updated November 10 · MIDAS-040 · Phase closure summary

### Outcome

All 17 committed phase 1 tasks are reviewed Done and their issues are closed. By the project owner's decision of November 7, 2026, MIDAS-018, MIDAS-019, MIDAS-023 and MIDAS-024 stay in review until the owner verifies them; their Done status is not a closure condition ([scope changes](../feladatlista.md#scope-changes)).

| ID | Task | Issue | Verified commit | Return |
| --- | --- | --- | --- | --- |
| MIDAS-021 | Versioned agent protocol and reconnection | [#22](https://github.com/Shran21/Midas/issues/22) | [e9bd9ca](https://github.com/Shran21/Midas/commit/e9bd9ca6ca6695ee83c894f5835717bef8735fbb) | fix after review |
| MIDAS-022 | Agent enrollment, approval, mTLS and revocation | [#23](https://github.com/Shran21/Midas/issues/23) | [d194859](https://github.com/Shran21/Midas/commit/d194859cf4e3d6d05c6ea358eb2bcd12bc84ffa2) | fix after review |
| MIDAS-025 | Heartbeat, metrics, rollups and retention | [#26](https://github.com/Shran21/Midas/issues/26) | [406688e](https://github.com/Shran21/Midas/commit/406688e2894ef1039f38c6bdbb48f56eae31e060) | — |
| MIDAS-026 | Incremental hardware and software inventory | [#27](https://github.com/Shran21/Midas/issues/27) | [c1360f9](https://github.com/Shran21/Midas/commit/c1360f960fd02580eb4cfe236dbe059b24177825) | fix after review |
| MIDAS-027 | Health checks and platform capabilities | [#28](https://github.com/Shran21/Midas/issues/28) | [b240c8a](https://github.com/Shran21/Midas/commit/b240c8ab35e40576de813c2a1df51dbbb7b0b66c) | — |
| MIDAS-028 | Inherited customer, site and group policies | [#29](https://github.com/Shran21/Midas/issues/29) | [efe8caa](https://github.com/Shran21/Midas/commit/efe8caab50e0da4de494b4fdd5673775b4df52b7) | — |
| MIDAS-029 | Alerts, deduplication and email | [#30](https://github.com/Shran21/Midas/issues/30) | [c616252](https://github.com/Shran21/Midas/commit/c616252cb4f52a64ea235e040ad4e7f176dcb3e3) | — |
| MIDAS-030 | Teams and Slack notification endpoints | [#31](https://github.com/Shran21/Midas/issues/31) | [bee9c46](https://github.com/Shran21/Midas/commit/bee9c4604f564829624927f02f72cc26148e2967) | — |
| MIDAS-031 | Device pages, cards, context menus and live state | [#32](https://github.com/Shran21/Midas/issues/32) | [11f5656](https://github.com/Shran21/Midas/commit/11f5656f7f29e987954aef44cacfb62bbe5a5fcc) | — |
| MIDAS-032 | Permission-aware search and command palette | [#33](https://github.com/Shran21/Midas/issues/33) | [af381de](https://github.com/Shran21/Midas/commit/af381de3e39e77d103d75202b51d7375f1f22cd3) | — |
| MIDAS-033 | Windows MSI and protected enrollment delivery | [#34](https://github.com/Shran21/Midas/issues/34) | [360c7bb](https://github.com/Shran21/Midas/commit/360c7bb0b376dcc4432508138d6f1336c96a3c80) | — |
| MIDAS-034 | Agent status view and tray icon | [#35](https://github.com/Shran21/Midas/issues/35) | [610f6f4](https://github.com/Shran21/Midas/commit/610f6f4c93407d755024970e4529f428bbcbb3cc) | fix after review |
| MIDAS-035 | Fleet load and reconnection testing | [#36](https://github.com/Shran21/Midas/issues/36) | [91a60f2](https://github.com/Shran21/Midas/commit/91a60f245912fb56291e31b433ad04f018cced80) | — |
| MIDAS-036 | Per-customer read-only AD connector | [#37](https://github.com/Shran21/Midas/issues/37) | [0652358](https://github.com/Shran21/Midas/commit/065235838a64e2af83e55e631c907316e6d1a7e2) | — |
| MIDAS-037 | AD discovery and verified agent matching | [#38](https://github.com/Shran21/Midas/issues/38) | [dce0c32](https://github.com/Shran21/Midas/commit/dce0c32956741c4e15a15c6901a910a1f086de52), [9cd294a](https://github.com/Shran21/Midas/commit/9cd294acc34a16145b619d1b9f1311e279f6dbc3) | reopened |
| MIDAS-038 | Discovery and enrollment view | [#39](https://github.com/Shran21/Midas/issues/39) | [9fe2183](https://github.com/Shran21/Midas/commit/9fe2183b30c63342319b8694d9f8cc0a64004951) | — |
| MIDAS-039 | 24-hour background profiling and server profiles | [#40](https://github.com/Shran21/Midas/issues/40) | [eb3cf0a](https://github.com/Shran21/Midas/commit/eb3cf0ae57789ce9f77a0a97757c951d5a888535) | — |

The verified commit is the one the issue's result section links. Windows, Linux and security checks succeeded on each. MIDAS-037's two commits are the original implementation and the fix after reopening. At the time of this summary, all three checks of main ([9cd294a](https://github.com/Shran21/Midas/commit/9cd294acc34a16145b619d1b9f1311e279f6dbc3)) succeeded.

### Status history

The check used GitHub's own status events (`ProjectV2ItemStatusChangedEvent`), not the local notes. All 17 items started in Backlog, passed Ready, In progress, In review and Done without skipping, and ended Done. Returns followed the workflow:

- MIDAS-021, MIDAS-022 and MIDAS-026: In review → In progress, fix, then a new In review → Done.
- MIDAS-034: In review → In progress because the Linux CI failed on splitting a Windows path; fix (610f6f4), then a new In review → Done.
- MIDAS-037: reopened Done → In progress. Away from the office the directory identity dropped out of a domain member's inventory; fix (9cd294a), then a new In review → Done.

Exempt items: MIDAS-018, MIDAS-023 and MIDAS-024 went Backlog → Ready → In progress → In review; MIDAS-019 returned to In progress twice and is in review again. All their issues are open and wait for the owner's verification.

### Open defects and warnings

- The November 9 scheduled Security run flagged `libsqlite3-sys 0.37.0` (RUSTSEC-2026-0459) on the previously passing `ce9d69d` commit. The package was in the lockfile, outside the active PostgreSQL build. The fix uses SQLx core and its PostgreSQL driver directly, removing unused SQLite, MySQL and macro dependencies. Embedded migrations and named row mapping are preserved; the [database test](../adatbazis.md#english) checks compatibility. Earlier successful CI does not establish acceptance of this fix.
- No unplanned issue is open. Open secret alerts: 0. Open Dependabot alerts: 0. Code scanning is not configured; CI checks dependencies with `cargo deny` and `cargo audit`, and secrets with gitleaks.
- Defects found and fixed during the phase:
  - MIDAS-034's tray program kept one handle per refused status query. The MIDAS-039 measurement found it and MIDAS-039 fixed it (f609192), with an integration test.
  - MIDAS-037 matching broke away from the office; the reopened task fixed it (9cd294a).
- Known limits that belong to later work:
  - The 500-device list is about 480 KiB of JSON; thousands of devices need paging or compression ([fleet measurement](../meresek/flotta-terheles.md#english)).
  - Teams and Slack channels have no web panel view yet; the API manages them until then ([alerts](../riasztasok.md#english)).
  - A Linux domain member stays a discovery candidate ([directory connector](../cimtar-connector.md#english)).
  - The host profile is not yet tried on a real Server Core or headless Linux host; unit tests verify the mapping ([host profile](../gepprofil.md#english)).
  - The second half of the fleet measurement ran on battery; a repeat on mains power is advisable.

### Decisions

- **November 10, 2026:** the owner moved the mandatory ten-device, one-week trial to before the MIDAS-086 pilot. It no longer blocks M1; M2 may start after gate acceptance. The trial is not considered performed.

- **November 7, 2026:** MIDAS-018, -019 and -020, then MIDAS-023 and -024 wait for the owner's verification without blocking later work. If a verification fails, re-check MIDAS-034, MIDAS-042, MIDAS-047, MIDAS-059 and work built on them.
- **November 8, 2026:** a 2.9-hour run replaces MIDAS-039's 24-hour agent measurement and counts as that evidence at this gate.
- **UAC and usability prototypes:** per the phase 0 closure, the decisions are recorded within MIDAS-040 after the MIDAS-018 VM trial and the MIDAS-019 participant trial. Both trials still wait for the owner, so the decisions are not recorded yet; the tasks track them in review.

### Gate conditions

| Condition | State | Evidence |
| --- | --- | --- |
| Monitoring, discovery, enrollment and load evidence accepted | Met | Result sections of MIDAS-021–039 |
| All committed work reviewed Done | Met | Status history above |
| Support matrix and next-phase prerequisites recorded | Met | This document |
| One-week trial with ten mixed devices | Moved; not an M1 gate condition | By the November 10 decision, a mandatory prerequisite to starting the [MIDAS-086](https://github.com/Shran21/Midas/issues/87) pilot; not yet performed |
| Evidence attached for the 24-hour (by decision 2.9-hour) agent and the one-hour 1000-agent measurements | Met | [agent measurement](../meresek/agent-profil.md#english), [fleet measurement](../meresek/flotta-terheles.md#english) |
| Verification state of MIDAS-018, -019, -023 and -024 recorded | Met | Status history above |

### Support matrix at the end of the phase

| Area | Verified | Evidence | Later |
| --- | --- | --- | --- |
| Build, lint, tests | Windows Server 2022 and Ubuntu 24.04 CI runners | Windows, Linux and security checks on every verified commit | — |
| Agent connection | Version range, reconnection, mTLS, revocation; 1000 concurrent agents | MIDAS-021, MIDAS-022, MIDAS-035 | Real network latency |
| Windows agent | Windows 11 Pro in console mode, Windows Server 2022 on CI; MSI install and removal on the CI runner | MIDAS-026, MIDAS-027, MIDAS-033, MIDAS-039 | Service trial on the owner's machine (MIDAS-023) |
| Linux agent | Ubuntu 24.04 on CI | MIDAS-026, MIDAS-027 | Debian, Ubuntu and Rocky Linux packages (MIDAS-024) |
| Server and database | PostgreSQL 18, row-level tenant isolation with 10 customers and 1000 agents | MIDAS-025, MIDAS-035 | — |
| Alerts | E-mail queue, Teams and Slack webhooks with a local HTTPS receiver | MIDAS-029, MIDAS-030 | Real Teams and Slack workspaces |
| AD | LDAPS test directory with 1206 computers, a real domain laptop with a reachable and an unreachable domain controller | MIDAS-036, MIDAS-037 | Production directories before the pilot |
| Web panel | Hungarian and English, light and dark themes, keyboard, Chromium, local harness | MIDAS-031, MIDAS-032, MIDAS-038, MIDAS-039 | A browser check against the real development server; the owner accepts its local certificate |
| Host profile | Windows 11 (`Client`) and Windows Server (`Server`) on real machines | MIDAS-039 | Server Core, headless Linux |
| Test fleet | — | — | Ten mixed test devices reporting for a week before the pilot (MIDAS-086) |

### Phase 2 start conditions

- By the phase rule, phase 2 starts only after MIDAS-040 is Done.
- Then MIDAS-041 (unattended server access), MIDAS-042 (DXGI capture and multiple monitors) and MIDAS-047 (protected transport and separate UAC component) can start first, as all their other dependencies are done.
- External resources worth arranging early: NVIDIA, AMD and Intel machines for the hardware encoders (MIDAS-043–045), a TURN server (MIDAS-052), and a Windows 11 test machine with a standard account (MIDAS-018).
- If MIDAS-018 or MIDAS-019 fails verification, re-check MIDAS-042, MIDAS-047 and work built on them.
