# 0. fázis zárása / Phase 0 closure

[English](#english)

## Magyar

2026. november 7. · MIDAS-020 · Fáziszárási összesítő

### Eredmény

A 0. fázis 18 vállalt feladata ellenőrzött Done állapotú, és mindegyik issue lezárt. A MIDAS-018 (UAC-próba) és a MIDAS-019 (használhatósági prototípus) a projektgazda 2026. november 7-i döntésével az 1. fázisba került. Mindkettő megvalósítása kész, és felülvizsgálat alatt marad, amíg a külső erőforrást igénylő próba eredménye rögzítésre nem kerül ([terjedelemváltozások](../feladatlista.md#terjedelemváltozások)).

| ID | Feladat | Issue | Igazolt commit | Visszalépés |
| --- | --- | --- | --- | --- |
| MIDAS-001 | Monorepó és közös fejlesztői parancsok | [#2](https://github.com/Shran21/Midas/issues/2) | [79d871c](https://github.com/Shran21/Midas/commit/79d871cc610b0c2cffa39c2d6b9ca7c79fb5f7fa) | — |
| MIDAS-002 | Repó és projekttábla előkészítése a felülvizsgált tervhez | [#3](https://github.com/Shran21/Midas/issues/3) | [79d871c](https://github.com/Shran21/Midas/commit/79d871cc610b0c2cffa39c2d6b9ca7c79fb5f7fa) | felülvizsgálat után javítás |
| MIDAS-003 | Projekttábla és feladatmezők beállítása | [#4](https://github.com/Shran21/Midas/issues/4) | beállítás, visszaolvasva | — |
| MIDAS-004 | Linux- és Windows-CI, függőség- és titokellenőrzés | [#5](https://github.com/Shran21/Midas/issues/5) | [3439722](https://github.com/Shran21/Midas/commit/3439722673376720877e0ba756a4f59a595bd652), [5a11498](https://github.com/Shran21/Midas/commit/5a11498fdd8dd8202e323a60373ce255bb3fa81d) | újranyitás |
| MIDAS-005 | Biztonsági és hozzájárulási útmutató, döntési jegyzetek | [#6](https://github.com/Shran21/Midas/issues/6) | [79d871c](https://github.com/Shran21/Midas/commit/79d871cc610b0c2cffa39c2d6b9ca7c79fb5f7fa) | — |
| MIDAS-006 | Szerverváz, TLS, ACME és megfigyelhetőség | [#7](https://github.com/Shran21/Midas/issues/7) | [551a688](https://github.com/Shran21/Midas/commit/551a6887ae1e305439962a74791ff774a008e90d) | — |
| MIDAS-007 | Ügyfélmodell, migrációk és PostgreSQL RLS | [#8](https://github.com/Shran21/Midas/issues/8) | [c77bf1d](https://github.com/Shran21/Midas/commit/c77bf1ddd4853b52531de21e7991684d2a90c335) | felülvizsgálat után javítás |
| MIDAS-008 | Passkey, jelszó+TOTP és helyreállítás | [#9](https://github.com/Shran21/Midas/issues/9) | [418a33e](https://github.com/Shran21/Midas/commit/418a33e26b2bcb8ca16b9efbbda52f8ecefda30b) | — |
| MIDAS-009 | Böngészős munkamenet, CSRF és visszavonás | [#10](https://github.com/Shran21/Midas/issues/10) | [467bf35](https://github.com/Shran21/Midas/commit/467bf35d4dc8eaed27046eddd12bdc56e55a6c2f) | — |
| MIDAS-010 | Megbízható helyi AD-belépés és csoportleképezés | [#11](https://github.com/Shran21/Midas/issues/11) | [597dc69](https://github.com/Shran21/Midas/commit/597dc69bf5cf2435441e2658c2cf03b1fe765ff5) | — |
| MIDAS-011 | Szerepek, egyedi engedélyek és célhatókör | [#12](https://github.com/Shran21/Midas/issues/12) | [cb01ec7](https://github.com/Shran21/Midas/commit/cb01ec7d88a47fe9571cee8e503b2bb1586518f8) | — |
| MIDAS-012 | Csak hozzáfűzhető alapaudit | [#13](https://github.com/Shran21/Midas/issues/13) | [38af867](https://github.com/Shran21/Midas/commit/38af867aebfe7cb7a823c241f69dd974885e0708) | — |
| MIDAS-013 | Kétnyelvű dashboardváz és közös komponensek | [#14](https://github.com/Shran21/Midas/issues/14) | [24282ff](https://github.com/Shran21/Midas/commit/24282ffdb90d0e1e4582d74bdeaf902c2e49cf3b) | — |
| MIDAS-014 | Mobilnézet, fókuszkezelés és PWA-alapok | [#15](https://github.com/Shran21/Midas/issues/15) | [00cae7a](https://github.com/Shran21/Midas/commit/00cae7ae0675bb8a7f5862db6651d75e080b9e0b) | — |
| MIDAS-015 | Ügyfél-, telephely- és felhasználókezelés | [#16](https://github.com/Shran21/Midas/issues/16) | [f23e804](https://github.com/Shran21/Midas/commit/f23e8042a8eb7a17f7694e5a0ec1a0fcd0c81090) | — |
| MIDAS-016 | Egyszeri beállító folyamat a végleges HTTPS-originen | [#17](https://github.com/Shran21/Midas/issues/17) | [9b9c236](https://github.com/Shran21/Midas/commit/9b9c236637ebaa32cbd0a659b4b87b88bd0a3cac) | — |
| MIDAS-017 | Korai WebRTC- és kódoló-prototípus | [#18](https://github.com/Shran21/Midas/issues/18) | [c5f9c12](https://github.com/Shran21/Midas/commit/c5f9c12c64cc6541d6106d7c44b086f25cf18a23) | — |
| MIDAS-103 | Üzemeltetői előnézet és komponensenkénti mappaszerkezet | [#104](https://github.com/Shran21/Midas/issues/104) | [5b65f66](https://github.com/Shran21/Midas/commit/5b65f66f7e0c76a1690839f359aa72ee76d0bae7) | — |

Az igazolt commit az, amelyre az issue eredményszakasza hivatkozik. Mindegyiken sikeres a Windows- és a Linux-ellenőrzés, és ahol már futott, a biztonsági ellenőrzés is. A biztonsági workflow a 95e24d9 commit óta létezik, ezért a korábbi 79d871c és 5b65f66 commiton nincs ilyen futás. A mostani main biztonsági ellenőrzése a teljes előzményen keres titkot, és a jelenlegi függőségeket vizsgálja, így ezeket is lefedi. A MIDAS-003 Project-beállítás, ennek bizonyítéka a visszaolvasott konfiguráció. A zárás idején a main ([ee63c72](https://github.com/Shran21/Midas/commit/ee63c72d9f958939b3eb10afaaa576ee7e978bd1)) mindhárom ellenőrzése sikeres.

### Státuszelőzmények

Az ellenőrzés a GitHub saját státuszeseményeit használta (`ProjectV2ItemStatusChangedEvent`), nem a helyi jegyzetet. Mind a 18 tétel Backlogból indult, kihagyás nélkül haladt a Ready, In progress, In review és Done állapoton, és Done állapotban zárult. A visszalépések a munkafolyamat szerint történtek:

- MIDAS-002 és MIDAS-007: In review → In progress visszalépés, javítás, majd új In review → Done.
- MIDAS-004: Done → In progress újranyitás, majd új In review → Done.

### Nyitott hibák és figyelmeztetések

- Nincs nyitott, a terven kívüli issue. Nyitott titokriasztás: 0. Nyitott Dependabot-riasztás: 0. Kódelemzés (code scanning) nincs beállítva; a függőségeket a CI-ban a `cargo deny` és a `cargo audit`, a titkokat a gitleaks ellenőrzi.
- A main ágvédelme a jóváhagyott munkamenetet követi: lineáris előzmény, adminisztrátorra is érvényes védelem, force push és törlés tiltva, feltöltés előtti kötelező ellenőrzés és PR nélkül. A titokellenőrzés és a push protection bekapcsolt.
- Ismert korlátok, amelyek későbbi feladatokhoz tartoznak:
  - A forrásból, NASM nélkül fordított OpenH264 1080p-n kb. 8 fps-t ér el; a terjesztett vagy SIMD-del fordított kódoló a MIDAS-046 része, a hardveres kódolók a MIDAS-043–045 részei.
  - A str0m sávszélesség-becslése statikus képnél a valós kapacitás alá csökken, ezért kell a közös adaptív vezérlő (MIDAS-046).
  - RDP-munkamenetben a DXGI-rögzítés átmenetileg `AccessDenied` hibát adhat; a konzol- és RDP-esetet a MIDAS-042 és a MIDAS-058 méri.
  - A secure-desktop UAC VM-es próbája (MIDAS-018), valamint a résztvevős és képernyőolvasós próba (MIDAS-019) az 1. fázisban zárul.

### Döntések

- **WebRTC-integráció:** a natív komponens a str0m könyvtárra épül, a libwebrtc tartalék marad ([ADR 0002](../adr/0002-remote-transport.md), [mérési jegyzőkönyv](../meresek/webrtc-prototipus.md)). A döntést a projektgazda a zárás felülvizsgálatakor hagyja jóvá, a jóváhagyást a MIDAS-020 issue rögzíti. Újranyitását a MIDAS-058 hálózati mátrixa válthatja ki.
- **UAC-prototípus:** a LocalSystem-szolgáltatás a konzolmunkamenetben követi a bemeneti asztalt, és helyi Befejezés gombot ad ([próbaleírás](../meresek/uac-prototipus.md)). A döntés a VM-es próba után, a MIDAS-040 keretében rögzítendő.
- **Használhatósági prototípus:** a kattintható prototípus és a próbaprotokoll kész ([protokoll](../hasznalhatosag/tavsegitesi-keres.md)). A döntés a résztvevős próba után, a MIDAS-040 keretében rögzítendő.

### Támogatási mátrix a fázis végén

| Terület | Ellenőrizve | Bizonyíték | Később |
| --- | --- | --- | --- |
| Build, lint, tesztek | Windows Server 2022 és Ubuntu 24.04 CI-futtató | Windows- és Linux-ellenőrzés minden igazolt commiton | Debian és Rocky Linux csomagok (MIDAS-024) |
| Adatbázis | PostgreSQL 18, ügyfélszigetelési negatív próbával | MIDAS-007 | — |
| Hitelesítés | Passkey, jelszó+TOTP, helyreállító kód, tesztcímtáras AD-belépés | MIDAS-008, MIDAS-010 | Éles címtárak a pilot előtt |
| Webpanel | Magyar és angol nyelv, világos és sötét téma, mobilnézet, billentyűzet, Chromium-alapú böngésző | MIDAS-013, MIDAS-014, MIDAS-103 | További böngészők mátrixa (MIDAS-058) |
| Távsegítés-prototípus | Windows 11 Pro, RDP-munkamenet, loopback, szoftveres OpenH264, Chromium | MIDAS-017 | Konzol, TURN, hardveres kódolók (2. fázis) |
| UAC | Fejlesztői gépen: kódos kapcsolódás, elutasítások, helyi Befejezés | MIDAS-018 előellenőrzés | VM-es próba standard fiókkal (1. fázis) |

### Az 1. fázis indulási feltételei

- A MIDAS-020 Done állapota és a 0. mérföldkő lezárása után az 1. fázis tételei a saját függőségeik szerint léphetnek Readybe. Elsőként a MIDAS-021 (verziózott agentprotokoll) indítható, mert csak a már kész MIDAS-006-tól függ.
- A MIDAS-034 a MIDAS-019 elfogadására, a fáziszáró MIDAS-040 a MIDAS-018 és a MIDAS-019 elfogadására is vár.
- Az 1. fázis zárásához külső tesztkörnyezet kell: tíz vegyes tesztgép egyhetes jelentése, 24 órás agentmérés, 1000 szimulált agentes terhelési próba, valamint Debian, Ubuntu és Rocky Linux tesztgép. Ezek előkészítését érdemes a fázis elején megkezdeni.

## English

November 7, 2026 · MIDAS-020 · Phase closure summary

### Outcome

All 18 committed phase 0 tasks are reviewed Done and their issues are closed. MIDAS-018 (UAC proof) and MIDAS-019 (usability prototype) moved to phase 1 by the project owner's decision on November 7, 2026. Both are implemented and stay in review until the trial that needs external resources is recorded ([scope changes](../feladatlista.md#scope-changes)).

| ID | Task | Issue | Verified commit | Return |
| --- | --- | --- | --- | --- |
| MIDAS-001 | Monorepo and shared development commands | [#2](https://github.com/Shran21/Midas/issues/2) | [79d871c](https://github.com/Shran21/Midas/commit/79d871cc610b0c2cffa39c2d6b9ca7c79fb5f7fa) | — |
| MIDAS-002 | Prepare the repository and project for the reviewed plan | [#3](https://github.com/Shran21/Midas/issues/3) | [79d871c](https://github.com/Shran21/Midas/commit/79d871cc610b0c2cffa39c2d6b9ca7c79fb5f7fa) | fix after review |
| MIDAS-003 | Configure project views and task fields | [#4](https://github.com/Shran21/Midas/issues/4) | configuration, read back | — |
| MIDAS-004 | Linux/Windows CI, dependency and secret checks | [#5](https://github.com/Shran21/Midas/issues/5) | [3439722](https://github.com/Shran21/Midas/commit/3439722673376720877e0ba756a4f59a595bd652), [5a11498](https://github.com/Shran21/Midas/commit/5a11498fdd8dd8202e323a60373ce255bb3fa81d) | reopened |
| MIDAS-005 | Security/contribution guidance and decision records | [#6](https://github.com/Shran21/Midas/issues/6) | [79d871c](https://github.com/Shran21/Midas/commit/79d871cc610b0c2cffa39c2d6b9ca7c79fb5f7fa) | — |
| MIDAS-006 | Server skeleton, TLS, ACME and observability | [#7](https://github.com/Shran21/Midas/issues/7) | [551a688](https://github.com/Shran21/Midas/commit/551a6887ae1e305439962a74791ff774a008e90d) | — |
| MIDAS-007 | Tenant model, migrations and PostgreSQL RLS | [#8](https://github.com/Shran21/Midas/issues/8) | [c77bf1d](https://github.com/Shran21/Midas/commit/c77bf1ddd4853b52531de21e7991684d2a90c335) | fix after review |
| MIDAS-008 | Passkeys, password+TOTP and recovery | [#9](https://github.com/Shran21/Midas/issues/9) | [418a33e](https://github.com/Shran21/Midas/commit/418a33e26b2bcb8ca16b9efbbda52f8ecefda30b) | — |
| MIDAS-009 | Browser sessions, CSRF and revocation | [#10](https://github.com/Shran21/Midas/issues/10) | [467bf35](https://github.com/Shran21/Midas/commit/467bf35d4dc8eaed27046eddd12bdc56e55a6c2f) | — |
| MIDAS-010 | Trusted local AD sign-in and group mapping | [#11](https://github.com/Shran21/Midas/issues/11) | [597dc69](https://github.com/Shran21/Midas/commit/597dc69bf5cf2435441e2658c2cf03b1fe765ff5) | — |
| MIDAS-011 | Roles, overrides and target scope | [#12](https://github.com/Shran21/Midas/issues/12) | [cb01ec7](https://github.com/Shran21/Midas/commit/cb01ec7d88a47fe9571cee8e503b2bb1586518f8) | — |
| MIDAS-012 | Append-only baseline audit | [#13](https://github.com/Shran21/Midas/issues/13) | [38af867](https://github.com/Shran21/Midas/commit/38af867aebfe7cb7a823c241f69dd974885e0708) | — |
| MIDAS-013 | Bilingual dashboard shell and shared components | [#14](https://github.com/Shran21/Midas/issues/14) | [24282ff](https://github.com/Shran21/Midas/commit/24282ffdb90d0e1e4582d74bdeaf902c2e49cf3b) | — |
| MIDAS-014 | Mobile layout, focus handling and PWA foundations | [#15](https://github.com/Shran21/Midas/issues/15) | [00cae7a](https://github.com/Shran21/Midas/commit/00cae7ae0675bb8a7f5862db6651d75e080b9e0b) | — |
| MIDAS-015 | Customer, site and user management | [#16](https://github.com/Shran21/Midas/issues/16) | [f23e804](https://github.com/Shran21/Midas/commit/f23e8042a8eb7a17f7694e5a0ec1a0fcd0c81090) | — |
| MIDAS-016 | Single-use setup on the final HTTPS origin | [#17](https://github.com/Shran21/Midas/issues/17) | [9b9c236](https://github.com/Shran21/Midas/commit/9b9c236637ebaa32cbd0a659b4b87b88bd0a3cac) | — |
| MIDAS-017 | Early WebRTC and encoder proof | [#18](https://github.com/Shran21/Midas/issues/18) | [c5f9c12](https://github.com/Shran21/Midas/commit/c5f9c12c64cc6541d6106d7c44b086f25cf18a23) | — |
| MIDAS-103 | Operator console preview and component directory layout | [#104](https://github.com/Shran21/Midas/issues/104) | [5b65f66](https://github.com/Shran21/Midas/commit/5b65f66f7e0c76a1690839f359aa72ee76d0bae7) | — |

The verified commit is the one cited in each issue's result section. Each passed the Windows and Linux checks, and the security check wherever it already ran. The security workflow exists since 95e24d9, so the earlier 79d871c and 5b65f66 have no such run. The current main security check scans the full history for secrets and audits the present dependencies, which covers them too. MIDAS-003 is a Project configuration whose evidence is the read-back configuration. At closure, all three checks on main ([ee63c72](https://github.com/Shran21/Midas/commit/ee63c72d9f958939b3eb10afaaa576ee7e978bd1)) passed.

### Status histories

The review used GitHub's own status events (`ProjectV2ItemStatusChangedEvent`), not the local notes. All 18 items started in Backlog, passed Ready, In progress, In review and Done without skipping a state, and ended Done. Returns followed the workflow:

- MIDAS-002 and MIDAS-007: returned In review → In progress, were corrected, then passed a new In review → Done.
- MIDAS-004: reopened Done → In progress, then a new In review → Done.

### Open findings and caveats

- No unplanned open issues. Open secret-scanning alerts: 0. Open Dependabot alerts: 0. Code scanning is not configured; CI checks dependencies with `cargo deny` and `cargo audit` and secrets with gitleaks.
- Main branch protection follows the approved workflow: linear history, enforced for administrators, force pushes and deletion blocked, no pre-push required checks and no PR requirement. Secret scanning and push protection are enabled.
- Known limits owned by later tasks:
  - OpenH264 built from source without NASM reaches about 8 fps at 1080p; a distributed or SIMD build belongs to MIDAS-046, hardware encoders to MIDAS-043–045.
  - str0m's bandwidth estimate drops below real capacity on static screens, which the shared adaptive controller (MIDAS-046) addresses.
  - DXGI capture in an RDP session can briefly return `AccessDenied`; MIDAS-042 and MIDAS-058 measure console and RDP cases.
  - The secure-desktop UAC VM trial (MIDAS-018) and the participant and screen-reader trials (MIDAS-019) close in phase 1.

### Decisions

- **WebRTC integration:** the native component uses str0m, with libwebrtc as the fallback ([ADR 0002](../adr/0002-remote-transport.md#english), [measurement record](../meresek/webrtc-prototipus.md#english)). The project owner approves it in the closure review, and the MIDAS-020 issue records the approval. The MIDAS-058 network matrix may reopen it.
- **UAC prototype:** a LocalSystem service follows the input desktop in the console session and provides a local End button ([trial description](../meresek/uac-prototipus.md#english)). Record the decision after the VM trial, as part of MIDAS-040.
- **Usability prototype:** the clickable prototype and trial protocol are complete ([protocol](../hasznalhatosag/tavsegitesi-keres.md#english)). Record the decision after the participant trial, as part of MIDAS-040.

### Support matrix at phase end

| Area | Verified | Evidence | Later |
| --- | --- | --- | --- |
| Build, lint, tests | Windows Server 2022 and Ubuntu 24.04 CI runners | Windows and Linux checks on every verified commit | Debian and Rocky Linux packages (MIDAS-024) |
| Database | PostgreSQL 18 with negative tenant-isolation tests | MIDAS-007 | — |
| Authentication | Passkey, password+TOTP, recovery codes, test-directory AD sign-in | MIDAS-008, MIDAS-010 | Production directories before the pilot |
| Web panel | Hungarian and English, light and dark themes, mobile layout, keyboard, Chromium-based browser | MIDAS-013, MIDAS-014, MIDAS-103 | Wider browser matrix (MIDAS-058) |
| Remote-support proof | Windows 11 Pro, RDP session, loopback, software OpenH264, Chromium | MIDAS-017 | Console, TURN, hardware encoders (phase 2) |
| UAC | On the development machine: code pairing, rejections, local End | MIDAS-018 pre-check | VM trial with a standard account (phase 1) |

### Phase 1 prerequisites

- After MIDAS-020 is Done and milestone 0 is closed, phase 1 items may become Ready as their own dependencies allow. MIDAS-021 (versioned agent protocol) can start first, as it depends only on the completed MIDAS-006.
- MIDAS-034 also waits for MIDAS-019 to be accepted; the closure task MIDAS-040 also waits for MIDAS-018 and MIDAS-019.
- Closing phase 1 needs an external test environment: ten mixed test devices reporting for a week, a 24-hour agent measurement, a 1000-simulated-agent load test, and Debian, Ubuntu and Rocky Linux test machines. Start preparing these early in the phase.
