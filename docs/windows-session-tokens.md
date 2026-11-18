# Windows munkamenettokenek / Windows session tokens

[English](#english)

## Magyar

A `remote_ipc::tokens::PreparedToken` a kiválasztott Windows-munkamenethez készít elő indítási tokent. Az [indítókönyvtár](windows-worker-lifecycle.md) ezt veszi át; nyers token, külön megadott szerep és munkamenetszám helyett összetartozó, ellenőrzött adatokat kap. A token nem kerül hálózati üzenetbe vagy parancssorba.

### Kiválasztás

Mindkét belépési pont az aktuális folyamat korábban rögzített SYSTEM-közvetítőjét követeli meg, a nullás szolgáltatásmunkamenetben. Megszemélyesítő száltoken mellett az előkészítés elutasított. A cél a helyi munkamenetfigyelő friss eredményével teljesen egyező `OsSession`: azonosító, generáció, fiók, állomás, jelenlét és zárolási állapot. Bizonytalan vagy eltűnt cél nem fogadható el.

A normál segéd tokenjének forrása a [WTSQueryUserToken](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsqueryusertoken). Közepes integritású, nem emelt, UIAccess nélküli elsődleges token szükséges. Emelt UAC-token esetén csak a Windows által visszaadott kapcsolt, korlátozott token választható, azonos felhasználóval és Windows-munkamenettel. Ha ilyen nincs vagy nem megfelelő, az indítás elutasított; a kód nem alakítja át pusztán az integritási szint csökkentésével az emelt tokent.

Az UAC kapcsolt tokenjeinek bejelentkezési azonosítói eltérhetnek, ezért a forrás és a kiválasztott token bejelentkezését külön rögzítjük. A [Microsoft leírása](https://learn.microsoft.com/en-us/troubleshoot/windows-client/networking/cannot-redirect-folder-use-drive-map-letter) is külön LSA-bejelentkezéseket ír le. Egy friss lekérdezés új tokenobjektuma elfogadható ugyanahhoz a bejelentkezéshez; megváltozott felhasználó, bejelentkezés vagy szereptulajdonság nem.

A privilegizált segéd külön másolatot kap a közvetítő SYSTEM-tokenjéből. Csak ennek a másolatnak állítjuk át a munkamenetét. A végleges kezelő lekérdezési, másolási és elsődleges tokenként hozzárendelési jogot tart meg; a munkamenet átállításához használt további jog nem marad rajta. A közvetítő saját tokenje változatlan marad.

Foglalt célnál a privilegizált segéd előkészítése is rögzíti a felhasználó natív bejelentkezési azonosságát. Felhasználó nélküli célnál a helyi jelenlétvizsgálat mellett a WTS-lekérdezésnek kifejezetten `ERROR_NO_TOKEN` eredményt kell adnia. Hozzáférési, jogosultsági vagy más hiba nem jelent üres munkamenetet. A felügyelet nélküli kapcsolat külön szabályzata és erős hitelesítése továbbra is szükséges.

### Indítási ellenőrzések

A cél és a token forrása az előkészítés lezárásakor, a folyamat létrehozása előtt és után, valamint a felfüggesztés feloldása előtt és után is ellenőrzött. A már birtokolt indítási token objektum- és módosításazonosítója változatlan kell maradjon. A segédfolyamat elsőként rögzített IPC-azonossága a felfüggesztett állapot alatt is megmarad; új kezelő kiadásakor nem rögzítjük újra az időközben megváltozott tokent. Az indítás vagy a felfüggesztés feloldásának sikertelen ellenőrzése leállítja a létrehozott segédet.

A szükséges szolgáltatásjogokat, köztük a WTS-hez és munkamenetbeállításhoz szükséges jogosultságokat, a szolgáltatásnak induláskor kell rendeznie. A könyvtár nem engedélyez privilégiumot, nem vált másik fiókra, és nem ad emelt jogot a normál segédnek. Nincs új háttérlekérdező ciklus vagy erőforrásplafon.

### Ellenőrzés és további munka

A tesztek lefedik a megváltozott célt, a kapcsolt tokenek eltérő bejelentkezését, a hibás szerepeket, a bizonytalan jelenlét elutasítását és az eredeti IPC-azonosság megőrzését. Valódi Windows-tokenekkel ellenőrzik a másolat készítését, a kezelő korlátozott jogait, az öröklés tiltását és a szál megszemélyesítésének elutasítását. A fejlesztői gépen nem fut SYSTEM-szolgáltatástelepítés vagy UAC-próba.

A szolgáltatásba kötés, a normál segéd SYSTEM-közvetítőjének hitelesített kezdeti elérése, a külön végrehajtók és a képernyő-/bevitelkezelés még hátravan. A futó kapcsolat visszavonása, az asztalgeneráció és az aláírt engedély ellenőrzése a tényleges műveleti végrehajtó feladata lesz. Az indítási token előkészítése önmagában nem távvezérlési engedély. A valós SYSTEM/WTS, UAC-szűrt token, RDP, felhasználó nélküli bejelentkezési felület és a telepített teljes működés próbái a végső kézi elfogadás részei.

## English

`remote_ipc::tokens::PreparedToken` prepares a launch token for the selected Windows session. The [launcher](windows-worker-lifecycle.md#english) accepts this checked binding instead of a raw token with separately supplied role and session arguments. Token handles are never sent over the network or placed on a command line.

### Selection

Both entry points require the current process's previously pinned SYSTEM broker in service session zero. Thread impersonation is refused. The target must exactly match the native observer's current `OsSession`, including ID, generation, account, station, presence and lock state. Unknown or disappeared targets are refused.

Ordinary workers obtain their source through [WTSQueryUserToken](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsqueryusertoken). They require a medium-integrity, non-elevated primary token without UIAccess. An elevated UAC source may use only the OS-returned linked, limited token for the same user and Windows session. Missing or unsuitable linked tokens cause rejection; the implementation does not turn an elevated token into an ordinary worker merely by lowering integrity.

The linked UAC tokens can belong to different logons, so source and selected logon identities are pinned separately. [Microsoft documents these separate LSA logons](https://learn.microsoft.com/en-us/troubleshoot/windows-client/networking/cannot-redirect-folder-use-drive-map-letter). Fresh queries may return new token objects for the same logon; changed user, logon or role properties are refused.

A privileged worker receives a separate copy of the broker's SYSTEM token, and only that copy's session is changed. The final handle retains query, duplicate and primary-assignment rights; the additional session-adjustment right is removed. The broker's own token remains unchanged.

For an occupied target, privileged preparation also pins the user's native logon identity. For a vacant target, WTS must explicitly return `ERROR_NO_TOKEN` in addition to the observer's vacancy result. Access, privilege or other failures never count as vacancy. Unattended policy and strong authentication remain separate requirements.

### Startup checks

The target and token source are checked at preparation completion, before and after process creation, and before and after resuming the suspended process. The owned launch token's object and modification IDs must remain unchanged. The worker's initially pinned IPC identity persists through suspension; issuing another handle never silently repins a changed token. Failed creation or resumption checks terminate the created worker.

The service must establish required privileges, including those needed by WTS and session adjustment, during startup. The library enables no privilege, switches no account and elevates no ordinary worker. It adds no background polling loop or resource ceiling.

### Verification and remaining work

Tests cover changed targets, distinct linked logons, invalid roles, uncertain presence and preservation of the original IPC identity. Real Windows token tests cover duplication, limited handle access, non-inheritance and impersonation refusal. No SYSTEM service installation or UAC trial runs on the development machine.

Service integration, authenticated ordinary-worker bootstrap to its SYSTEM broker, separate executors and capture/input remain. Running-session revocation, desktop generation and signed authorization must be enforced by the actual operation executor. A prepared token is not remote-control authorization. Real SYSTEM/WTS, UAC-filtered token, RDP, vacant logon-surface and installed end-to-end trials remain final manual acceptance work.
