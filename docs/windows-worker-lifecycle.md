# Windows segédfolyamatok / Windows worker lifecycle

[English](#english)

## Magyar

A `remote_ipc::launch` könyvtár a távsegítési segédfolyamat előkészített indítását és leállítását kezeli. Még nincs bekötve az agent futó szolgáltatásába; nem indít távvezérlést, nem kér hozzájárulást, és nem kezeli a képernyőt vagy a bevitelt.

### Indítás és azonosság

A hívónak a saját, korábban ellenőrzött SYSTEM-közvetítőjét, egy [munkamenethez kötött PreparedToken értéket](windows-session-tokens.md) és egy rendszergazda által védett telepítési útvonalat kell átadnia. A szerep és a munkamenet az előkészített tokenhez tartozik. Távoli kérésből nem vehető át programútvonal, parancssor, PID vagy tokenkezelő. A telepítési könyvtár védelme és az aláírás ellenőrzése a telepítő és a későbbi integráció feladata.

A token ugyanazokon a szerepellenőrzéseken megy át, mint az [IPC azonossága](windows-remote-ipc.md): normál segédnél pontos felhasználó, bejelentkezés és munkamenet, közepes integritás, emelés és UIAccess nélkül; privilegizált segédnél SYSTEM és a kiválasztott munkamenet. Az előkészítő WTS-ből kéri le a felhasználói tokent, szükség esetén kiválasztja a megfelelő kapcsolt korlátozott tokent, illetve külön SYSTEM-másolatot készít a privilegizált szerephez.

Az indítás teljes helyi útvonalat, rögzített `remote-worker` argumentumot, a telepítési könyvtárat és a `winsta0\default` asztalt használja. Nincs örökölt kezelő vagy szülői környezetmásolat. A környezet csak a Windows API-ból származó rendszerkönyvtárra épülő `PATH`, `SystemRoot` és `WINDIR` értékeket tartalmazza. Asztaljogosultságot nem módosít.

A létrejött folyamat felfüggesztve marad, amíg a tényleges program, token, szerep és munkamenet ellenőrzése sikerül. Az ellenőrzés az elindítás előtt és után is megtörténik. A közvetítő és az átadott token indítás közbeni változása elutasítást okoz. A [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) ideiglenesen módosíthatja a hívó privilégiumait; a szolgáltatásnak ezért a szükséges induló jogosultságokat a közvetítő rögzítése előtt kell rendeznie. A könyvtár nem követi automatikusan a megváltozott tokent.

### Életciklus

A segéd már létrehozáskor névtelen Windows Job Objecthez kerül a `PROC_THREAD_ATTRIBUTE_JOB_LIST` segítségével. Nincs külön, későbbi hozzárendelési lépés. Ez a [Windows által támogatott indítási minta](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812) a folyamat futása előtt érvényesíti a felügyeletet.

A job egy segédfolyamatot enged; abból új gyermekfolyamat nem indulhat. A job kezelőjét kizárólag az indító tartja meg. Lezárásakor a Windows leállítja a segédet, így a felügyelő váratlan megszűnése is kezelt. Normál hibakezeléskor a birtokolt folyamatkezelőn keresztüli leállítás további védelmet ad. A leállítás kérése és a tényleges kilépés külön ellenőrizhető; nincs folyamatos háttérlekérdezés.

Ez folyamatelkülönítés, nem RAM-, CPU-, GPU- vagy videominőségi korlát. Ilyen erőforrásplafonokat a job nem állít be.

### Eseményalapú felügyelet

A `launch::supervise` átveszi az összetartozó, már elindított segédek tulajdonjogát. Leállítási jelre vagy bármelyik segéd kilépésekor az egész csoportot leállítja. Sikeres visszatérés előtt minden folyamat tényleges kilépését megvárja; ennek visszaigazolására öt másodperc áll rendelkezésre, utána hibát jelez. A felügyeleti feladat megszakítása vagy eldobása is megszünteti a birtokolt segédeket, akkor is, ha a feladat még nem kezdett futni.

A kilépésjelzés a Windows [szálkészletének várakozási objektumára](https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwait) épül. Nincs ismételt folyamatlekérdezés vagy segédenként lefoglalt blokkoló alkalmazásszál. A visszahívás csak az eredményt adja át; a várakozás megszüntetésekor a kód letiltja a további visszahívásokat, és a szükséges szinkronizálás után szabadítja fel a kontextust és a folyamatkezelőt.

A `Lease::ended()` az engedély visszavonására, a hitelesített csatorna elvesztésére vagy az eredeti helyi határidőre ébred. A natív ellenőrzés által visszavont engedély is értesíti a várakozókat. A már beállított jel nem veszhet el későn induló felügyeletnél, és egy megszakított várakozás nem hosszabbítja meg az engedélyt. Ez a jövőbeli koordinátor egyik leállítási forrása; a szolgáltatás leállítását, az IPC hibáját és a natív munkamenetváltozást ugyanott kell majd bekötni. Az életciklus-felügyelet nem helyettesíti a műveletenkénti engedély- és asztalellenőrzést.

### Ellenőrzés és hátralévő integráció

A tesztek saját jogosultsággal indított valódi Windows-folyamatokkal ellenőrzik a felfüggesztett állapotot, a gyermekindítás tiltását, a kifejezett leállítást, a hibás ismételt indítást és a felügyelő váratlan kilépését. Nem végeznek szolgáltatástelepítést vagy SYSTEM/UAC-próbát.

A felügyelet próbái lefedik az egyik segéd kilépését követő csoportos leállítást, a felügyeleti feladat megszakítását, az előre jelzett leállítást és a már kilépett folyamatot. Aláírt tesztengedéllyel a visszavonás, csatornavesztés és lejárat valódi helyi tesztfolyamatokat állít le; a tesztben a jelenlét mesterséges, ezért ez nem telepített távvezérlési vagy UAC-próba.

Még szükséges a szolgáltatásba kötés, a védett kezdeti kezelőátadás, a külön segédfolyamatok belépési pontjai és a jogosultságok tényleges műveletenkénti ellenőrzése. A normál segéd továbbra is nem megbízható felhasználói folyamat. Az indítás és a hitelesített IPC önmagában nem ad távvezérlési vagy UAC-engedélyt. A telepített SYSTEM–felhasználó kapcsolat, a natív WTS-tokenválasztás, az RDP/UAC és a valós asztalváltások ellenőrzése a végső kézi próbák része.

## English

The `remote_ipc::launch` library prepares and contains remote-support worker processes. It is not wired into the running agent service and does not initiate remote control, request consent, capture screens or inject input.

### Startup and identity

The caller supplies its previously verified SYSTEM broker, a [session-bound PreparedToken](windows-session-tokens.md#english), and an administrator-protected installed image. The role and session belong to the prepared token. Remote requests must never supply an executable path, command line, PID or token handle. Installation protection and signature verification remain installer and integration responsibilities.

Tokens use the same role checks as [IPC identity](windows-remote-ipc.md#english): an ordinary worker must match the exact user, logon and session, with medium integrity, no elevation and no UIAccess; a privileged worker requires SYSTEM in the selected session. Preparation acquires the user token through WTS, selects the appropriate linked limited token when needed, or creates a separate SYSTEM copy for the privileged role.

Creation uses an explicit local image, the fixed `remote-worker` argument, the installation directory and `winsta0\default`. No handles or parent environment are inherited. Only `PATH`, `SystemRoot` and `WINDIR`, derived from the Windows API's system directory, are provided. Desktop permissions are not changed.

The new process remains suspended until its actual image, token, role and session pass verification. Identity is checked again before and after resuming. Changes to the broker or supplied token during creation cause rejection. [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) can temporarily change the caller's privileges, so the service must establish required startup privileges before pinning the broker. The library never silently follows a changed token.

### Lifetime

The worker joins an unnamed Windows job during creation through `PROC_THREAD_ATTRIBUTE_JOB_LIST`, without a subsequent assignment step. This [Windows creation pattern](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812) applies containment before the process runs.

The job permits one worker and refuses child-process creation. Only the launcher retains the job handle. Closing it terminates the worker, including when the owner process crashes. Normal error cleanup additionally terminates through the retained creation handle. Requesting termination and observing actual exit are separate operations; there is no background polling.

This contains processes without imposing RAM, CPU, GPU or video-quality limits.

### Event-driven supervision

`launch::supervise` takes ownership of the related running workers. A stop signal or any worker's exit terminates the entire group. Successful return requires observing every process's exit; confirmation has a five-second timeout, after which an error is returned. Cancelling or dropping the supervision future also terminates its owned workers, including before its first poll.

Exit notification uses a Windows [thread-pool wait object](https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwait). There is no repeated process query or dedicated blocking application thread per worker. The callback only passes the result. Cleanup disarms further callbacks and synchronizes before freeing the context and process handle.

`Lease::ended()` wakes on grant revocation, authenticated-channel loss or the original local deadline. Native validation failures that revoke the lease also wake waiters. An existing signal cannot be lost by late supervision, and cancelling a wait cannot extend the lease. This is one stop source for the future coordinator; service shutdown, IPC failures and native session changes must still be connected there. Lifetime supervision does not replace per-operation grant and desktop verification.

### Verification and remaining integration

Tests launch real Windows processes with the test account's privileges to verify suspension, child-process refusal, explicit termination, invalid repeated resumes and owner crashes. They install no service and perform no SYSTEM/UAC trial.

Supervision tests cover group shutdown after one worker exits, supervisor cancellation, an immediate stop signal and an already-exited process. Signed test grants exercise revocation, channel loss and expiry against real local test processes. Presence is synthetic in that test, so it is not an installed remote-control or UAC trial.

Service integration, protected bootstrap handle transfer, separate worker entry points and per-operation authorization remain. Ordinary helpers remain untrusted user processes. Process creation and authenticated IPC do not authorize remote control or UAC interaction. Installed SYSTEM-to-user, native WTS token selection, RDP/UAC and real desktop-transition verification remain final manual trials.
