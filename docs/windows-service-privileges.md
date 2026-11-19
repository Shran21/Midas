# Windows szolgáltatásjogosultságok / Windows service privileges

[English](#english)

## Magyar

A telepített szolgáltatás induláskor előkészíti a Windows-segédek indításához szükséges jogosultságokat, majd rögzíti saját közvetítői azonosságát. A `remote_ipc::startup::Broker` csak SYSTEM-fiókkal, rendszerintegritással, a nullás szolgáltatásmunkamenetben működik; megszemélyesítő száltoken, UIAccess vagy AppContainer mellett elutasít. Nem emel felhasználói folyamatot, és nem módosít fiókjogot, helyi biztonsági szabályzatot vagy UAC-beállítást.

A bejelentkezési SID-et a natív `TokenGroups` lista `SE_GROUP_LOGON_ID` jelöléséből olvassa ki. A SYSTEM-szerep megengedi e csoport hiányát; normál felhasználói segédnél továbbra is kötelező. Több bejelentkezési csoport vagy hibás méretű natív adat elutasítást okoz. A teljes token- és folyamatellenőrzés megmarad.

### Indulás és visszaállítás

Három, a szolgáltatás tokenjében már meglévő privilégiumot engedélyez:

| Privilégium | Feladat |
| --- | --- |
| `SeTcbPrivilege` | WTS-felhasználói token lekérése és a külön token munkamenetének beállítása. |
| `SeAssignPrimaryTokenPrivilege` | Másik elsődleges tokenhez tartozó segéd létrehozása. |
| `SeIncreaseQuotaPrivilege` | A `CreateProcessAsUserW` indítási feltétele. |

A [WTSQueryUserToken](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsqueryusertoken) SYSTEM-környezetet és TCB-jogosultságot követel meg. A [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) a hiányzó engedélyezést ideiglenesen maga is elvégezheti, ami módosíthatná a már rögzített tokent. Ezért a beállítás megelőzi az azonosság rögzítését.

Az [AdjustTokenPrivileges](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-adjusttokenprivileges) igaz visszatérési értéke önmagában nem siker: az `ERROR_NOT_ALL_ASSIGNED` részleges teljesítést jelenthet. A kód ellenőrzi a hibakódot, visszaolvassa az engedélyezett privilégiumokat, és hibánál visszaállítja az API által megadott korábbi állapotot. A másik két szükséges jog megléte nem helyettesíti a harmadikat. Más privilégiumot nem kér; a korábban már engedélyezett jogokat a leállítás sem tiltja le.

A folyamatban egyszerre egy előkészítési tulajdonos élhet. A natív token objektumazonosítójának és egyéb azonosságadatainak meg kell maradniuk; a privilégiumbeállítás után a módosításazonosító is rögzített. Egy későbbi változást a közvetítő nem fogad el automatikusan új alapállapotként.

A szolgáltatás először `StartPending` állapotot jelez. Csak sikeres előkészítés után vált `Running` állapotra; sikertelen indulás szolgáltatásspecifikus hibával áll le. Futásának befejezésekor előbb megszünteti a munkamenetkötéseket, majd ellenőrzi és visszaállítja a korábbi privilégiumállapotot. A későbbi koordinátornak a segédeket és IPC-tulajdonosaikat is le kell állítania a `Broker::finish` előtt. A visszaállítás hibája külön naplóbejegyzés és szolgáltatáshiba; a tulajdonos eldobása hibakezeléskor is megkísérli a visszaállítást.

### Ellenőrzés és korlátok

A helyi tesztek a futó tesztfolyamat tokenjének hozzá nem rendelt másolatain dolgoznak. Lefedik a valódi részleges Windows-sikert és annak visszavonását, az eredeti folyamat változatlanságát, a már engedélyezett jogok megőrzését, a hiányzó kezelőjogot, a többszörös előkészítés tiltását és a normál folyamat elutasítását. Nincs helyi szolgáltatástelepítés vagy SYSTEM/UAC-próba.

A GitHub Windows-telepítőpróbája a ténylegesen telepített SYSTEM-szolgáltatás sikeres előkészítési és leállítási visszaállítási bejegyzését is megköveteli. Ez nem tesztel WTS-felhasználóváltást vagy UAC-kezelést.

A normál segéd és a közvetítő [hitelesített indítási kapcsolatot](windows-bootstrap-handles.md) létesít. A szolgáltatás a [koordinátort](windows-worker-lifecycle.md) a jogosultságok előkészítése után köti be. A `remote.grants` képességet csak a `remote_support` beállítás bekapcsolásakor hirdeti; ez alapból és az MSI-telepítés után is ki van kapcsolva, amíg a jóváhagyás és a képernyő-/bevitelkezelés be nem kerül, és a telepített rendszer próbái meg nem történnek.

## English

The installed service prepares the privileges required to launch Windows workers, then pins its broker identity. `remote_ipc::startup::Broker` requires SYSTEM at system integrity in service session zero; thread impersonation, UIAccess and AppContainer are refused. It elevates no user process and changes no account rights, local security policy or UAC setting.

The logon SID is read from the native `TokenGroups` entry marked `SE_GROUP_LOGON_ID`. SYSTEM roles permit this group to be absent; ordinary user workers still require it. Multiple logon groups or malformed native array lengths cause rejection. Full token and process verification remains in place.

### Startup and restoration

It enables three privileges already present in the service token:

| Privilege | Purpose |
| --- | --- |
| `SeTcbPrivilege` | Acquire WTS user tokens and assign the separate token's session. |
| `SeAssignPrimaryTokenPrivilege` | Create a worker with another primary token. |
| `SeIncreaseQuotaPrivilege` | Satisfy the `CreateProcessAsUserW` startup requirement. |

[WTSQueryUserToken](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsqueryusertoken) requires SYSTEM and TCB privilege. [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) may temporarily enable required privileges, changing an already pinned token. Preparation therefore precedes identity pinning.

A true return from [AdjustTokenPrivileges](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-adjusttokenprivileges) is insufficient: `ERROR_NOT_ALL_ASSIGNED` can indicate partial completion. The code checks the error, reads enabled privileges back and restores the API-provided previous state on failure. Having two required privileges cannot substitute for the third. No other privilege is requested; privileges already enabled before startup remain enabled after cleanup.

Only one preparation owner may exist in the process. Native token object identity and other identity properties must remain unchanged; the modification ID is pinned after adjustment too. Later changes never silently become a new accepted baseline.

The service reports `StartPending` first and reaches `Running` only after successful preparation. Failed startup reports a service-specific error. Runtime completion ends session bindings before checking and restoring original privilege states. The future coordinator must stop workers and their IPC owners before `Broker::finish`. Restoration failure is logged and reported as a service error; dropping the owner during error cleanup also attempts restoration.

### Verification and limits

Local tests use unassigned copies of the test process's token. They exercise actual partial Windows success and rollback, unchanged source identity, preservation of already-enabled privileges, missing handle rights, exclusive preparation and normal-process refusal. No service installation or SYSTEM/UAC trial runs locally.

The GitHub Windows installer trial also requires preparation and shutdown-restoration records from the installed SYSTEM service. This does not exercise WTS user transitions or UAC handling.

The ordinary worker and companion establish an [authenticated startup connection](windows-bootstrap-handles.md#english). The service wires the [coordinator](windows-worker-lifecycle.md#english) after preparing privileges. It advertises `remote.grants` only when the `remote_support` setting is on; that setting is off by default and after MSI installation until consent and capture/input are integrated and the installed-system trials are done.
