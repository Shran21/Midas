# Munkamenethez kötött Windows-közvetítő / Session-bound Windows broker

[English](#english)

## Magyar

A `remote_ipc::broker::SuspendedBroker` a szolgáltatás által előkészített normál segéd mellé külön SYSTEM-folyamatot hoz létre, a nullás szolgáltatásmunkamenetben. A közvetítő saját elsődleges tokent kap. Csak a kiválasztott normál segéd natív bejelentkezési SID-je kap `TOKEN_QUERY` jogot ehhez a tokenhez, illetve `PROCESS_QUERY_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` jogot a folyamathoz. A [ProcessIdToSessionId](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-processidtosessionid) a teljes információlekérdezési jogot igényli; ez nem ad memóriakezelési vagy folyamatvezérlési jogosultságot. A szolgáltatás saját tokenjének hozzáférési listája változatlan.

A [DuplicateTokenEx](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-duplicatetokenex) biztonsági leírója az új tokenobjektum hozzáférését szabályozza, nem a `TokenDefaultDacl` értékét. A létrehozás SYSTEM-környezetben történik; a normál felhasználó nem kap tokenmásolási, megszemélyesítési, jogosultságmódosítási, folyamatvezérlési vagy memóriakezelési jogot. A jogosultság a konkrét bejelentkezéshez tartozik, nem általában a felhasználói fiókhoz. A [tokenlekérés](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocesstoken) továbbra is a tényleges folyamat aktuális elsődleges tokenjét ellenőrzi; nincs hozzáférési hibát megkerülő PID-alapú elfogadás.

### Életciklus

A nyilvános indító csak a meglévő `SuspendedWorker` tulajdonost fogadja el, és annak natív token-, folyamat- és munkamenetkötését ellenőrzi. A bejelentkezési SID-et a rögzített normál segéd adja; nem távoli kérésből vagy szabadon megadott SID-ből származik. Emelt segédhez ez az indító nem használható.

A külön közvetítő létrehozásakor már saját, egyfolyamatos Job Objectben van. Felfüggesztve marad a token, a kép és a szerep ellenőrzéséig; a szolgáltatás megszűnése vagy a tulajdonos eldobása leállítja. Nincs örökölt kezelő, szülői környezetmásolat vagy erőforrásplafon. Az indító rögzített `remote-broker` belépési pontot használ, távoli parancssort nem fogad el. A futó tulajdonos megtartja a normál segéd munkamenetkötését, és a meglévő csoportos felügyelet leállítja munkamenetváltozáskor. A koordinátornak a normál és emelt segédeket, valamint ezt a közvetítőt együtt kell felügyelnie.

### Ellenőrzés és korlátok

A helyi próbák hozzá nem rendelt tokenmásolatot használnak, és ellenőrzik a normál folyamat elutasítását, a forrás változatlanságát és a kezelő öröklésének tiltását.

Az elkülönített Windows-CI külön SYSTEM-próbát futtat egy ideiglenes standard fiók két valódi bejelentkezésével. A kiválasztott bejelentkezés megszemélyesítése mellett natív Windows-hozzáférési próbák ellenőrzik a folyamat és az aktuális token lekérdezését; a módosító jogok és a másik bejelentkezés elutasítását is megkövetelik. A másik bejelentkezés a korábban rögzített `Peer` ellenőrzését sem kerülheti meg. A próba ellenőrzi az indítást, a munkamenetjelzés utáni leállítást és az elavult előkészítés elutasítását. A munkamenetjelzést itt tesztforrás adja: ez nem helyettesíti a végső WTS/RDP-kézi próbát. A tesztfeladatot, fiókot és fájlokat a CI eltávolítja; a fejlesztői gépen ilyen rendszerszintű próba nem fut.

A CI tesztgazdája [WMI-indítással](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/create-method-in-class-win32-process) kilép az indító feladat munkamenethez kötött csoportjából, és ezt natív lekérdezéssel igazolja. Ez csak a tesztgazdára vonatkozik; a termék minden segédje megtartja a saját, létrehozáskor hozzárendelt folyamatcsoportját és annak kilépési tilalmát.

Ez az indítókönyvtár még nem jelent működő távvezérlést. A védett kezdeti kapcsolat- és kezelőátadás, a `remote-broker` és `remote-worker` belépési pontok, a szolgáltatáskoordinátor és a képernyő-/bevitelkezelés továbbra is hátravan. A normál segéd nem kaphat aláírókulcsot vagy önálló felhatalmazást a védett asztalhoz. A komponens nem hirdet új távoli képességet.

## English

`remote_ipc::broker::SuspendedBroker` creates a separate SYSTEM companion in service session zero for the ordinary worker prepared by the service. The companion receives its own primary token. Only the selected worker's native logon SID receives `TOKEN_QUERY` on that token and `PROCESS_QUERY_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` on the process. [ProcessIdToSessionId](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-processidtosessionid) requires the full information-query right, which grants no memory or process-control access. The service token's own access list remains unchanged.

The [DuplicateTokenEx](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-duplicatetokenex) security descriptor controls the new token object, not `TokenDefaultDacl`. Creation runs in the SYSTEM context. The ordinary user receives no token duplication, impersonation, privilege adjustment, process control or memory access. Access belongs to the specific logon, not the user account generally. [Token queries](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocesstoken) still inspect the actual process's current primary token; access errors never fall back to accepting a PID.

### Lifecycle

The public launcher accepts the existing `SuspendedWorker` owner and verifies its native token, process and session binding. The pinned ordinary worker supplies the logon SID; no remote request or caller-supplied SID does. This launcher refuses privileged workers.

At creation, the companion already belongs to its own one-process job. It remains suspended until token, image and role checks pass; loss of the service or owner stops it. There are no inherited handles, copied parent environment or resource ceilings. The launcher uses the fixed `remote-broker` entry point and accepts no remote command line. The running owner retains the ordinary worker's session binding, and existing group supervision stops it on session changes. The coordinator must supervise ordinary and privileged workers and this companion together.

### Verification and limits

Local tests use unassigned token copies and check refusal of an ordinary process, unchanged source identity and non-inheritance.

Isolated Windows CI runs a SYSTEM trial with two real logons of one temporary standard account. Native Windows access checks while impersonating the selected logon verify process and current-token queries; modifying rights and the other logon must be refused. The other logon cannot reuse an already pinned `Peer` either. The trial covers resume, shutdown after session invalidation and stale preparation refusal. Session events use a test source here, so final manual WTS/RDP trials remain necessary. CI removes its task, account and files; no such system-level trial runs on the development machine.

The CI host uses [WMI creation](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/create-method-in-class-win32-process) to leave its launcher's session-bound job and verifies this natively. This applies only to the test host; every product worker retains its own at-creation job and prohibition on breakaway.

This launcher library does not yet provide remote control. Protected initial connection/handle transfer, `remote-broker` and `remote-worker` entry points, service coordination and capture/input remain. Ordinary workers must never receive signing keys or independent secure-desktop authority. No new remote capability is advertised.
