# Windows távsegítési IPC / Windows remote-support IPC

[English](#english)

## Magyar

A `service/src/win/remote_ipc/` a távsegítési vezérlőüzenetek helyi csatornáját valósítja meg. Különálló a tálcaikon csak állapotolvasásra használt csövétől. A szolgáltatás még nem indít vele segédfolyamatokat, és a csatorna nincs bekötve az aláírt engedélyek natív végrehajtójába. Önmagában nem indít távvezérlést.

### Folyamat és szerep

A hívó megbízható folyamatindítója adja át a már megnyitott folyamatkezelőt és a telepített program pontos elérési útját. A csatorna nem fogad el a kapcsolódó féltől kapott PID-et vagy programnevet bizalmi alapként. A folyamatkezelőt a kapcsolat teljes ideje alatt megtartja, ellenőrzi az életben maradást, és a cső Windows által lekérdezett túloldali PID-jét ehhez hasonlítja.

Három szerepet különít el:

- A közvetítő LocalSystem-folyamat a 0. munkamenetben, rendszerintegritással.
- A normál segéd a kijelölt munkamenetben, pontos felhasználói SID-del és bejelentkezési LUID-dal, közepes integritással, emelés és UIAccess nélkül.
- A privilegizált segéd LocalSystem-folyamat, rendszerintegritással, kizárólag a kijelölt, nem nulla munkamenetben.

A folyamat tényleges munkamenetének és a token munkamenetének egyeznie kell. A csatorna rögzíti a token azonosítóját, módosítási azonosítóját, bejelentkezési azonosítóját, felhasználóját és szerepadatait. Ezeket minden üzenet küldése/fogadása előtt és után újraolvassa. Változás, megszűnt folyamat vagy lekérdezési hiba lezárja a csatornát. A [Windows tokenstatisztikája](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_statistics) külön jelzi a token cseréjét és módosítását.

### Csatorna

Minden segéd saját, véletlen nevű, egyetlen példányú helyi csövet kap. A hálózati kliensek tiltottak. A normál segéd csövéhez csak SYSTEM és a kijelölt bejelentkezés SID-je kap hozzáférést; a privilegizált csőn nincs felhasználói engedély. A hozzáférési lista mellett a konkrét folyamat ellenőrzése is kötelező.

A kliens olvasási és adatírási jogot kap, csőpéldány létrehozási jogot nem. Az általános írási jog ezt is tartalmazná, ezért nem használható: [Microsoft: named pipe hozzáférési jogok](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights). A kliens csak azonosítási szintű megszemélyesítést enged a szervernek. Egy meglévő név második tulajdonosként nem nyitható meg.

Az `MRC1` keretezés 1–65 536 bájtos vezérlőüzeneteket enged, a hossz ellenőrzése a lefoglalás előtt történik. Egy művelet legfeljebb öt másodpercig várhat, a hívó ennél korábbi határidőt is adhat. Hibás keret, időtúllépés vagy megszakított olvasás/írás végleg lezárja az adott kapcsolatot. A fogadott adat törlődik a saját pufferének felszabadításakor, a keret alapértelmezett hibakereső megjelenítése nem írja ki a tartalmát. Ezek protokollvédelmi korlátok; a videó minőségét és az agent teljes erőforrás-használatát nem korlátozzák. Nincs új háttérbeli folyamatos lekérdezés.

### Integrációs feltételek és ellenőrzés

A folyamatkezelőhöz lekérdezési és szinkronizálási jog, a folyamat elsődleges tokenjéhez lekérdezési jog szükséges. A normál segéd nem feltétlenül tudja megnyitni a SYSTEM-folyamat tokenjét. Ennek szűk jogosultságú, hitelesített indításkori átadását vagy hozzáférését még a folyamatindítóban kell kialakítani és ellenőrizni; nincs PID-alapú kerülőút vagy automatikus jogbővítés. A programútvonalnak rendszergazdák által védett telepítési helyről kell származnia. A könyvtár nem végez önálló telepítés- vagy kódaláírás-ellenőrzést.

A normál segéd bemenete továbbra is nem megbízható adat: a saját felhasználója módosíthatja vagy befolyásolhatja a folyamatát. Nem állíthat jogosultságot, helyi jelenlétet vagy asztalt, és nem kaphat csatornakulcsot vagy biztonságos asztalról származó képet. A védett végrehajtónak minden tényleges műveletnél külön ellenőriznie kell az aláírt engedélyt, a kijelölt Windows-munkamenet generációját, az aktuális asztalt és a leállítást. A csatorna ellenőrzése nem ad végrehajtási engedélyt; a már kézbesített üzenetet is újra kell ellenőrizni végrehajtáskor. A helyi rendszergazda/SYSTEM kompromittálása kívül esik ezen IPC-határ védelmén.

Az automatizált Windows-próbák valódi helyi csövekkel és tesztfolyamatokkal ellenőrzik az adatcserét, másik folyamat és hamis szerver elutasítását, a folyamat megszűnését, a hibás/hosszú kereteket és a megszakítást. A szerepszabályok külön negatív teszteket kapnak; a tesztcsatorna a tesztfolyamat saját jogaival fut. Ez nem igazolja a tényleges SYSTEM–normál felhasználó határt, az RDP-t vagy az UAC-t. A védett indítás, a külön végrehajtók, az asztalváltás és a teljes telepített működés még megvalósítást és a fejlesztés végi kézi próbát igényel.

## English

`service/src/win/remote_ipc/` implements local transport for remote-support control messages, separate from the tray's status-only pipe. The service does not launch workers through it yet, and it is not connected to signed-grant native execution. It cannot start remote control by itself.

### Process and role

The trusted launcher supplies an already-open process handle and the exact installed executable path. A peer-supplied PID or image name is never a trust anchor. The channel retains the process handle, checks liveness and compares the pipe's OS-reported peer PID against that process.

The broker must be LocalSystem with system integrity in session zero. An ordinary worker must match the selected nonzero session, user SID and logon LUID, with medium integrity, no elevation and no UIAccess. A privileged worker must be LocalSystem with system integrity in the selected nonzero session. App-container tokens are refused.

The process and token session must agree. Token ID, modification ID, logon ID, user and role properties are pinned and reread before and after every message. Changes, process exit and query failures close the channel. [Windows token statistics](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_statistics) distinguish token replacement and modification.

### Channel

Each worker receives a separate random-name, single-instance local pipe. Remote clients are refused. Ordinary-worker pipes allow SYSTEM and the selected logon SID; privileged pipes have no ordinary-user ACE. Exact-process verification is mandatory in addition to this ACL.

Clients receive generic read plus data-write access, without pipe-instance creation. Generic write would also grant instance creation: [Microsoft named pipe access rights](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights). Clients permit identification-level impersonation only. A second owner cannot claim an existing name.

`MRC1` framing accepts 1–65,536-byte control messages, checking length before allocation. Operations wait at most five seconds or a shorter caller deadline. Malformed frames, timeouts and cancelled reads/writes permanently close that connection. Received buffers are zeroed on drop and default frame debug output is redacted. These are protocol safety bounds, not video-quality or global agent resource limits. No continuous background polling is added.

### Integration conditions and verification

Process handles need query and synchronize rights; the primary process token needs query access. An ordinary worker may be unable to open a SYSTEM process token. The launcher still needs a verified, narrowly scoped bootstrap transfer/access design; there is no PID-based fallback or automatic permission expansion. Executable paths must come from an administrator-protected installation. This library does not independently verify installation security or code signatures.

Ordinary-worker input remains untrusted because its user may modify or influence that process. It cannot assert authorization, presence or desktop identity, and must not receive channel keys or secure-desktop frames. Before each actual operation, the protected executor must separately verify the signed lease, selected Windows session generation, current desktop and cancellation. Channel verification does not authorize execution; already delivered messages need revalidation at execution time. Local administrator/SYSTEM compromise is outside this IPC boundary.

Automated Windows checks use real local pipes and test processes for exchange, wrong-process/server rejection, process exit, malformed/oversized frames and cancellation. Role rules have separate negative tests; test channels run with the test process's own privileges. They do not establish the real SYSTEM-to-standard-user boundary or validate RDP/UAC. Protected startup, separate executors, desktop switching and the installed end-to-end flow still require implementation and final manual trials.
