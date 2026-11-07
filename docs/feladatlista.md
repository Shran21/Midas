# Feladatfelosztás / Task allocation

[English](#english)

## Magyar

2026. október 31. · Közzétett kezdő felosztás. Az aktuális állapot a [GitHub Projectben](https://github.com/users/Shran21/projects/4) követhető.

**97 ütemezett tétel**: 91 megvalósítási vagy előkészítési feladat és 6 fáziszárás. **6 további javaslat** mérföldkő nélkül. A 74 korábbi feladat azonosítói a részletes feltételeknél visszakereshetők; a bontás és az áthelyezés megőrizte az eredeti szándékot a felülvizsgált követelményekkel együtt.

A [backlog.json](backlog.json) tartalmazza ugyanazokat az azonosítókat, kétnyelvű címeket és elfogadási feltételeket, függőségeket, prioritást, kezdő státuszt és mérföldkövet. Az itt szereplő elfogadási feltételek mellett a hivatkozott [követelmények](kovetelmenyek.md) és a [fázis elfogadási próbái](rendszerterv.md#ütemterv) is kötelezők.

A MIDAS-103 az első fejlesztési kör után hozzáadott M0-feladat: komponensenkénti mappaszerkezet és két témát használó üzemeltetői előnézet. Az alábbi induló státuszösszesítés az eredeti 102 tétel importját rögzíti.

2026. november 7-én a MIDAS-018 és a MIDAS-019 M0-ból M1-be került. Ugyanaznap a projektgazda a MIDAS-018, a MIDAS-019 és a MIDAS-020 Done állapotát kivette a későbbi munka feltételei közül. Az okát és a hatását a [terjedelemváltozások](#terjedelemváltozások) rögzítik.

### Induló sapkák

Minden új tétel **Backlogban** jön létre. Az alábbi „induláskor” érték az import és az előkészítési ellenőrzés utáni tervezett állapot. A Ready-jelölteket külön, visszaellenőrzött váltással kell előléptetni; az import nem készít mesterséges előzményt.

| Státusz | Importkor | Előkészítés után tervezett |
| --- | --- | --- |
| Backlog | 102 | 99 |
| Ready | 0 | 3 |
| In progress | 0 | 0 |
| In review | 0 | 0 |
| Done | 0 | 0 |

Induló Ready-jelöltek: **MIDAS-001** (fejlesztői alapok), **MIDAS-002** (repó és Project előkészítése), **MIDAS-005** (útmutatók és döntési jegyzetek). Ha bármelyik szükséges hozzáférés hiányzik, az érintett tétel Backlogban marad. Nincs előre Done-nak nyilvánított megvalósítás.

### Mérföldkövek

Az M0–M5 helyi kulcs, nem GitHub-mérföldkőszám. A célrepót, Projectet és a tényleges mérföldkő-azonosítókat és az issue-hivatkozásokat a backlog.json közzétételi adatai rögzítik.

| Kulcs | Mérföldkő | Tételek kapuval együtt | Zárófeladat | Előző kapu |
| --- | --- | --- | --- | --- |
| M0 | 0. Alapok | 19 | MIDAS-020 | — |
| M1 | 1. Agent és monitorozás | 22 | MIDAS-040 | MIDAS-020 |
| M2 | 2. Távsegítés | 20 | MIDAS-060 | MIDAS-040 |
| M3 | 3. Automatizált karbantartás | 14 | MIDAS-074 | MIDAS-060 |
| M4 | 4. Megerősítés és pilot | 13 | MIDAS-087 | MIDAS-074 |
| M5 | 5. Bővítések | 9 | MIDAS-096 | MIDAS-087 |

Minden későbbi fázis minden vállalt tétele az előző kapu Done állapotától is függ; kivétel, hogy az M1 tételei 2026. november 7. óta nem függenek a MIDAS-020-tól. A következő táblázatok a fázison belüli és egyéb közvetlen függőségeket mutatják; az előző kaput nem ismétlik soronként. A JSON minden függőséget teljesen felsorol. Azonos fázison belül a kész, egymástól független feladatok párhuzamosan végezhetők.

P0: elsőbbséget igénylő alapműködés, biztonság vagy kapu; P1: a fázison belül utána sorolható munka; P2: alacsonyabb sorrendi prioritás vagy külön javaslat. A vállalt vagy javasolt állapotot a tétel típusa és mérföldkő-hozzárendelése határozza meg, nem a prioritása. A prioritás nem mentesít a fázis vagy a függőség alól. A zárófeladat mindig az adott mérföldkő legvégén következik.

### 0. Alapok

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-001 | Monorepó és közös fejlesztői parancsok | Ready | P0 | — |
| MIDAS-002 | Repó és projekttábla előkészítése a felülvizsgált tervhez | Ready | P0 | — |
| MIDAS-003 | Projekttábla és feladatmezők beállítása | Backlog | P0 | MIDAS-002 |
| MIDAS-004 | Linux- és Windows-CI, függőség- és titokellenőrzés | Backlog | P0 | MIDAS-001, MIDAS-002 |
| MIDAS-005 | Biztonsági és hozzájárulási útmutató, döntési jegyzetek | Ready | P1 | — |
| MIDAS-006 | Szerverváz, TLS, ACME és megfigyelhetőség | Backlog | P0 | MIDAS-001 |
| MIDAS-007 | Ügyfélmodell, migrációk és PostgreSQL RLS | Backlog | P0 | MIDAS-001, MIDAS-006 |
| MIDAS-008 | Passkey, jelszó+TOTP és helyreállítás | Backlog | P0 | MIDAS-006, MIDAS-007 |
| MIDAS-009 | Böngészős munkamenet, CSRF és visszavonás | Backlog | P0 | MIDAS-008 |
| MIDAS-010 | Megbízható helyi AD-belépés és csoportleképezés | Backlog | P1 | MIDAS-008, MIDAS-007 |
| MIDAS-011 | Szerepek, egyedi engedélyek és célhatókör | Backlog | P0 | MIDAS-008, MIDAS-007 |
| MIDAS-012 | Csak hozzáfűzhető alapaudit | Backlog | P1 | MIDAS-007, MIDAS-008 |
| MIDAS-013 | Kétnyelvű dashboardváz és közös komponensek | Backlog | P0 | MIDAS-009, MIDAS-011, MIDAS-103 |
| MIDAS-014 | Mobilnézet, fókuszkezelés és PWA-alapok | Backlog | P1 | MIDAS-013 |
| MIDAS-015 | Ügyfél-, telephely- és felhasználókezelés | Backlog | P0 | MIDAS-013, MIDAS-011, MIDAS-012 |
| MIDAS-016 | Egyszeri beállító folyamat a végleges HTTPS-originen | Backlog | P0 | MIDAS-006, MIDAS-008, MIDAS-013 |
| MIDAS-017 | Korai WebRTC- és kódoló-prototípus | Backlog | P0 | MIDAS-001, MIDAS-006 |
| MIDAS-103 | Üzemeltetői előnézet és komponensenkénti mappaszerkezet | Backlog | P0 | MIDAS-001, MIDAS-002 |
| MIDAS-020 | 0. fázis ellenőrzése és mérföldkőzárás | Backlog | P0 | A fázis összes többi tétele |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-001:** Tiszta klónon a dokumentált Windows- és Linux-build, formázás és ellenőrzés lefut. Követelmény: —. Korábbi azonosító: F0-01.
- **MIDAS-002:** A megerősített célrepóban a kétnyelvű terv, licenc és sablonok elérhetők, a projekt és a mérföldkövek helyesen kapcsolódnak. Követelmény: —. Korábbi azonosító: —.
- **MIDAS-003:** A projektnézetek a feladatokkal azonos mérföldkő-, prioritás- és címkeadatokat mutatnak. Új feladat duplikáció nélkül kapcsolható a projekthez. Követelmény: —. Korábbi azonosító: —.
- **MIDAS-004:** A hibás módosítást a CI elutasítja. Helyi ellenőrzés után a változás közvetlenül mainre kerül, külön fejlesztési ág és PR nélkül; elfogadásához ugyanazon commit Windows-, Linux- és biztonsági ellenőrzése sikeres. A feltöltés előtti kötelező CI-feltétel nincs bekapcsolva; a lineáris előzmény, a force push és a main törlésének tiltása megmarad. Követelmény: SEC-06. Korábbi azonosító: F0-02.
- **MIDAS-005:** A közzétett útmutató tartalmazza a privát hibabejelentést, a hozzájárulás feltételeit, a licencet és a Rust/Tauri/WebRTC döntéseket. Követelmény: SEC-06. Korábbi azonosító: F0-03.
- **MIDAS-006:** HTTPS-válasz, állapotvégpont és naplózás működik. A későbbi agentbeiratkozás és az mTLS-üzem külön útvonalat kap. Követelmény: SEC-02. Korábbi azonosító: F0-04.
- **MIDAS-007:** Két ügyfél adatai hibás alkalmazásszűrő és újrahasznált adatbázis-kapcsolat mellett is elkülönülnek a próbában. Követelmény: SEC-01. Korábbi azonosító: F0-05.
- **MIDAS-008:** Passkey-felhasználóellenőrzés, tartalék hitelesítés, egyszeri helyreállító kód és próbálkozáskorlátozás tesztelve. Követelmény: SEC-01. Korábbi azonosító: F0-06.
- **MIDAS-009:** A visszavont munkamenet nem indíthat API- vagy WebSocket-műveletet, az állapotváltoztatás CSRF-védett. Követelmény: SEC-01, SEC-03. Korábbi azonosító: F0-07.
- **MIDAS-010:** Tesztcímtárral az engedélyezett fiók belép, a tiltott fiók és hibás LDAPS-tanúsítvány elutasított. Követelmény: AD-01, SEC-01. Korábbi azonosító: F0-08.
- **MIDAS-011:** A megvonás elsőbbsége, ügyfélhatár, normál/admin/UAC és unattended jogok negatív próbái sikeresek. Követelmény: SEC-01, REMOTE-04. Korábbi azonosító: F0-09.
- **MIDAS-012:** Belépés, jogosultságváltozás és kérésdöntés kereshető, hitelesítő adatok nélkül. Követelmény: SEC-04. Korábbi azonosító: F0-10.
- **MIDAS-013:** Belépés után az ügyfélkörnyezet, navigáció és állapotok mindkét nyelven és témában működnek. Követelmény: UI-01, UI-07, UI-08. Korábbi azonosító: F0-11.
- **MIDAS-014:** A fő oldalak billentyűzettel és érintéssel elérhetők, a gyorsítótár csak statikus felületet tárol. Követelmény: UI-03, UI-08. Korábbi azonosító: F0-12.
- **MIDAS-015:** Az admin ügyfelet és korlátozott technikust hoz létre, az új jogok és megvonások a célhatókörben érvényesülnek. Követelmény: UI-01, SEC-01. Korábbi azonosító: F0-13.
- **MIDAS-016:** Az első admin a végleges originen regisztrál passkeyt, a felhasznált setupfelhatalmazás lejár és lezárul. Követelmény: SEC-01, SEC-06. Korábbi azonosító: F0-14.
- **MIDAS-017:** Valódi Windows-asztal böngészőből látható, a mérési jegyzőkönyv dönt a WebRTC-integrációról. A prototípus igazolja a hálózati mérések elérhetőségét és a bitráta, felbontás, képkockaszám futás közbeni állíthatóságát; romló és javuló hálózaton rögzíti az adaptáció reakcióját. Követelmény: PERF-04, PERF-07. Korábbi azonosító: F2-01.
- **MIDAS-103:** A szerver a server/, a webpanel a web-panel/, a platformok az agent/agent-windows/ és agent/agent-linux/ mappába kerülnek ugyanabban a repóban. A dokumentáció és a közös parancsok követik az útvonalakat; a Windows- és Linux-build sikeres. Az üzemeltetői előnézet világos és sötét témát, magyar és angol nyelvet, navigációt, kereshető és szűrhető eszközlistát, valamint eszközrészleteket ad. A választott téma és nyelv megmarad újratöltéskor. A mintaadatok egyértelműen jelöltek; az előnézet nem hajt végre távoli műveletet. A tényleges szerverkapcsolat állapota ettől elkülönül. A szűk nézet és a billentyűzetes használat ellenőrzött. Követelmény: UI-08. Korábbi azonosító: —.
- **MIDAS-020:** A fázis összes vállalt feladata ellenőrzött Done; a fázis próbáinak bizonyítéka, a státuszelőzmények és a következő fázis feltételei áttekintve. A WebRTC-döntés dokumentált, az M1-be áthelyezett UAC- és használhatósági próba állapota rögzített. Követelmény: —. Korábbi azonosító: F0-15.

### 1. Agent és monitorozás

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-018 | Normál kapcsolatú UAC és védett beviteli út próbája | Backlog | P0 | MIDAS-017 (M0-ból áthelyezve, kapufüggés nélkül) |
| MIDAS-019 | Adminpanel és helyi jóváhagyás használhatósági prototípusa | Backlog | P0 | MIDAS-013, MIDAS-014 (M0-ból áthelyezve, kapufüggés nélkül) |
| MIDAS-021 | Verziózott agentprotokoll és újracsatlakozás | Backlog | P0 | MIDAS-006 |
| MIDAS-022 | Agentbeiratkozás, jóváhagyás, mTLS és visszavonás | Backlog | P0 | MIDAS-021, MIDAS-011, MIDAS-012 |
| MIDAS-023 | Windows-szolgáltatás és biztonságos helyi IPC | Backlog | P0 | MIDAS-022 |
| MIDAS-024 | Linux-agent, systemd és csomagok | Backlog | P0 | MIDAS-022 |
| MIDAS-025 | Heartbeat, metrikák, összesítés és megőrzés | Backlog | P0 | — |
| MIDAS-026 | Változásalapú hardver- és szoftverleltár | Backlog | P1 | MIDAS-025 |
| MIDAS-027 | Állapotellenőrzések és platformképességek | Backlog | P1 | — |
| MIDAS-028 | Öröklődő ügyfél-, telephely- és csoportszabályzatok | Backlog | P1 | MIDAS-026, MIDAS-011 |
| MIDAS-029 | Riasztás, összevonás és e-mail | Backlog | P0 | MIDAS-025, MIDAS-027, MIDAS-028 |
| MIDAS-030 | Teams- és Slack-értesítési végpontok | Backlog | P2 | MIDAS-029 |
| MIDAS-031 | Eszközoldal, kártyák, helyi menük és élő állapot | Backlog | P0 | MIDAS-013, MIDAS-025, MIDAS-026, MIDAS-029 |
| MIDAS-032 | Jogosultságot követő kereső és parancspaletta | Backlog | P2 | MIDAS-031 |
| MIDAS-033 | Windows MSI és védett regisztrációs átadás | Backlog | P0 | — |
| MIDAS-034 | Agent állapotnézet és tálcaikon | Backlog | P1 | — |
| MIDAS-035 | Flottaterhelés és újracsatlakozási hullám | Backlog | P0 | MIDAS-031, MIDAS-025 |
| MIDAS-036 | Ügyfelenkénti, csak olvasó AD-connector | Backlog | P0 | MIDAS-010, MIDAS-022 |
| MIDAS-037 | AD-felderítés és agenthez egyeztetett jelöltek | Backlog | P0 | MIDAS-036, MIDAS-026 |
| MIDAS-038 | Felderítési és regisztrációs nézet | Backlog | P0 | MIDAS-037, MIDAS-031 |
| MIDAS-039 | 24 órás háttérmérés és szerverprofil | Backlog | P0 | MIDAS-034, MIDAS-027 |
| MIDAS-040 | 1. fázis ellenőrzése és mérföldkőzárás | Backlog | P0 | A fázis összes többi tétele, a MIDAS-018, MIDAS-019, MIDAS-023 és MIDAS-024 kivételével |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-018:** Standard felhasználónál a böngésző kezeli a valódi secure-desktop UAC-t, helyi leállítással és védelmek kikapcsolása nélkül. Követelmény: REMOTE-03, REMOTE-05, REMOTE-06, REMOTE-07. Korábbi azonosító: —.
- **MIDAS-019:** A kezelő indít kérést, az új felhasználó érti és elutasítja, majd új kérésből enged és bont. Billentyűzetes próba is sikerül. A mobil adminpanel és a helyi jóváhagyó ablak képernyőolvasós próbája is dokumentált, mindkét nyelven. Követelmény: UI-04, UI-06, UI-07, UI-08. Korábbi azonosító: —.
- **MIDAS-021:** Eltérő támogatott verziók üzenetet váltanak, kapcsolatkimaradás után az agent önállóan visszatér. Követelmény: PERF-01. Korábbi azonosító: F1-01.
- **MIDAS-022:** Első beiratkozás TLS-en működik, művelethez mTLS kell. Újrahasznált token, rossz cél és visszavont cert elutasított. Követelmény: SEC-02. Korábbi azonosító: F1-02.
- **MIDAS-023:** Újraindítás és hálózatkiesés után visszatér, idegen folyamat és más Windows-munkamenet nem éri el az IPC-műveleteket. Követelmény: PERF-05, SERVER-01, REMOTE-06. Korábbi azonosító: F1-03.
- **MIDAS-024:** Debianon, Ubuntun és Rocky Linuxon csomagból indul, ügyfélhez kötötten és biztonságos szolgáltatásjoggal. Követelmény: SERVER-01. Korábbi azonosító: F1-04.
- **MIDAS-025:** Három kimaradás után offline, a kiesés adatai szabályosan visszaérkeznek, az összesítés és lejárati törlés ellenőrzött. Követelmény: PERF-02, PERF-05. Korábbi azonosító: F1-05.
- **MIDAS-026:** Windows/Linux változás után frissül a leltár, újraindítás megőrzi az utolsó sikeres egyeztetést. Követelmény: PERF-02. Korábbi azonosító: F1-06.
- **MIDAS-027:** Szolgáltatás-, frissítés-, Defender-, BitLocker- és tárolóhibák kipróbálva, a nem elérhető ellenőrzés látható. Követelmény: SERVER-01, UI-05. Korábbi azonosító: F1-07.
- **MIDAS-028:** Felülírás csak a megfelelő csoportot érinti, a gép oldalán követhető a beállítás eredete. Követelmény: UI-01, SEC-01. Korábbi azonosító: F1-08.
- **MIDAS-029:** Kiesésre egy kezelhető riasztás és értesítés születik, a nyugtázás auditált. Követelmény: UI-07. Korábbi azonosító: F1-09.
- **MIDAS-030:** Próbariasztás az ügyfélhez kijelölt csatornán jelenik meg, a webhooktitok védett. Követelmény: SEC-01. Korábbi azonosító: F1-10.
- **MIDAS-031:** A menü és az eszközoldal egyező jogokat használ, a kiválasztott ügyfél és gép látszik, az offline funkciók érthetők. Követelmény: UI-01, UI-02, UI-03, UI-05, UI-07, SERVER-04. Korábbi azonosító: F1-11.
- **MIDAS-032:** Csak hozzáférhető gép és művelet kereshető, billentyűzettel megnyitható. Követelmény: UI-03, UI-08. Korábbi azonosító: F1-12.
- **MIDAS-033:** Kézi és központi telepítés a megfelelő ügyfélhez regisztrál, titok nem kerül URL-be, parancssorba vagy MSI-naplóba. Követelmény: AD-04, SEC-02. Korábbi azonosító: F1-13.
- **MIDAS-034:** A felhasználó látja a kapcsolat állapotát és érthető hibajelzést kap, a felület felhasználói jogon fut. Követelmény: UI-06, UI-08, PERF-04. Korábbi azonosító: F1-14.
- **MIDAS-035:** Legalább 1000 szimulált agent egyórás próbája dokumentálja a terhelést és az ügyfélszigetelést. Követelmény: PERF-06, SEC-01. Korábbi azonosító: F1-15.
- **MIDAS-036:** A kijelölt címtár érvényes tanúsítvánnyal, olvasási joggal és OU-határral elérhető, más ügyfélé nem. Követelmény: AD-01, AD-02. Korábbi azonosító: —.
- **MIDAS-037:** A jelölt különbözik az online agenttől, az objectGUID-alapú egyeztetés nem kapcsol össze más ügyfeleket. Követelmény: AD-03, AD-04. Korábbi azonosító: —.
- **MIDAS-038:** Az admin látja a jelölteket, kezelt gépeket és regisztrációs döntéseket, a felderítés nem telepít automatikusan. Követelmény: UI-01, AD-03, AD-04. Korábbi azonosító: —.
- **MIDAS-039:** Külön mérés készül a háttéragentről és megnyitott helyi felületről, a szerverprofil megkülönbözteti a grafikus és háttérképességeket. Legalább 24 órás agentmérés készül, Windows 11-en a kapcsolódó WebView2-folyamatokkal együtt. Az eredmény optimalizálási alap, nincs előírt CPU-, GPU-, memória- vagy adatforgalmi plafon. Követelmény: PERF-01, PERF-02, PERF-03, PERF-04, SERVER-03. Korábbi azonosító: —.
- **MIDAS-040:** A monitorozás, felderítés, regisztráció és terhelésmérés elfogadva; az összes vállalt feladat ellenőrzött Done, a támogatási mátrix és a következő fázis feltételei rögzítve. Tíz vegyes tesztgép egy hétig jelent; a 24 órás agent- és az egyórás, 1000 szimulált agentes mérés bizonyítéka csatolt. A MIDAS-018, MIDAS-019, MIDAS-023 és MIDAS-024 ellenőrzésének állapota rögzített; Done állapotuk nem feltétele a zárásnak. Követelmény: —. Korábbi azonosító: F1-16.

### 2. Távsegítés

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-041 | Felhasználó nélküli szerveres hozzáférés szabályzata | Backlog | P0 | MIDAS-011, MIDAS-031 |
| MIDAS-042 | DXGI-rögzítés és több monitor | Backlog | P0 | — |
| MIDAS-043 | NVIDIA hardveres kódoló integráció | Backlog | P0 | MIDAS-042 |
| MIDAS-044 | AMD hardveres kódoló integráció | Backlog | P0 | MIDAS-042 |
| MIDAS-045 | Intel hardveres kódoló integráció | Backlog | P0 | MIDAS-042 |
| MIDAS-046 | Kodekegyeztetés, szoftveres H.264 és adaptív képátvitel | Backlog | P0 | MIDAS-042 |
| MIDAS-047 | Védett transzport és külön UAC-komponens | Backlog | P0 | — |
| MIDAS-048 | Adminpanelről indított helyi jóváhagyás | Backlog | P0 | MIDAS-047, MIDAS-034, MIDAS-011, MIDAS-012 |
| MIDAS-049 | Normál távvezérlés működő UAC-val | Backlog | P0 | MIDAS-048, MIDAS-042, MIDAS-046 |
| MIDAS-050 | Rendszergazdai távvezérlés és külön engedélyek | Backlog | P0 | MIDAS-049 |
| MIDAS-051 | Helyi leállítás, lejárat és visszavonás | Backlog | P0 | MIDAS-049, MIDAS-050, MIDAS-041 |
| MIDAS-052 | TURN-elérés és tiltott célok tesztje | Backlog | P0 | MIDAS-047, MIDAS-006 |
| MIDAS-053 | Böngészős távsegítő nézet és eszköztár | Backlog | P0 | MIDAS-049, MIDAS-050, MIDAS-052, MIDAS-031 |
| MIDAS-054 | Fizikai billentyűk, szöveg, IME és kurzor | Backlog | P0 | MIDAS-047, MIDAS-053 |
| MIDAS-055 | Munkameneten belüli vágólap és fájlátvitel | Backlog | P1 | MIDAS-053, MIDAS-051 |
| MIDAS-056 | Chat az aktív távsegítésben | Backlog | P2 | MIDAS-053, MIDAS-034 |
| MIDAS-057 | Mobilos távvezérlési bevitel | Backlog | P2 | MIDAS-053, MIDAS-054, MIDAS-014 |
| MIDAS-058 | Windows-, UAC-, böngésző- és késleltetési mátrix | Backlog | P0 | MIDAS-051, MIDAS-043, MIDAS-044, MIDAS-045, MIDAS-053, MIDAS-054, MIDAS-055, MIDAS-056, MIDAS-057 |
| MIDAS-059 | A két résztvevős használhatósági próba valódi kapcsolattal | Backlog | P0 | MIDAS-058 |
| MIDAS-060 | 2. fázis ellenőrzése és mérföldkőzárás | Backlog | P0 | A fázis összes többi tétele |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-041:** Ügyfél- és gépszabályzat, jogosultság, indoklás és friss hitelesítés együtt kell, jelen lévő felhasználó asztala helyi jóváhagyást kér. Követelmény: SERVER-02, SERVER-04. Korábbi azonosító: F2-02.
- **MIDAS-042:** Monitorválasztás és kombinált kép tesztelve, asztalváltás után az előző célhoz tartozó bevitel eldobódik. Követelmény: REMOTE-03, REMOTE-06. Korábbi azonosító: F2-03.
- **MIDAS-043:** Támogatott NVIDIA-gépen egyeztetett kodekkel fut és mérhető a késleltetés, kódolóhiba kezelhető. Követelmény: PERF-04. Korábbi azonosító: F2-04.
- **MIDAS-044:** Támogatott AMD-gépen egyeztetett kodekkel fut és mérhető a késleltetés, kódolóhiba kezelhető. Követelmény: PERF-04. Korábbi azonosító: F2-04.
- **MIDAS-045:** Támogatott Intel-gépen egyeztetett kodekkel fut és mérhető a késleltetés, kódolóhiba kezelhető. Követelmény: PERF-04. Korábbi azonosító: F2-04.
- **MIDAS-046:** Virtuális kijelzőn hardveres kódoló nélkül vezérelhető az asztal, a használt bináris terjesztési feltételei ellenőrzöttek. Közös adaptív vezérlő igazítja a bitrátát, szükség szerint a felbontást és a képkockaszámot a becsült sávszélességhez, RTT-hez, veszteséghez és jitterhez. Romláskor csökkent, tartós javuláskor fokozatosan visszaemel, ingadozás és növekvő képkockasor nélkül. Szöveg- és mozgásprofil, hiányzó mérés, újracsatlakozás és hardveres/szoftveres kódoló is kezelt; a teljes hálózati mátrixot a MIDAS-058 igazolja. Követelmény: PERF-04, SERVER-03, PERF-07. Korábbi azonosító: F2-05.
- **MIDAS-047:** Normál segéd, transzport és emelt komponens külön identitással működik, más felhasználó és munkamenet hozzáférése elutasított. Követelmény: REMOTE-03, REMOTE-06. Korábbi azonosító: F2-06.
- **MIDAS-048:** Mindkét módnál név és képesség látszik, elutasított vagy lejárt kérést távoli bevitel és késő válasz nem aktivál. Követelmény: UI-04, UI-06, REMOTE-01, REMOTE-02. Korábbi azonosító: F2-07.
- **MIDAS-049:** A felhasználói alkalmazások saját jogukkal futnak, az engedélyezett UAC ugyanabban a kapcsolatban látható és kezelhető. Követelmény: REMOTE-03, REMOTE-04, REMOTE-05. Korábbi azonosító: —.
- **MIDAS-050:** A mód friss passkeyt és megfelelő jogot kér, meglévő felhasználói folyamat jogosultságát nem írja át. Követelmény: REMOTE-04, REMOTE-05, SEC-03. Korábbi azonosító: —.
- **MIDAS-051:** Helyi stop azonnal bont, visszavonás és jelzésvesztés közvetlen/TURN kapcsolaton is az előírt időben zár. Követelmény: REMOTE-07, SEC-03. Korábbi azonosító: —.
- **MIDAS-052:** Közvetlen és relézett út működik, rövid életű hitelesítés és IPv4/IPv6 belsőcím-tiltás tesztelve. Követelmény: SEC-05. Korábbi azonosító: F2-08.
- **MIDAS-053:** A tesztelt böngészőkben látszik a mód és a célgép, a kapcsolatjelző valós adatot ad, a támogatott rendszerbillentyűk elérhetők. Alapértelmezett automatikus minőség, szöveg- és mozgásprofil, tényleges felbontás/bitráta/képkockaszám, valamint érthető hálózatromlási jelzés látható. A hiányzó mérés nem jelenhet meg egészséges kapcsolatként. Követelmény: UI-02, UI-04, UI-07, PERF-07. Korábbi azonosító: F2-09.
- **MIDAS-054:** Magyar és eltérő kiosztás, egérmódosítók és SAS-kérés célgépre kötötten működik. Követelmény: REMOTE-08. Korábbi azonosító: F2-10.
- **MIDAS-055:** Engedélyezett irányokban folytatható fájlátvitel működik, tartalom- és vágólaptitok nem kerül auditba. Követelmény: REMOTE-08, SEC-04. Korábbi azonosító: F2-11.
- **MIDAS-056:** Az elfogadott munkamenet két résztvevője üzenetet vált, lezárás után a csatorna megszűnik. Követelmény: UI-06. Korábbi azonosító: F2-12.
- **MIDAS-057:** Valódi telefonon kattintás, másodlagos kattintás, nagyítás és szövegbevitel kipróbált. Követelmény: UI-08. Korábbi azonosító: F2-14.
- **MIDAS-058:** Konzol, RDP, zárolás, UAC-jelszókérés, normál/admin és szoftveres út mérése dokumentált, a támogatás az eredményt követi. Ismételhető hálózati profilokkal igazolt a sávszélesség csökkenése és visszaállása, a késleltetés, jitter és csomagvesztés hatása, valamint az útvonalváltás. Közvetlen/TURN, normál/admin, UAC és hardveres/szoftveres kódolás mellett mérni kell az adaptáció reakció- és helyreállási idejét, a tényleges képminőséget, az olvashatóságot és a bevitel látható válaszidejét; tartós minőségingadozás és növekvő képkockasor nem fogadható el. Követelmény: REMOTE-03, REMOTE-07, REMOTE-08, SERVER-03, PERF-07. Korábbi azonosító: —.
- **MIDAS-059:** Az adminpanelről induló elutasítás, új kérés elfogadása, UAC és helyi bontás önállóan követhető mindkét oldalon. Magyar és angol, billentyűzetes, mobil dashboardos és helyi képernyőolvasós próba is készül. Követelmény: UI-06, UI-07, UI-08. Korábbi azonosító: —.
- **MIDAS-060:** Mindkét távvezérlési mód normál és rendszergazdai fiókkal, UAC-val, helyi leállítással és külön szabályzatos szervereléréssel igazolt. A teljes mátrix és a két felület használhatósága elfogadva; minden vállalt feladat ellenőrzött Done. Követelmény: —. Korábbi azonosító: F2-15.

### 3. Automatizált karbantartás

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-061 | Verziózott szkriptek és aláírt futtatási felhatalmazás | Backlog | P0 | MIDAS-022, MIDAS-011, MIDAS-012 |
| MIDAS-062 | Ütemező, futásállapot és duplikációvédelem | Backlog | P0 | MIDAS-061 |
| MIDAS-063 | Karbantartási ablakok és szerver-újraindítási profil | Backlog | P0 | MIDAS-062, MIDAS-028 |
| MIDAS-064 | Windows Update jóváhagyás és újraindítás | Backlog | P0 | MIDAS-063 |
| MIDAS-065 | Gépszintű és felhasználói alkalmazásfrissítések | Backlog | P1 | MIDAS-063 |
| MIDAS-066 | apt- és dnf-frissítési ciklus | Backlog | P0 | MIDAS-063 |
| MIDAS-067 | Jóváhagyott MSI- és EXE-csomagok terítése | Backlog | P1 | MIDAS-062 |
| MIDAS-068 | Riasztáshoz kötött javító feladat | Backlog | P1 | MIDAS-062, MIDAS-029 |
| MIDAS-069 | Külön engedélyű távoli PowerShell és Linux-shell | Backlog | P1 | MIDAS-062, MIDAS-031 |
| MIDAS-070 | Önálló háttérfájlkezelő és folyamat-/szolgáltatásműveletek | Backlog | P1 | MIDAS-062, MIDAS-031 |
| MIDAS-071 | Tartományba léptetés előellenőrzése és OU-jogosultság | Backlog | P0 | MIDAS-036, MIDAS-038, MIDAS-062 |
| MIDAS-072 | Védett offline domain join csomag | Backlog | P0 | MIDAS-071 |
| MIDAS-073 | Domain trust és visszatérés ellenőrzése | Backlog | P0 | MIDAS-072, MIDAS-063 |
| MIDAS-074 | 3. fázis ellenőrzése és mérföldkőzárás | Backlog | P0 | A fázis összes többi tétele |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-061:** Módosított, lejárt vagy más ügyfélhez tartozó feladat elutasított, az identitás és paraméter auditált. Követelmény: SEC-03. Korábbi azonosító: F3-01.
- **MIDAS-062:** Megszakítás és újrakézbesítés után a futás állapota egyeztethető, ugyanaz a nem ismételhető művelet nem indul duplán. Követelmény: UI-05, PERF-03, SEC-03. Korábbi azonosító: F3-02.
- **MIDAS-063:** A feladat a jóváhagyott időben fut, a halasztás és újraindítás eredménye gépenként követhető. Követelmény: SERVER-04. Korábbi azonosító: F3-03.
- **MIDAS-064:** Próbacsoporton a jóváhagyott javítás települ, értesítés és újraindítás ellenőrzött. Követelmény: SERVER-01, SERVER-04. Korábbi azonosító: F3-04.
- **MIDAS-065:** A megfelelő futtatási környezetet választja, a WinGet-elérés hiánya látható, jóváhagyott csomagút kipróbált. Követelmény: SERVER-01. Korábbi azonosító: F3-05.
- **MIDAS-066:** Debian/Ubuntu és Rocky Linux gépeken az ablak és jóváhagyás szerint fut, újraindítási igény visszaérkezik. Követelmény: SERVER-01. Korábbi azonosító: F3-06.
- **MIDAS-067:** Ellenőrzött csomag kiválasztott csoportra települ, a sikertelen futások és kilépési kódok látszanak. Követelmény: SEC-03. Korábbi azonosító: F3-07.
- **MIDAS-068:** A jóváhagyott szkript riasztásra lefut, ismétlődése kontrollált és eredménye a riasztáshoz kapcsolódik. Követelmény: PERF-03. Korábbi azonosító: F3-08.
- **MIDAS-069:** Választott futtatási identitás, hatókör, megszakítás és audit működik grafikus munkamenet nélkül. Követelmény: UI-02, SERVER-01, SEC-01. Korábbi azonosító: F2-13.
- **MIDAS-070:** A kiválasztott identitás és célhatókör érvényesül, a gép műveleti eredménye látszik és auditált. Követelmény: UI-02, SERVER-01, SEC-01. Korábbi azonosító: F2-13.
- **MIDAS-071:** A célgép, tartomány és OU előnézete, DNS/idő/kiadás ellenőrzése és külön jogosultsága működik. Követelmény: AD-01, AD-05. Korábbi azonosító: —.
- **MIDAS-072:** A connector külön írási identitással provisionál, csak a célagent kapja meg titkosítva, a tárolt másolat élettartama kezelt. Követelmény: AD-06, SEC-03. Korábbi azonosító: —.
- **MIDAS-073:** Jóváhagyott újraindítás után a tagság és az agent visszatérése igazolt, bizonytalan kimenetel nem okoz vak ismétlést. Követelmény: AD-07, SERVER-04. Korábbi azonosító: —.
- **MIDAS-074:** A szkriptek, frissítések, önálló háttéreszközök és ellenőrzött Windows-tartományba léptetés elfogadva. A hibák, újrapróbálás és újraindítás kezelése igazolt; minden vállalt feladat ellenőrzött Done. Követelmény: —. Korábbi azonosító: F3-09.

### 4. Megerősítés és pilot

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-075 | Linux-szervertelepítő és ellenőrzött csomagtárolók | Backlog | P0 | MIDAS-016 |
| MIDAS-076 | Windows-szervertelepítő és szolgáltatásfiókok | Backlog | P0 | MIDAS-016 |
| MIDAS-077 | Védett kódaláírás és telepítési reputációs próba | Backlog | P0 | MIDAS-075, MIDAS-076, MIDAS-033 |
| MIDAS-078 | Aláírt agentönfrissítés, gyűrűk és visszaállás | Backlog | P0 | MIDAS-077 |
| MIDAS-079 | Külső ellenőrzőpont és auditlánc | Backlog | P1 | MIDAS-012 |
| MIDAS-080 | Mentés és teljes visszaállítás másik gépen | Backlog | P0 | MIDAS-075, MIDAS-076, MIDAS-022 |
| MIDAS-081 | SBOM, kiadási aláírás és buildigazolás | Backlog | P1 | MIDAS-004, MIDAS-077 |
| MIDAS-082 | Ügyfél- és globális vészleállítás | Backlog | P0 | MIDAS-051, MIDAS-062, MIDAS-011 |
| MIDAS-083 | Független biztonsági felülvizsgálat és javítás | Backlog | P0 | MIDAS-082, MIDAS-079, MIDAS-081, MIDAS-080, MIDAS-078 |
| MIDAS-084 | Ügyfelenkénti adatkezelés és megőrzési szabályok | Backlog | P1 | MIDAS-079, MIDAS-080 |
| MIDAS-085 | Platform-, terhelési és használhatósági regresszió | Backlog | P0 | MIDAS-078, MIDAS-058, MIDAS-059, MIDAS-035, MIDAS-039 |
| MIDAS-086 | Első ügyfél bevezetése és pilotüzem | Backlog | P0 | MIDAS-083, MIDAS-084, MIDAS-085 |
| MIDAS-087 | 4. fázis ellenőrzése és mérföldkőzárás | Backlog | P0 | A fázis összes többi tétele |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-075:** Debian, Ubuntu és Rocky Linux tiszta telepítésen és frissítésen végigmegy a teljes stackkel. Követelmény: SEC-06. Korábbi azonosító: F4-01.
- **MIDAS-076:** Windows Server 2022/2025 tiszta gépen a MIDAS, PostgreSQL és eturnal együtt települ és frissül. Követelmény: SEC-06. Korábbi azonosító: F4-02.
- **MIDAS-077:** Saját Windows-binárisok aláírtak és időbélyegzettek, kulcs a védett aláíróban, a valós terjesztési út kipróbált. Követelmény: SEC-06. Korábbi azonosító: F4-03.
- **MIDAS-078:** Hibás csomag elutasított, hibás tesztkiadás engedélyezett korábbi verzióra áll vissza és nem terjed tovább. Követelmény: SEC-06. Korábbi azonosító: F4-04.
- **MIDAS-079:** Átírás, törlés és naplóvég-csonkolás külső bizonyítékkal kimutatható, titkok kizárása ellenőrzött. Követelmény: SEC-04. Korábbi azonosító: F4-05.
- **MIDAS-080:** Adat, konfiguráció és kulcshelyreállítás kipróbált, a visszavont agent helyreállítás után is kizárt. Követelmény: SEC-02, SEC-06. Korábbi azonosító: F4-06.
- **MIDAS-081:** A kiadás forrása, függőségei és aláírásai letölthetők és ellenőrizhetők. Követelmény: SEC-06. Korábbi azonosító: F4-07.
- **MIDAS-082:** A választott hatókör új műveleteit felfüggeszti és aktív felhatalmazásait visszavonja, az audit megmarad. Követelmény: SEC-06. Korábbi azonosító: —.
- **MIDAS-083:** Fenyegetésmodell és külső vizsgálat alapján a kritikus/magas találatok javítottak, visszateszteltek. Követelmény: SEC-01, SEC-02, SEC-03, SEC-04, SEC-05, SEC-06. Korábbi azonosító: F4-08.
- **MIDAS-084:** A pilot adatkezelési szerepei, megőrzése, hozzáférése és alkalmazandó kötelezettségei dokumentáltak. Követelmény: —. Korábbi azonosító: F4-09.
- **MIDAS-085:** A deklarált támogatási mátrix, műveleti korlátok és korábbi használhatósági próbák ismét sikerülnek. Követelmény: PERF-03, UI-08, REMOTE-07. Korábbi azonosító: —.
- **MIDAS-086:** Javított biztonsági találatok után bevezetett ügyfél egy hónapos incidensmentes próbaüzeme dokumentált. Követelmény: —. Korábbi azonosító: F4-10.
- **MIDAS-087:** A biztonsági és visszaállítási próbák elfogadva; a kritikus és magas kockázatú hibák javítva; az első ügyfél egy hónapos incidensmentes pilotja lezárva. Minden vállalt feladat ellenőrzött Done. Követelmény: —. Korábbi azonosító: F4-11.

### 5. Bővítések

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-088 | Linux grafikus távsegítés X11-en | Backlog | P1 | MIDAS-053 |
| MIDAS-089 | Wayland távsegítés és portálengedélyek | Backlog | P1 | MIDAS-088 |
| MIDAS-090 | Telepítés nélküli, attended gyorssegítség | Backlog | P1 | MIDAS-077, MIDAS-053, MIDAS-048 |
| MIDAS-091 | WASAPI/Opus hangátvitel | Backlog | P2 | MIDAS-053 |
| MIDAS-092 | Szabályozott felvétel és hitelesítési felületek kizárása | Backlog | P2 | MIDAS-084, MIDAS-053 |
| MIDAS-093 | Entra ID és AD FS OIDC-belépés | Backlog | P1 | MIDAS-009, MIDAS-011 |
| MIDAS-094 | PWA Web Push valódi mobilokon | Backlog | P1 | MIDAS-014, MIDAS-029 |
| MIDAS-095 | Ügyfélriport és ütemezett export | Backlog | P2 | MIDAS-062, MIDAS-031, MIDAS-079 |
| MIDAS-096 | 5. fázis ellenőrzése és mérföldkőzárás | Backlog | P0 | A fázis összes többi tétele |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-088:** X11 gépen helyi jóváhagyással, bevitelkezeléssel és leállítással végigmegy a kapcsolat. Követelmény: REMOTE-01, REMOTE-07. Korábbi azonosító: F5-01.
- **MIDAS-089:** A kijelölt Wayland környezetben a portál, rögzítés, bevitel és jogosultságok tényleges gépen kipróbáltak. Követelmény: REMOTE-01, REMOTE-07. Korábbi azonosító: F5-01.
- **MIDAS-090:** A rövid kód után az admin kérése helyben jóváhagyható, kilépés lezárja az ideiglenes komponenseket és felhatalmazást. Követelmény: REMOTE-01, REMOTE-07. Korábbi azonosító: F5-02.
- **MIDAS-091:** Engedélyezett hang szinkronban hallható, kikapcsolás és lezárás megszünteti az adatfolyamot. Követelmény: SEC-05. Korábbi azonosító: F5-03.
- **MIDAS-092:** Az engedélyezett felvétel megőrzése, hozzáférése és UAC/bejelentkezési kizárása a pufferre is ellenőrzött. Követelmény: REMOTE-08, SEC-04. Korábbi azonosító: F5-04.
- **MIDAS-093:** Megbízható issuer+subject, callbackkorreláció és csoporthatókör tesztelve, ügyfelek nem keverednek. Követelmény: SEC-01. Korábbi azonosító: F5-05.
- **MIDAS-094:** Jóváhagyott feliratkozás értesít Androidon és iPhone-on a támogatott telepítési környezetben. Követelmény: UI-08. Korábbi azonosító: F5-06.
- **MIDAS-095:** A kiválasztott ügyfél riportja saját adatokból készül, PDF és ütemezett kézbesítés ellenőrzött. Követelmény: SEC-01. Korábbi azonosító: F5-07.
- **MIDAS-096:** A mérföldkő minden vállalt bővítésének önálló elfogadási, jogosultsági és regressziós próbája sikeres, dokumentációja kész; minden vállalt feladat ellenőrzött Done. Külön el nem fogadott javaslat nem zárási feltétel. Követelmény: —. Korábbi azonosító: —.

### Külön javaslatok – mérföldkő nélkül

Ezek még nem vállalt funkciók, mind Backlogban maradnak. Külön döntés nélkül nem kerülnek Readybe és nem zárási feltételek.

| ID | Feladat | Induláskor | Prioritás | Közvetlen függőségek az előző kapun felül |
| --- | --- | --- | --- | --- |
| MIDAS-097 | Ticketing, ügyfélportál és felhasználói segítségkérés | Backlog | P2 | — |
| MIDAS-098 | Windows LAPS-integráció | Backlog | P2 | — |
| MIDAS-099 | Időszakos jogosultság és két kezelő jóváhagyása | Backlog | P2 | — |
| MIDAS-100 | Biztonsági karantén és szabályozott feloldás | Backlog | P2 | — |
| MIDAS-101 | Csak megtekintés távsegítségi mód | Backlog | P2 | — |
| MIDAS-102 | Flottaszintű megfelelőségi nézet | Backlog | P2 | — |

**Elfogadási feltételek és visszakereshetőség**

- **MIDAS-097:** Csak külön scope-döntés után tervezhető. Előbb a ticketing és a portál kapcsolata, jogosultságai és adatkezelése tisztázandó; a jelenlegi agentben nincs „Segítséget kérek” funkció. Követelmény: —. Korábbi azonosító: F5-08, F1-14.
- **MIDAS-098:** Külön döntés és jogosultsági terv után: rövid ideig elérhető helyi adminhitelesítő, audit, naplóból és felvételből kizárt titok. Követelmény: —. Korábbi azonosító: —.
- **MIDAS-099:** Külön döntés után: lejáró, célgépre és műveletre szűkített jog, veszélyes műveletnél két külön kezelő engedélye. Követelmény: —. Korábbi azonosító: —.
- **MIDAS-100:** Külön döntés után: ügyfelenként engedélyezett hálózati elkülönítés, működő helyi helyreállítás és auditált feloldás. Követelmény: —. Korábbi azonosító: —.
- **MIDAS-101:** Külön döntés után: egyértelmű helyi tájékoztatás, bevitel tiltása és külön jóváhagyás a vezérlésre váltás előtt. Követelmény: —. Korábbi azonosító: —.
- **MIDAS-102:** Külön döntés után: agentfrissítési gyűrűk, BitLocker, Defender, javítások és függő újraindítások ügyfélszigetelt áttekintése; az adat frissessége és a hiányzó adat látszik. Követelmény: —. Korábbi azonosító: —.

### Terjedelemváltozások

**2026. november 7. · A MIDAS-018 és a MIDAS-019 M0-ból M1-be került**

- **Döntés és ok:** a projektgazda döntése. A két hiányzó próba külső erőforrást igényel: Windows 11-es tesztgépet standard fiókkal, illetve új résztvevőket és képernyőolvasót futtató eszközöket. Ezek most nem érhetők el, a 0. fázis többi eredménye viszont kész és ellenőrzött.
- **Elkészült rész:** mindkét feladat megvalósítása és fejlesztői ellenőrzése kész (0f3fb16, 982f1ee). Az elfogadási feltétel nem változott; a feladatok felülvizsgálat alatt maradnak, amíg a próba eredménye rögzítésre nem kerül.
- **Függőségek:** a két feladat megtartja eredeti függőségeit, és nem függ a MIDAS-020-tól, mert a munkájuk még M0-ban indult. A MIDAS-020 nem vár rájuk, a MIDAS-040 igen. A MIDAS-034 a MIDAS-019-től, a MIDAS-042 és a MIDAS-047 a MIDAS-018-tól közvetlenül függ, így ezek csak a próbák elfogadása után indulhatnak.
- **Hatás:** a 0. fázis lezárható, és az 1. fázis többi feladata a szokásos feltételekkel indulhat. A függőségeket a következő bejegyzés módosította.

**2026. november 7. · A projektgazdai ellenőrzésre váró MIDAS-018, MIDAS-019 és MIDAS-020 nem akadályozza a későbbi munkát**

- **Döntés és ok:** a projektgazda döntése. A három feladat az ő saját ellenőrzésére vár: a MIDAS-018 a Windows 11-es tesztgépes UAC-próbára standard fiókkal, a MIDAS-019 a résztvevős, billentyűzetes és képernyőolvasós próbára, a MIDAS-020 a WebRTC-döntés (ADR 0002) jóváhagyására. Ezek most nem végezhetők el, a fejlesztést viszont az ütemterv miatt folytatni kell.
- **Szabály:** a három feladat In review állapotban marad, és csak a projektgazdai ellenőrzés után léphet Done állapotba. Az ellenőrzés kötelező marad, de a Done állapotuk nem feltétele későbbi feladat vagy fázis indításának, sem fáziskapu zárásának. A kivétel csak erre a három feladatra vonatkozik; minden más feladatra és kapura a teljes státuszmenet és a fázisszabály változatlan.
- **Kivett függőségek:** az M1 tételeinek (MIDAS-021–040) függése a MIDAS-020-tól; a MIDAS-034, MIDAS-040 és MIDAS-059 függése a MIDAS-019-től; a MIDAS-040, MIDAS-042 és MIDAS-047 függése a MIDAS-018-tól.
- **Kockázat és teendő:** a későbbi munka még nem igazolt UAC-, használhatósági és WebRTC-feltevésekre épülhet. Sikertelen ellenőrzésnél a MIDAS-034, MIDAS-042, MIDAS-047, MIDAS-059 és az ezekre épülő munka újravizsgálandó, szükség esetén újranyitandó.

**2026. november 7. · A kivétel kiterjesztése a MIDAS-023-ra és a MIDAS-024-re**

- **Döntés és ok:** a projektgazda döntése. A Windows-szolgáltatás (MIDAS-023) és a Linux-csomag (MIDAS-024) rendszerszintű próbáját a projektgazda saját gépen végzi: telepítés, újraindítás, más munkamenet, illetve a három disztribúció. Ez most nem történik meg, a fejlesztést viszont folytatni kell.
- **Szabály:** ugyanaz, mint a MIDAS-018–020-nál. A két feladat In review marad, és csak a próba után léphet Done állapotba; Done állapotuk nem feltétele későbbi feladat vagy fázis indításának, sem fáziskapu zárásának. A kivétel összesen erre az öt feladatra vonatkozik.
- **Kivett függőségek:** a MIDAS-025, -027, -033, -034, -040, -041, -042, -047 és -078 függése a MIDAS-023-tól; a MIDAS-025, -027, -039, -040 és -078 függése a MIDAS-024-től.
- **Kockázat és teendő:** sikertelen próbánál az érintett feladatokat és az ezekre épülő munkát újra kell vizsgálni, szükség esetén újra kell nyitni.

### Lényeges változások a régi vázlathoz képest

- F2-01: a korai WebRTC-megvalósíthatóság M0 (MIDAS-017), az UAC-próba 2026. november 7. óta M1 (MIDAS-018); a kész vezérlés M2.
- F1-14: kapcsolatállapot és tálcaikon M1 (MIDAS-034); a segítségkérés csak a MIDAS-097 javaslat része.
- F2-02: külön ügyfél- és gépszabályzatos unattended szerverelérés M2 (MIDAS-041).
- F2-04: NVIDIA, AMD és Intel külön megvalósítási és mérési tétel (MIDAS-043, MIDAS-044, MIDAS-045).
- F2-13: önálló terminál és fájlkezelő M3 (MIDAS-069, MIDAS-070); munkameneten belüli fájlátvitel M2 (MIDAS-055).
- F5-08: ticketing, ügyfélportál és felhasználói segítségkérés mérföldkő nélküli javaslat (MIDAS-097).
- Új külön tételek: két felület használhatósága, UAC-jogosultság és visszavonás, AD-connector és felderítés, delegált Windows domain join, erőforrásmérés, vészleállítás és regresszió.
- Minden fázisnak saját zárófeladata van. Az alapműködés és a kötelező biztonsági feltételek a javaslatoktól függetlenül elkészülnek.

## English

October 31, 2026 · Published initial allocation. Track current state in the [GitHub Project](https://github.com/users/Shran21/projects/4).

**97 scheduled items**: 91 implementation/preparation tasks and 6 phase closures. **6 further proposals** have no milestone. All 74 earlier task identifiers are traceable below; splitting and moving work preserves its intent subject to the reviewed requirements.

[backlog.json](backlog.json) contains the same IDs, bilingual titles and acceptance criteria, dependencies, priority, initial status and milestone. Applicable [requirements](kovetelmenyek.md#english) and [phase acceptance trials](rendszerterv.md#roadmap) remain mandatory in addition to the criteria below.

MIDAS-103 was added to M0 after the first implementation round: component directories and an operator preview with two themes. The initial status totals below describe the original 102-item import.

On November 7, 2026 MIDAS-018 and MIDAS-019 moved from M0 to M1. The same day the project owner removed the Done status of MIDAS-018, MIDAS-019 and MIDAS-020 from the conditions for later work. See [scope changes](#scope-changes) for the reason and effect.

### Initial statuses

Create every new item in **Backlog**. “At kickoff” means its planned state after import and preparation checks. Promote Ready candidates through separate read-back transitions; imports must not fabricate history.

| Status | On import | Planned after preparation |
| --- | --- | --- |
| Backlog | 102 | 99 |
| Ready | 0 | 3 |
| In progress | 0 | 0 |
| In review | 0 | 0 |
| Done | 0 | 0 |

Kickoff Ready candidates: **MIDAS-001** (development foundations), **MIDAS-002** (repository and Project preparation), **MIDAS-005** (guides and decision records). Missing required access leaves the affected item in Backlog. No implementation is declared Done in advance.

### Milestones

M0–M5 are local keys, not GitHub milestone numbers. The publication fields in backlog.json record the target repository, Project, actual milestone IDs and issue links.

| Key | Milestone | Items including gate | Closure task | Previous gate |
| --- | --- | --- | --- | --- |
| M0 | 0. Foundations | 19 | MIDAS-020 | — |
| M1 | 1. Agent and monitoring | 22 | MIDAS-040 | MIDAS-020 |
| M2 | 2. Remote support | 20 | MIDAS-060 | MIDAS-040 |
| M3 | 3. Automated maintenance | 14 | MIDAS-074 | MIDAS-060 |
| M4 | 4. Hardening and pilot | 13 | MIDAS-087 | MIDAS-074 |
| M5 | 5. Extensions | 9 | MIDAS-096 | MIDAS-087 |

All committed items in later phases additionally depend on the previous gate being Done, except that M1 items do not depend on MIDAS-020 since November 7, 2026. Tables show other direct dependencies without repeating that gate in every row; JSON lists every dependency explicitly. Independent ready tasks within the same phase may run in parallel.

P0: core behavior, security or gates needing precedence; P1: work ordered afterwards within the phase; P2: lower ordering priority or a separate proposal. Item kind and milestone assignment determine commitment, not priority. Priority does not override phases or dependencies. Each closure task comes last within its milestone.

### 0. Foundations

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-001 | Monorepo and shared development commands | Ready | P0 | — |
| MIDAS-002 | Prepare the repository and project for the reviewed plan | Ready | P0 | — |
| MIDAS-003 | Configure project views and task fields | Backlog | P0 | MIDAS-002 |
| MIDAS-004 | Linux/Windows CI, dependency and secret checks | Backlog | P0 | MIDAS-001, MIDAS-002 |
| MIDAS-005 | Security/contribution guidance and decision records | Ready | P1 | — |
| MIDAS-006 | Server skeleton, TLS, ACME and observability | Backlog | P0 | MIDAS-001 |
| MIDAS-007 | Tenant model, migrations and PostgreSQL RLS | Backlog | P0 | MIDAS-001, MIDAS-006 |
| MIDAS-008 | Passkeys, password+TOTP and recovery | Backlog | P0 | MIDAS-006, MIDAS-007 |
| MIDAS-009 | Browser sessions, CSRF and revocation | Backlog | P0 | MIDAS-008 |
| MIDAS-010 | Trusted local AD sign-in and group mapping | Backlog | P1 | MIDAS-008, MIDAS-007 |
| MIDAS-011 | Roles, overrides and target scope | Backlog | P0 | MIDAS-008, MIDAS-007 |
| MIDAS-012 | Append-only baseline audit | Backlog | P1 | MIDAS-007, MIDAS-008 |
| MIDAS-013 | Bilingual dashboard shell and shared components | Backlog | P0 | MIDAS-009, MIDAS-011, MIDAS-103 |
| MIDAS-014 | Mobile layout, focus handling and PWA foundations | Backlog | P1 | MIDAS-013 |
| MIDAS-015 | Customer, site and user management | Backlog | P0 | MIDAS-013, MIDAS-011, MIDAS-012 |
| MIDAS-016 | Single-use setup on the final HTTPS origin | Backlog | P0 | MIDAS-006, MIDAS-008, MIDAS-013 |
| MIDAS-017 | Early WebRTC and encoder proof | Backlog | P0 | MIDAS-001, MIDAS-006 |
| MIDAS-103 | Operator console preview and component directory layout | Backlog | P0 | MIDAS-001, MIDAS-002 |
| MIDAS-020 | Review phase 0 and close its milestone | Backlog | P0 | All other phase items |

**Acceptance criteria and traceability**

- **MIDAS-001:** A clean clone builds and runs the documented formatting and checks on Windows and Linux. Requirements: —. Legacy ID: F0-01.
- **MIDAS-002:** The confirmed target repository contains bilingual plans, license and templates, with correct project and milestone links. Requirements: —. Legacy ID: —.
- **MIDAS-003:** Project views show the same milestone, priority and label values as their issues. New issues can be linked without creating duplicate project items. Requirements: —. Legacy ID: —.
- **MIDAS-004:** CI rejects broken changes. After local checks, changes go directly to main without development branches or PRs; acceptance requires successful Windows, Linux and security checks on that exact commit. Pre-push required CI checks are disabled; linear history and prohibitions on force pushes and main deletion remain enabled. Requirements: SEC-06. Legacy ID: F0-02.
- **MIDAS-005:** Published guidance covers private vulnerability reporting, contribution requirements, licensing and Rust/Tauri/WebRTC decisions. Requirements: SEC-06. Legacy ID: F0-03.
- **MIDAS-006:** HTTPS, health endpoint and logging work, with distinct enrollment and operational mTLS routes. Requirements: SEC-02. Legacy ID: F0-04.
- **MIDAS-007:** Negative tests isolate two tenants with a faulty application filter and reused database connections. Requirements: SEC-01. Legacy ID: F0-05.
- **MIDAS-008:** Passkey user verification, fallback authentication, single-use recovery and rate limiting are tested. Requirements: SEC-01. Legacy ID: F0-06.
- **MIDAS-009:** Revoked sessions cannot invoke APIs or WebSockets, and mutations are protected against CSRF. Requirements: SEC-01, SEC-03. Legacy ID: F0-07.
- **MIDAS-010:** A test directory permits valid users and rejects disabled accounts and invalid LDAPS certificates. Requirements: AD-01, SEC-01. Legacy ID: F0-08.
- **MIDAS-011:** Denial precedence, tenant boundaries and normal/admin/UAC/unattended permission checks pass negative tests. Requirements: SEC-01, REMOTE-04. Legacy ID: F0-09.
- **MIDAS-012:** Sign-ins, permission changes and request decisions are searchable without credentials. Requirements: SEC-04. Legacy ID: F0-10.
- **MIDAS-013:** After sign-in, tenant context, navigation and states work in both languages and themes. Requirements: UI-01, UI-07, UI-08. Legacy ID: F0-11.
- **MIDAS-014:** Primary pages support keyboard/touch and the service worker caches only static UI assets. Requirements: UI-03, UI-08. Legacy ID: F0-12.
- **MIDAS-015:** An admin creates a tenant and scoped technician, and grants/denials apply within target scope. Requirements: UI-01, SEC-01. Legacy ID: F0-13.
- **MIDAS-016:** The first admin enrolls a passkey at the final origin and consumed setup authorization becomes invalid. Requirements: SEC-01, SEC-06. Legacy ID: F0-14.
- **MIDAS-017:** A browser displays a real Windows desktop and measurements determine the WebRTC integration. The prototype verifies network measurement access and runtime bitrate, resolution and frame-rate controls, recording adaptation under degrading and recovering network conditions. Requirements: PERF-04, PERF-07. Legacy ID: F2-01.
- **MIDAS-103:** Server code lives in server/, the web panel in web-panel/, and platform agents in agent/agent-windows/ and agent/agent-linux/ in one repository. Documentation and shared commands use the new paths; Windows and Linux builds pass. The operator preview provides light/dark themes, Hungarian/English text, navigation, searchable and filterable devices and device details. Theme and language choices persist after reload. Sample data is clearly labeled and the preview performs no remote operations. Actual server connection status is separate. Narrow layouts and keyboard interaction are verified. Requirements: UI-08. Legacy IDs: —.
- **MIDAS-020:** All committed phase work is reviewed Done; inspect phase evidence, status histories and next-phase prerequisites. Record the WebRTC decision and the state of the UAC and usability trials moved to M1. Requirements: —. Legacy ID: F0-15.

### 1. Agent and monitoring

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-018 | Normal-session UAC and protected input proof | Backlog | P0 | MIDAS-017 (moved from M0, no gate dependency) |
| MIDAS-019 | Admin-panel and local-consent usability prototype | Backlog | P0 | MIDAS-013, MIDAS-014 (moved from M0, no gate dependency) |
| MIDAS-021 | Versioned agent protocol and reconnection | Backlog | P0 | MIDAS-006 |
| MIDAS-022 | Agent enrollment, approval, mTLS and revocation | Backlog | P0 | MIDAS-021, MIDAS-011, MIDAS-012 |
| MIDAS-023 | Windows service and protected local IPC | Backlog | P0 | MIDAS-022 |
| MIDAS-024 | Linux agent, systemd and packages | Backlog | P0 | MIDAS-022 |
| MIDAS-025 | Heartbeat, metrics, rollups and retention | Backlog | P0 | — |
| MIDAS-026 | Incremental hardware and software inventory | Backlog | P1 | MIDAS-025 |
| MIDAS-027 | Health checks and platform capabilities | Backlog | P1 | — |
| MIDAS-028 | Inherited customer, site and group policies | Backlog | P1 | MIDAS-026, MIDAS-011 |
| MIDAS-029 | Alerts, deduplication and email | Backlog | P0 | MIDAS-025, MIDAS-027, MIDAS-028 |
| MIDAS-030 | Teams and Slack notification endpoints | Backlog | P2 | MIDAS-029 |
| MIDAS-031 | Device pages, cards, context menus and live state | Backlog | P0 | MIDAS-013, MIDAS-025, MIDAS-026, MIDAS-029 |
| MIDAS-032 | Permission-aware search and command palette | Backlog | P2 | MIDAS-031 |
| MIDAS-033 | Windows MSI and protected enrollment delivery | Backlog | P0 | — |
| MIDAS-034 | Agent status view and tray icon | Backlog | P1 | — |
| MIDAS-035 | Fleet load and reconnection testing | Backlog | P0 | MIDAS-031, MIDAS-025 |
| MIDAS-036 | Per-customer read-only AD connector | Backlog | P0 | MIDAS-010, MIDAS-022 |
| MIDAS-037 | AD discovery and verified agent matching | Backlog | P0 | MIDAS-036, MIDAS-026 |
| MIDAS-038 | Discovery and enrollment view | Backlog | P0 | MIDAS-037, MIDAS-031 |
| MIDAS-039 | 24-hour background profiling and server profiles | Backlog | P0 | MIDAS-034, MIDAS-027 |
| MIDAS-040 | Review phase 1 and close its milestone | Backlog | P0 | All other phase items except MIDAS-018, MIDAS-019, MIDAS-023 and MIDAS-024 |

**Acceptance criteria and traceability**

- **MIDAS-018:** A browser controls real secure-desktop UAC for a standard user with local termination and security policies enabled. Requirements: REMOTE-03, REMOTE-05, REMOTE-06, REMOTE-07. Legacy ID: —.
- **MIDAS-019:** The operator initiates, a new user understands/declines, then accepts a new request and ends it, including keyboard testing. Document mobile admin-panel and screen-reader trials on the local consent dialog, in both languages. Requirements: UI-04, UI-06, UI-07, UI-08. Legacy ID: —.
- **MIDAS-021:** Supported protocol versions interoperate and an agent reconnects after connection loss. Requirements: PERF-01. Legacy ID: F1-01.
- **MIDAS-022:** First enrollment uses server-authenticated TLS and operations require mTLS. Reused tokens, wrong targets and revoked certificates are rejected. Requirements: SEC-02. Legacy ID: F1-02.
- **MIDAS-023:** The service recovers from reboot/network loss and rejects unauthorized processes and other sessions on IPC. Requirements: PERF-05, SERVER-01, REMOTE-06. Legacy ID: F1-03.
- **MIDAS-024:** Packages start a tenant-bound, restricted service on Debian, Ubuntu and Rocky Linux. Requirements: SERVER-01. Legacy ID: F1-04.
- **MIDAS-025:** Three missed heartbeats mark offline, buffered data returns safely and rollups/retention cleanup are verified. Requirements: PERF-02, PERF-05. Legacy ID: F1-05.
- **MIDAS-026:** Windows/Linux inventory updates on changes and persists successful reconciliation time across restarts. Requirements: PERF-02. Legacy ID: F1-06.
- **MIDAS-027:** Service, update, Defender, BitLocker and storage cases are exercised and unavailable checks are labelled. Requirements: SERVER-01, UI-05. Legacy ID: F1-07.
- **MIDAS-028:** An override affects only its scoped group and the device shows policy provenance. Requirements: UI-01, SEC-01. Legacy ID: F1-08.
- **MIDAS-029:** An outage produces one actionable alert/notification and acknowledgement is audited. Requirements: UI-07. Legacy ID: F1-09.
- **MIDAS-030:** A test alert reaches the scoped channel and the webhook secret is protected. Requirements: SEC-01. Legacy ID: F1-10.
- **MIDAS-031:** Menus and pages share permission rules, target identity stays visible and offline capabilities are clear. Requirements: UI-01, UI-02, UI-03, UI-05, UI-07, SERVER-04. Legacy ID: F1-11.
- **MIDAS-032:** Search only returns accessible devices/actions and supports keyboard access. Requirements: UI-03, UI-08. Legacy ID: F1-12.
- **MIDAS-033:** Manual and managed deployment enroll the right tenant without secrets in URLs, process arguments or MSI logs. Requirements: AD-04, SEC-02. Legacy ID: F1-13.
- **MIDAS-034:** The user sees connection status and actionable errors in a user-level interface. Requirements: UI-06, UI-08, PERF-04. Legacy ID: F1-14.
- **MIDAS-035:** A one-hour run of at least 1000 simulated agents documents load and tenant isolation. Requirements: PERF-06, SEC-01. Legacy ID: F1-15.
- **MIDAS-036:** A connector reads its designated directory with valid TLS and OU scope and cannot access another tenant's directory. Requirements: AD-01, AD-02. Legacy ID: —.
- **MIDAS-037:** Candidates remain distinct from live agents and objectGUID matching does not cross tenants. Requirements: AD-03, AD-04. Legacy ID: —.
- **MIDAS-038:** The admin sees candidates, managed devices and enrollment decisions, while discovery triggers no automatic installation. Requirements: UI-01, AD-03, AD-04. Legacy ID: —.
- **MIDAS-039:** Separate measurements cover background and open UI, and server profiles distinguish graphical and background capabilities. Run at least a 24-hour agent measurement, including related WebView2 processes on Windows 11. Results guide optimization without preset CPU, GPU, memory or traffic ceilings. Requirements: PERF-01, PERF-02, PERF-03, PERF-04, SERVER-03. Legacy ID: —.
- **MIDAS-040:** Accept monitoring, discovery, enrollment and load evidence; all committed work is reviewed Done, with a support matrix and next-phase prerequisites recorded. Ten mixed test devices report for one week; attach evidence for the 24-hour agent and one-hour 1000-simulated-agent measurements. Record the verification state of MIDAS-018, MIDAS-019, MIDAS-023 and MIDAS-024; their Done status is not a closure condition. Requirements: —. Legacy ID: F1-16.

### 2. Remote support

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-041 | Unattended server-access policy | Backlog | P0 | MIDAS-011, MIDAS-031 |
| MIDAS-042 | DXGI capture and multiple monitors | Backlog | P0 | — |
| MIDAS-043 | NVIDIA hardware encoder integration | Backlog | P0 | MIDAS-042 |
| MIDAS-044 | AMD hardware encoder integration | Backlog | P0 | MIDAS-042 |
| MIDAS-045 | Intel hardware encoder integration | Backlog | P0 | MIDAS-042 |
| MIDAS-046 | Codec negotiation, software H.264 and adaptive video | Backlog | P0 | MIDAS-042 |
| MIDAS-047 | Protected transport and separate UAC component | Backlog | P0 | — |
| MIDAS-048 | Admin-initiated local consent | Backlog | P0 | MIDAS-047, MIDAS-034, MIDAS-011, MIDAS-012 |
| MIDAS-049 | Normal remote control with working UAC | Backlog | P0 | MIDAS-048, MIDAS-042, MIDAS-046 |
| MIDAS-050 | Administrative control and distinct permissions | Backlog | P0 | MIDAS-049 |
| MIDAS-051 | Local stop, expiry and revocation | Backlog | P0 | MIDAS-049, MIDAS-050, MIDAS-041 |
| MIDAS-052 | TURN connectivity and forbidden-destination tests | Backlog | P0 | MIDAS-047, MIDAS-006 |
| MIDAS-053 | Browser remote-support view and toolbar | Backlog | P0 | MIDAS-049, MIDAS-050, MIDAS-052, MIDAS-031 |
| MIDAS-054 | Physical keys, text, IME and cursor | Backlog | P0 | MIDAS-047, MIDAS-053 |
| MIDAS-055 | In-session clipboard and file transfer | Backlog | P1 | MIDAS-053, MIDAS-051 |
| MIDAS-056 | Chat during an active support session | Backlog | P2 | MIDAS-053, MIDAS-034 |
| MIDAS-057 | Mobile remote-control input | Backlog | P2 | MIDAS-053, MIDAS-054, MIDAS-014 |
| MIDAS-058 | Windows/UAC/browser and latency matrix | Backlog | P0 | MIDAS-051, MIDAS-043, MIDAS-044, MIDAS-045, MIDAS-053, MIDAS-054, MIDAS-055, MIDAS-056, MIDAS-057 |
| MIDAS-059 | Two-participant usability trial over a real connection | Backlog | P0 | MIDAS-058 |
| MIDAS-060 | Review phase 2 and close its milestone | Backlog | P0 | All other phase items |

**Acceptance criteria and traceability**

- **MIDAS-041:** Customer/device opt-in, permission, reason and fresh authentication are required, and an occupied desktop requires local consent. Requirements: SERVER-02, SERVER-04. Legacy ID: F2-02.
- **MIDAS-042:** Monitor selection and combined view work, and old-target pending input is discarded on desktop changes. Requirements: REMOTE-03, REMOTE-06. Legacy ID: F2-03.
- **MIDAS-043:** A supported NVIDIA host negotiates a working codec, reports measured latency and handles encoder failures. Requirements: PERF-04. Legacy ID: F2-04.
- **MIDAS-044:** A supported AMD host negotiates a working codec, reports latency and handles encoder failures. Requirements: PERF-04. Legacy ID: F2-04.
- **MIDAS-045:** A supported Intel host negotiates a working codec, reports latency and handles encoder failures. Requirements: PERF-04. Legacy ID: F2-04.
- **MIDAS-046:** A virtual-display desktop is controllable without a hardware encoder and binary distribution terms are verified. A shared adaptive controller adjusts bitrate and, where needed, resolution and frame rate using estimated bandwidth, RTT, loss and jitter. It reduces quality on degradation and restores it gradually after sustained recovery without oscillation or growing frame queues. It handles text/motion profiles, missing measurements, reconnection and hardware/software encoders; MIDAS-058 validates the full network matrix. Requirements: PERF-04, SERVER-03, PERF-07. Legacy ID: F2-05.
- **MIDAS-047:** User worker, transport and privileged component use isolated identities and reject other users/sessions. Requirements: REMOTE-03, REMOTE-06. Legacy ID: F2-06.
- **MIDAS-048:** Both modes show identity/capabilities and reject remote approval and late replies to declined/expired requests. Requirements: UI-04, UI-06, REMOTE-01, REMOTE-02. Legacy ID: F2-07.
- **MIDAS-049:** User applications retain their privileges and authorized UAC is visible and interactive in the same connection. Requirements: REMOTE-03, REMOTE-04, REMOTE-05. Legacy ID: —.
- **MIDAS-050:** The mode requires fresh passkey verification and appropriate rights without altering existing user processes. Requirements: REMOTE-04, REMOTE-05, SEC-03. Legacy ID: —.
- **MIDAS-051:** Local stop is immediate and revocation/signaling loss terminates direct and TURN sessions within the specified lifetime. Requirements: REMOTE-07, SEC-03. Legacy ID: —.
- **MIDAS-052:** Direct/relayed connectivity works and short-lived authentication plus IPv4/IPv6 internal-destination blocks are tested. Requirements: SEC-05. Legacy ID: F2-08.
- **MIDAS-053:** Tested browsers show mode/target, real connection statistics and controls for supported system keys. Show default automatic quality, text/motion profiles, actual resolution/bitrate/frame rate and a clear degraded-network indication. Missing measurements must not be presented as a healthy connection. Requirements: UI-02, UI-04, UI-07, PERF-07. Legacy ID: F2-09.
- **MIDAS-054:** Hungarian/mismatched layouts, mouse modifiers and SAS requests work for the authorized target. Requirements: REMOTE-08. Legacy ID: F2-10.
- **MIDAS-055:** Authorized transfers resume correctly and file/clipboard secrets stay out of audit. Requirements: REMOTE-08, SEC-04. Legacy ID: F2-11.
- **MIDAS-056:** The two authorized participants exchange chat messages and the channel ends with the session. Requirements: UI-06. Legacy ID: F2-12.
- **MIDAS-057:** Tap, secondary click, zoom and text input are exercised on a real phone. Requirements: UI-08. Legacy ID: F2-14.
- **MIDAS-058:** Measurements document console/RDP, lock, credential prompts, both modes and software encoding, with support claims matching results. Repeatable network profiles verify bandwidth reduction/recovery, latency, jitter, packet loss and path changes. Across direct/TURN paths, normal/admin modes, UAC and hardware/software encoding, record adaptation/recovery time, actual quality, readability and input-to-visible-response delay; sustained quality oscillation and growing frame queues are unacceptable. Requirements: REMOTE-03, REMOTE-07, REMOTE-08, SERVER-03, PERF-07. Legacy ID: —.
- **MIDAS-059:** Operator initiation, decline, fresh acceptance, UAC and local stop are understandable unaided on both ends. Include Hungarian and English, keyboard, mobile-dashboard and local screen-reader trials. Requirements: UI-06, UI-07, UI-08. Legacy ID: —.
- **MIDAS-060:** Verify both remote modes with standard and administrator accounts, UAC, local stop and separately authorized server access. Accept the complete test matrix and both interfaces' usability; all committed work is reviewed Done. Requirements: —. Legacy ID: F2-15.

### 3. Automated maintenance

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-061 | Versioned scripts and signed execution grants | Backlog | P0 | MIDAS-022, MIDAS-011, MIDAS-012 |
| MIDAS-062 | Scheduler, run state and duplicate protection | Backlog | P0 | MIDAS-061 |
| MIDAS-063 | Maintenance windows and server reboot policy | Backlog | P0 | MIDAS-062, MIDAS-028 |
| MIDAS-064 | Windows Update approvals and reboot handling | Backlog | P0 | MIDAS-063 |
| MIDAS-065 | Machine-wide and per-user application updates | Backlog | P1 | MIDAS-063 |
| MIDAS-066 | apt and dnf update cycle | Backlog | P0 | MIDAS-063 |
| MIDAS-067 | Deploy approved MSI and EXE packages | Backlog | P1 | MIDAS-062 |
| MIDAS-068 | Alert-triggered remediation job | Backlog | P1 | MIDAS-062, MIDAS-029 |
| MIDAS-069 | Separately authorized PowerShell and Linux shell | Backlog | P1 | MIDAS-062, MIDAS-031 |
| MIDAS-070 | Background files, processes and service operations | Backlog | P1 | MIDAS-062, MIDAS-031 |
| MIDAS-071 | Domain-join preflight and delegated OU authorization | Backlog | P0 | MIDAS-036, MIDAS-038, MIDAS-062 |
| MIDAS-072 | Protected offline domain join package | Backlog | P0 | MIDAS-071 |
| MIDAS-073 | Verify domain trust and agent return | Backlog | P0 | MIDAS-072, MIDAS-063 |
| MIDAS-074 | Review phase 3 and close its milestone | Backlog | P0 | All other phase items |

**Acceptance criteria and traceability**

- **MIDAS-061:** Modified, expired and cross-tenant jobs are rejected and execution identity/parameters are audited safely. Requirements: SEC-03. Legacy ID: F3-01.
- **MIDAS-062:** Interrupted/re-delivered jobs reconcile their state without duplicating non-repeatable operations. Requirements: UI-05, PERF-03, SEC-03. Legacy ID: F3-02.
- **MIDAS-063:** Jobs obey approved windows and per-device postponement/reboot outcomes remain visible. Requirements: SERVER-04. Legacy ID: F3-03.
- **MIDAS-064:** A test group installs approved patches with verified notification and restart behavior. Requirements: SERVER-01, SERVER-04. Legacy ID: F3-04.
- **MIDAS-065:** Select the proper execution context, show unavailable WinGet support and validate an approved package route. Requirements: SERVER-01. Legacy ID: F3-05.
- **MIDAS-066:** Debian/Ubuntu and Rocky hosts update under approval/windows and report reboot requirements. Requirements: SERVER-01. Legacy ID: F3-06.
- **MIDAS-067:** Verified packages deploy to the selected group with visible failures and exit codes. Requirements: SEC-03. Legacy ID: F3-07.
- **MIDAS-068:** An approved script runs on an alert, controls repetition and links its result to the alert. Requirements: PERF-03. Legacy ID: F3-08.
- **MIDAS-069:** Chosen execution identity, scope, cancellation and audit work without a graphical session. Requirements: UI-02, SERVER-01, SEC-01. Legacy ID: F2-13.
- **MIDAS-070:** Selected identity and scope are enforced and per-device outcomes are visible and audited. Requirements: UI-02, SERVER-01, SEC-01. Legacy ID: F2-13.
- **MIDAS-071:** Device/domain/OU preview, DNS/time/edition checks and separate authorization work. Requirements: AD-01, AD-05. Legacy ID: —.
- **MIDAS-072:** The connector provisions with a separate write identity, encrypts delivery to the target agent and manages stored-copy lifetime. Requirements: AD-06, SEC-03. Legacy ID: —.
- **MIDAS-073:** Approved reboot is followed by verified trust and agent return, and uncertain outcomes never trigger blind retries. Requirements: AD-07, SERVER-04. Legacy ID: —.
- **MIDAS-074:** Accept scripts, updates, independent background tools and verified Windows domain joining. Verify failure, retry and reboot handling; all committed work is reviewed Done. Requirements: —. Legacy ID: F3-09.

### 4. Hardening and pilot

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-075 | Linux server installer and verified package repositories | Backlog | P0 | MIDAS-016 |
| MIDAS-076 | Windows server installer and service accounts | Backlog | P0 | MIDAS-016 |
| MIDAS-077 | Protected code signing and distribution reputation tests | Backlog | P0 | MIDAS-075, MIDAS-076, MIDAS-033 |
| MIDAS-078 | Signed agent updates, rings and rollback | Backlog | P0 | MIDAS-077 |
| MIDAS-079 | External checkpoints and audit chain | Backlog | P1 | MIDAS-012 |
| MIDAS-080 | Backup and full recovery on another host | Backlog | P0 | MIDAS-075, MIDAS-076, MIDAS-022 |
| MIDAS-081 | SBOM, release signatures and build provenance | Backlog | P1 | MIDAS-004, MIDAS-077 |
| MIDAS-082 | Customer and global emergency stop | Backlog | P0 | MIDAS-051, MIDAS-062, MIDAS-011 |
| MIDAS-083 | Independent security review and remediation | Backlog | P0 | MIDAS-082, MIDAS-079, MIDAS-081, MIDAS-080, MIDAS-078 |
| MIDAS-084 | Customer data-handling and retention arrangements | Backlog | P1 | MIDAS-079, MIDAS-080 |
| MIDAS-085 | Platform, load and usability regression checks | Backlog | P0 | MIDAS-078, MIDAS-058, MIDAS-059, MIDAS-035, MIDAS-039 |
| MIDAS-086 | First customer deployment and pilot operation | Backlog | P0 | MIDAS-083, MIDAS-084, MIDAS-085 |
| MIDAS-087 | Review phase 4 and close its milestone | Backlog | P0 | All other phase items |

**Acceptance criteria and traceability**

- **MIDAS-075:** The full stack passes clean installation and upgrade on Debian, Ubuntu and Rocky Linux. Requirements: SEC-06. Legacy ID: F4-01.
- **MIDAS-076:** MIDAS, PostgreSQL and eturnal install and upgrade together on clean Windows Server 2022/2025 hosts. Requirements: SEC-06. Legacy ID: F4-02.
- **MIDAS-077:** Project Windows binaries are signed/timestamped, keys remain protected and actual distribution routes are tested. Requirements: SEC-06. Legacy ID: F4-03.
- **MIDAS-078:** Invalid packages are rejected and a failed test release rolls back to an approved prior version without wider rollout. Requirements: SEC-06. Legacy ID: F4-04.
- **MIDAS-079:** External evidence detects rewriting, deletion and tail truncation and verifies secret exclusion. Requirements: SEC-04. Legacy ID: F4-05.
- **MIDAS-080:** Data/config/key recovery is exercised and revoked agents remain excluded after restoration. Requirements: SEC-02, SEC-06. Legacy ID: F4-06.
- **MIDAS-081:** Release source, dependencies and signatures are available and verifiable. Requirements: SEC-06. Legacy ID: F4-07.
- **MIDAS-082:** The selected scope suspends new actions and revokes active grants while preserving audit. Requirements: SEC-06. Legacy ID: —.
- **MIDAS-083:** Threat modeling and external review findings of critical/high severity are fixed and retested. Requirements: SEC-01, SEC-02, SEC-03, SEC-04, SEC-05, SEC-06. Legacy ID: F4-08.
- **MIDAS-084:** Pilot data roles, retention, access and applicable obligations are documented. Requirements: —. Legacy ID: F4-09.
- **MIDAS-085:** The declared support matrix, authorization boundaries and prior usability trials pass again. Requirements: PERF-03, UI-08, REMOTE-07. Legacy ID: —.
- **MIDAS-086:** After security remediation, the first customer's month-long incident-free pilot is documented. Requirements: —. Legacy ID: F4-10.
- **MIDAS-087:** Accept security and recovery tests; fix critical and high-risk findings; complete the first customer's one-month incident-free pilot. All committed work is reviewed Done. Requirements: —. Legacy ID: F4-11.

### 5. Extensions

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-088 | Graphical Linux remote support on X11 | Backlog | P1 | MIDAS-053 |
| MIDAS-089 | Wayland support and portal permissions | Backlog | P1 | MIDAS-088 |
| MIDAS-090 | Portable attended quick support | Backlog | P1 | MIDAS-077, MIDAS-053, MIDAS-048 |
| MIDAS-091 | WASAPI/Opus audio transmission | Backlog | P2 | MIDAS-053 |
| MIDAS-092 | Governed recording and credential-surface exclusion | Backlog | P2 | MIDAS-084, MIDAS-053 |
| MIDAS-093 | Entra ID and AD FS OIDC sign-in | Backlog | P1 | MIDAS-009, MIDAS-011 |
| MIDAS-094 | PWA Web Push on real phones | Backlog | P1 | MIDAS-014, MIDAS-029 |
| MIDAS-095 | Customer reports and scheduled export | Backlog | P2 | MIDAS-062, MIDAS-031, MIDAS-079 |
| MIDAS-096 | Review phase 5 and close its milestone | Backlog | P0 | All other phase items |

**Acceptance criteria and traceability**

- **MIDAS-088:** An X11 host completes a session with local consent, input and termination. Requirements: REMOTE-01, REMOTE-07. Legacy ID: F5-01.
- **MIDAS-089:** Portal consent, capture, input and permissions are tested on the designated real Wayland environment. Requirements: REMOTE-01, REMOTE-07. Legacy ID: F5-01.
- **MIDAS-090:** After short-code pairing, an operator request is locally approved and exit invalidates temporary components/grants. Requirements: REMOTE-01, REMOTE-07. Legacy ID: F5-02.
- **MIDAS-091:** Authorized audio is synchronized and stops when disabled or the session ends. Requirements: SEC-05. Legacy ID: F5-03.
- **MIDAS-092:** Authorized recording verifies retention, access and UAC/sign-in exclusion including buffered frames. Requirements: REMOTE-08, SEC-04. Legacy ID: F5-04.
- **MIDAS-093:** Trusted issuer/subject, callback correlation and group scope are tested without tenant mixing. Requirements: SEC-01. Legacy ID: F5-05.
- **MIDAS-094:** Authorized subscriptions notify real Android and iPhone devices in supported installation contexts. Requirements: UI-08. Legacy ID: F5-06.
- **MIDAS-095:** The selected customer's report uses only its data, with validated PDF/export and scheduled delivery. Requirements: SEC-01. Legacy ID: F5-07.
- **MIDAS-096:** Each committed extension passes its acceptance, authorization and regression trials and has documentation; all committed work is reviewed Done. Unapproved proposals are not closure criteria. Requirements: —. Legacy ID: —.

### Separate proposals – no milestone

These are uncommitted proposals and remain Backlog. They need a separate decision before Ready and do not block phase closure.

| ID | Task | At kickoff | Priority | Direct dependencies in addition to the previous gate |
| --- | --- | --- | --- | --- |
| MIDAS-097 | Ticketing, customer portal and user-initiated support requests | Backlog | P2 | — |
| MIDAS-098 | Windows LAPS integration | Backlog | P2 | — |
| MIDAS-099 | Temporary grants and two-person approval | Backlog | P2 | — |
| MIDAS-100 | Security quarantine and controlled release | Backlog | P2 | — |
| MIDAS-101 | View-only remote support mode | Backlog | P2 | — |
| MIDAS-102 | Fleet compliance view | Backlog | P2 | — |

**Acceptance criteria and traceability**

- **MIDAS-097:** Schedule only after a separate scope decision. Define ticketing/portal integration, authorization and data handling first; the current agent has no Request help action. Requirements: —. Legacy ID: F5-08, F1-14.
- **MIDAS-098:** After a separate decision and authorization design: short-lived access to local administrator credentials, auditing and exclusion of secrets from logs and recordings. Requirements: —. Legacy ID: —.
- **MIDAS-099:** After a separate decision: expiring grants scoped to device and action, with approval by two distinct operators for high-impact actions. Requirements: —. Legacy ID: —.
- **MIDAS-100:** After a separate decision: per-customer authorization for network isolation, a working local recovery path and audited release. Requirements: —. Legacy ID: —.
- **MIDAS-101:** After a separate decision: clear local disclosure, disabled input and separate consent before switching to control. Requirements: —. Legacy ID: —.
- **MIDAS-102:** After a separate decision: a tenant-isolated overview of agent update rings, BitLocker, Defender, patches and pending reboots; show freshness and missing data. Requirements: —. Legacy ID: —.

### Scope changes

**November 7, 2026 · MIDAS-018 and MIDAS-019 moved from M0 to M1**

- **Decision and reason:** decided by the project owner. Both missing trials need external resources: a Windows 11 test machine with a standard account, and new participants with screen-reader devices. These are not available now, while the rest of phase 0 is complete and verified.
- **Completed part:** both tasks are implemented and checked by the developer (0f3fb16, 982f1ee). Acceptance is unchanged; both stay in review until the trial result is recorded.
- **Dependencies:** both tasks keep their original dependencies and do not depend on MIDAS-020, because their work started in M0. MIDAS-020 no longer waits for them; MIDAS-040 does. MIDAS-034 depends directly on MIDAS-019, and MIDAS-042 and MIDAS-047 on MIDAS-018, so those start only after the trials are accepted.
- **Effect:** phase 0 can close, and the rest of phase 1 starts under the usual conditions. The next entry changed the dependencies.

**November 7, 2026 · MIDAS-018, MIDAS-019 and MIDAS-020, awaiting the project owner's verification, no longer block later work**

- **Decision and reason:** decided by the project owner. The three tasks await the owner's own verification: MIDAS-018 the Windows 11 test-machine UAC trial with a standard account, MIDAS-019 the participant, keyboard and screen-reader trials, MIDAS-020 the approval of the WebRTC decision (ADR 0002). These cannot be done now, while development must continue to keep the schedule.
- **Rule:** the three tasks stay In review and may become Done only after the owner's verification. Verification remains required, but their Done status is not a prerequisite for starting later tasks or phases or for closing phase gates. The exception covers only these three tasks; the full status flow and the phase rule apply unchanged to every other task and gate.
- **Removed dependencies:** M1 items (MIDAS-021–040) on MIDAS-020; MIDAS-034, MIDAS-040 and MIDAS-059 on MIDAS-019; MIDAS-040, MIDAS-042 and MIDAS-047 on MIDAS-018.
- **Risk and follow-up:** later work may build on UAC, usability and WebRTC assumptions that are not yet verified. If a verification fails, re-check MIDAS-034, MIDAS-042, MIDAS-047, MIDAS-059 and work built on them, reopening where needed.

**November 7, 2026 · The exception extended to MIDAS-023 and MIDAS-024**

- **Decision and reason:** decided by the project owner. The owner runs the system-level trials of the Windows service (MIDAS-023) and the Linux package (MIDAS-024) on their own machines: installation, restart, another session, and the three distributions. This is not happening now, while development must continue.
- **Rule:** the same as for MIDAS-018–020. Both tasks stay In review and may become Done only after the trial; their Done status is not a prerequisite for starting later tasks or phases or for closing phase gates. The exception now covers these five tasks only.
- **Removed dependencies:** MIDAS-025, -027, -033, -034, -040, -041, -042, -047 and -078 on MIDAS-023; MIDAS-025, -027, -039, -040 and -078 on MIDAS-024.
- **Risk and follow-up:** if a trial fails, re-check the affected tasks and work built on them, reopening where needed.

### Material changes from the earlier drafts

- F2-01: early WebRTC feasibility is M0 (MIDAS-017), the UAC proof is M1 since November 7, 2026 (MIDAS-018); complete control is M2.
- F1-14: tray/status is M1 (MIDAS-034); help requests are only in proposal MIDAS-097.
- F2-02: governed customer-and-device unattended access is M2 (MIDAS-041).
- F2-04: separate NVIDIA, AMD and Intel implementation/measurement items (MIDAS-043, MIDAS-044, MIDAS-045).
- F2-13: independent terminal and file management are M3 (MIDAS-069, MIDAS-070); in-session transfer is M2 (MIDAS-055).
- F5-08: ticketing, customer portal and user help requests form an uncommitted proposal (MIDAS-097).
- New dedicated items cover both interfaces' usability, UAC authorization and revocation, AD connector/discovery, delegated Windows domain joining, resource measurement, emergency stop and regression.
- Every phase has its own closure task. Core behavior and required security are independent of optional proposals.
