# Windows segédfolyamatok / Windows worker lifecycle

[English](#english)

## Magyar

A `remote_ipc::launch` könyvtár a távsegítési segédfolyamat előkészített indítását és leállítását kezeli. Még nincs bekötve az agent futó szolgáltatásába; nem indít távvezérlést, nem kér hozzájárulást, és nem kezeli a képernyőt vagy a bevitelt.

### Indítás és azonosság

A hívónak a saját, korábban ellenőrzött SYSTEM-közvetítőjét, egy [munkamenethez kötött PreparedToken értéket](windows-session-tokens.md) és egy rendszergazda által védett telepítési útvonalat kell átadnia. A szerep és a munkamenet az előkészített tokenhez tartozik. Távoli kérésből nem vehető át programútvonal, parancssor, PID vagy tokenkezelő. A telepítési könyvtár védelme és az aláírás ellenőrzése a telepítő és a későbbi integráció feladata.

A token ugyanazokon a szerepellenőrzéseken megy át, mint az [IPC azonossága](windows-remote-ipc.md): normál segédnél pontos felhasználó, bejelentkezés és munkamenet, közepes integritás, emelés és UIAccess nélkül; privilegizált segédnél SYSTEM és a kiválasztott munkamenet. Az előkészítő WTS-ből kéri le a felhasználói tokent, szükség esetén kiválasztja a megfelelő kapcsolt korlátozott tokent, illetve külön SYSTEM-másolatot készít a privilegizált szerephez.

Az indítás teljes helyi útvonalat, a telepítési könyvtárat és a `winsta0\default` asztalt használja. A normál segéd rögzített argumentuma `remote-worker`, az emelt segédé `remote-privileged-worker`, a jóváhagyó ablaké `remote-consent`. Az indító külön környezetet állít össze a Windows rendszerkönyvtára alapján (`PATH`, `SystemRoot`, `WINDIR`). A normál segéd megkapja a saját egyszer használható indítási csatornájának azonosítóját is. A folyamat örökölt kezelők nélkül indul, a meglévő asztaljogosultságokkal.

A létrejött folyamat felfüggesztve marad, amíg a tényleges program, token, szerep és munkamenet ellenőrzése sikerül. Az ellenőrzés az elindítás előtt és után is megtörténik. A közvetítő és az átadott token indítás közbeni változása elutasítást okoz. A [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) ideiglenesen módosíthatja a hívó privilégiumait; a szolgáltatásnak ezért a szükséges induló jogosultságokat a közvetítő rögzítése előtt kell rendeznie. A könyvtár nem követi automatikusan a megváltozott tokent.

### Életciklus

A segéd már létrehozáskor névtelen Windows Job Objecthez kerül a `PROC_THREAD_ATTRIBUTE_JOB_LIST` segítségével. Nincs külön, későbbi hozzárendelési lépés. Ez a [Windows által támogatott indítási minta](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812) a folyamat futása előtt érvényesíti a felügyeletet.

A job egy segédfolyamatot enged; abból új gyermekfolyamat nem indulhat. A job kezelőjét kizárólag az indító tartja meg. Lezárásakor a Windows leállítja a segédet, így a felügyelő váratlan megszűnése is kezelt. Normál hibakezeléskor a birtokolt folyamatkezelőn keresztüli leállítás további védelmet ad. A leállítás kérése és a tényleges kilépés külön ellenőrizhető; nincs folyamatos háttérlekérdezés.

Ez folyamatelkülönítés, nem RAM-, CPU-, GPU- vagy videominőségi korlát. Ilyen erőforrásplafonokat a job nem állít be.

### Eseményalapú felügyelet

A `launch::supervise` átveszi az összetartozó, már elindított segédek tulajdonjogát. Leállítási jelre vagy bármelyik segéd kilépésekor az egész csoportot leállítja. Sikeres visszatérés előtt minden folyamat tényleges kilépését megvárja; ennek visszaigazolására öt másodperc áll rendelkezésre, utána hibát jelez. A felügyeleti feladat megszakítása vagy eldobása is megszünteti a birtokolt segédeket, akkor is, ha a feladat még nem kezdett futni.

A kilépésjelzés a Windows [szálkészletének várakozási objektumára](https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwait) épül. Nincs ismételt folyamatlekérdezés vagy segédenként lefoglalt blokkoló alkalmazásszál. A visszahívás csak az eredményt adja át; a várakozás megszüntetésekor a kód letiltja a további visszahívásokat, és a szükséges szinkronizálás után szabadítja fel a kontextust és a folyamatkezelőt.

A `Lease::ended()` az engedély visszavonására, a hitelesített csatorna elvesztésére vagy az eredeti helyi határidőre ébred. A natív ellenőrzés által visszavont engedély is értesíti a várakozókat. A már beállított jel nem veszhet el későn induló felügyeletnél, és egy megszakított várakozás nem hosszabbítja meg az engedélyt. Ez a jövőbeli koordinátor egyik leállítási forrása; az IPC hibáját ugyanott kell majd bekötni. Az életciklus-felügyelet nem helyettesíti a műveletenkénti engedély- és asztalellenőrzést.

A futó segéd megtartja a token előkészítésekor létrejött natív munkamenetkötést. A felügyelet erre külön vár: a Windows szolgáltatás munkamenetjelzése, egy megváltozott vagy sikertelen WTS-megfigyelés, illetve a megfigyelő megszűnése leállítja a teljes segédcsoportot. Nincs külön lekérdező ciklus. A szolgáltatás Stop/Shutdown eseménye és futásának befejezése végleg lezárja a megfigyelőt; utána új kötés sem készülhet.

A megfigyelő sorosítja a natív adatgyűjtést, hogy egy korábban elkezdett lekérés ne írhasson felül újabb eredményt. A kötés még a cél újbóli ellenőrzése előtt feliratkozik a változásokra. A változatlan megfigyelés megtartja, a már érvénytelen kötést egy későbbi sikeres lekérés sem éleszti újra. A jelenlegi konzervatív szabály az adott gép megfigyelt munkamenetlistájának bármely változására bont; nem vált át automatikusan másik felhasználóra vagy új bejelentkezésre. Az UAC asztalváltásának külön ellenőrzése továbbra is a műveleti végrehajtó feladata.

### Szolgáltatáskoordinátor

A `remote_ipc::coordinator` a Windows-szolgáltatás natív háttere a távoli engedélyekhez. Az `observe` a kért munkamenetet friss WTS-adatból keresi meg. Csak azonos azonosítójú és generációjú munkamenetet fogad el, a foglaltságot pedig ugyanabból az adatból adja, amelyet a szerver is lát. Az elfogadott lease-nél az `activate` újra lekéri a munkamenetet, és kiválasztja a segédet:

- üres munkamenetben nincs felhasználói token, ezért csak az [emelt segéd](windows-bootstrap-handles.md) szolgálhatja ki, és csak rendszergazdai vagy UAC-jogot tartalmazó engedéllyel;
- foglalt munkamenetben a felhasználó saját normál segéde és SYSTEM-közvetítője indul, de csak helyi jóváhagyással kiadott lease-szel;
- ismeretlen állapotú munkamenetet elutasít.

Egyszerre egy támogatási munkamenet futhat. A segédeket a lease végéig felügyeli: visszavonás, csatornavesztés, lejárat, munkamenet-változás vagy bármelyik segéd kilépése az egész csoportot leállítja. Indítási hibánál a segédek leállnak, az engedély nem aktiválódik, és a napló a hiba okát rögzíti, titok nélkül. Az asztalfajtát a koordinátor egyelőre normál asztalnak jelenti; a biztonsági asztal felismerése a képernyő- és bevitelkezeléssel együtt készül. A szolgáltatás a képességet csak a `remote_support` beállítás bekapcsolásakor hirdeti, ez alapból ki van kapcsolva.

### Jóváhagyó ablak

A koordinátor `consent` művelete a cél munkamenet friss WTS-adatából indul: csak azonos azonosítójú és generációjú, foglalt munkamenetet fogad el. Futó támogatási munkamenet alatt nem nyit ablakot. A munkamenet felhasználójának tokenjéből a normál segéddel azonos módon készít tokent, és a telepített programot `remote-consent` paranccsal indítja a felhasználó asztalán (`winsta0\default`). Az indítás a normál segéd védett útját követi: az egyszer használható indítási cső csak a felhasználó bejelentkezési azonosítójának nyílik meg, a folyamat ellenőrzi a saját tokenjét és a szolgáltatást, és csak a szolgáltatás saját, ehhez a folyamathoz kötött csatornájához kapcsolódik. Külön üdvözlő üzenete van, így nem téveszthető össze a segédekkel.

A szolgáltatás egyetlen kérdést küld a kérés azonosítójával, a nevekkel, a móddal és a hátralévő másodpercekkel. Csak ugyanerre a kérésre szóló választ fogad el a saját határidején belül; a határidő után a választ lejártnak tekinti. Visszavonáskor nem vár tovább, és az ablak folyamatát a feladatobjektum lezárásával leállítja. Bármilyen indítási vagy kapcsolati hibánál az agent „nem elérhető” választ ad, ami nem aktivál engedélyt.

Az ablak a Windows feladat-párbeszédablaka (Common Controls 6). Az ezt kiválasztó manifest a program 2-es erőforrása, amelyet csak az ablak szála aktivál a megnyitás idejére, és a `comctl32.dll` is ekkor töltődik be. Folyamatszintű manifest vagy DPI-beállítás nem használható, mert minden résztvevőt az asztalához kapcsolna indításkor; a DPI-tudatosságot ezért az ablak szála állítja be. A szöveg a felhasználó felületi nyelvét követi (magyar vagy angol), a visszaszámláló sor nem élő régió. Az automatizált teszt valódi ablakot nyit, és ellenőrzi, hogy a kezdő fókusz az Elutasításon van, az Engedélyezés az első másodpercben nem aktív, majd az engedélyezést, az elutasítást és a lejáratot.

### Ellenőrzés és hátralévő integráció

A tesztek saját jogosultsággal indított valódi Windows-folyamatokkal ellenőrzik a felfüggesztett állapotot, a gyermekindítás tiltását, a kifejezett leállítást, a hibás ismételt indítást és a felügyelő váratlan kilépését. Nem végeznek szolgáltatástelepítést vagy SYSTEM/UAC-próbát.

A felügyelet próbái lefedik az egyik segéd kilépését követő csoportos leállítást, a felügyeleti feladat megszakítását, az előre jelzett leállítást és a már kilépett folyamatot. Aláírt tesztengedéllyel a visszavonás, csatornavesztés és lejárat valódi helyi tesztfolyamatokat állít le; a tesztben a jelenlét mesterséges, ezért ez nem telepített távvezérlési vagy UAC-próba.

A munkamenetkötés próbái a jelzéssel egyidejű és késői várakozást, a megváltozott vagy hibás natív adatot, a megfigyelő megszűnését és végleges leállítását is lefedik. Mesterséges munkamenetadatokkal, valódi helyi folyamatokon ellenőrzött, hogy a felügyelet indulása előtt és közben érkező érvénytelenítés a társfolyamatot is leállítja.

A [páros indítás](windows-bootstrap-handles.md) mindkét irányban kezeli a védett kezelőátadást. A koordinátor a szolgáltatásba kötve indítja a segédeket; a műveletenkénti jogosultság-ellenőrzés a tényleges képernyő- és bevitelkezeléssel együtt készül. A normál segéd bemenetét minden művelet előtt ellenőrizni kell. A telepített szolgáltatás és segéd kapcsolata, a natív WTS-tokenválasztás, az RDP, az UAC és a valós asztalváltások a végső kézi próbák részei.

## English

The `remote_ipc::launch` library prepares and contains remote-support worker processes. It is not wired into the running agent service and does not initiate remote control, request consent, capture screens or inject input.

### Startup and identity

The caller supplies its previously verified SYSTEM broker, a [session-bound PreparedToken](windows-session-tokens.md#english), and an administrator-protected installed image. The role and session belong to the prepared token. Remote requests must never supply an executable path, command line, PID or token handle. Installation protection and signature verification remain installer and integration responsibilities.

Tokens use the same role checks as [IPC identity](windows-remote-ipc.md#english): an ordinary worker must match the exact user, logon and session, with medium integrity, no elevation and no UIAccess; a privileged worker requires SYSTEM in the selected session. Preparation acquires the user token through WTS, selects the appropriate linked limited token when needed, or creates a separate SYSTEM copy for the privileged role.

Creation uses an explicit local image, the installation directory and `winsta0\default`. Ordinary workers use the fixed `remote-worker` argument, the privileged worker `remote-privileged-worker` and the consent window `remote-consent`. The launcher builds a fresh environment with `PATH`, `SystemRoot` and `WINDIR` derived from the Windows system directory. An ordinary worker also receives its single-use startup pipe locator. The child starts without inherited handles and uses existing desktop permissions.

The new process remains suspended until its actual image, token, role and session pass verification. Identity is checked again before and after resuming. Changes to the broker or supplied token during creation cause rejection. [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw) can temporarily change the caller's privileges, so the service must establish required startup privileges before pinning the broker. The library never silently follows a changed token.

### Lifetime

The worker joins an unnamed Windows job during creation through `PROC_THREAD_ATTRIBUTE_JOB_LIST`, without a subsequent assignment step. This [Windows creation pattern](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812) applies containment before the process runs.

The job permits one worker and refuses child-process creation. Only the launcher retains the job handle. Closing it terminates the worker, including when the owner process crashes. Normal error cleanup additionally terminates through the retained creation handle. Requesting termination and observing actual exit are separate operations; there is no background polling.

This contains processes without imposing RAM, CPU, GPU or video-quality limits.

### Event-driven supervision

`launch::supervise` takes ownership of the related running workers. A stop signal or any worker's exit terminates the entire group. Successful return requires observing every process's exit; confirmation has a five-second timeout, after which an error is returned. Cancelling or dropping the supervision future also terminates its owned workers, including before its first poll.

Exit notification uses a Windows [thread-pool wait object](https://learn.microsoft.com/en-us/windows/win32/api/threadpoolapiset/nf-threadpoolapiset-createthreadpoolwait). There is no repeated process query or dedicated blocking application thread per worker. The callback only passes the result. Cleanup disarms further callbacks and synchronizes before freeing the context and process handle.

`Lease::ended()` wakes on grant revocation, authenticated-channel loss or the original local deadline. Native validation failures that revoke the lease also wake waiters. An existing signal cannot be lost by late supervision, and cancelling a wait cannot extend the lease. This is one stop source for the future coordinator; IPC failures must still be connected there. Lifetime supervision does not replace per-operation grant and desktop verification.

A running worker retains the native session binding created during token preparation. Supervision independently awaits its invalidation: a Windows service session notification, changed or failed WTS observation, or loss of the observer stops the entire worker group. There is no additional polling loop. Service Stop/Shutdown events and completion permanently stop the observer; it cannot issue new bindings afterwards.

The observer serializes native collection so an earlier query cannot overwrite a newer result. A binding subscribes before rechecking its target. Unchanged observations preserve it; a subsequent successful observation never revives an invalidated binding. The current conservative policy disconnects on any change to the machine's observed session list; it never silently follows another user or new logon. UAC desktop transitions still require separate checks in the operation executor.

### Service coordinator

`remote_ipc::coordinator` is the Windows service's native backend for remote grants. `observe` finds the requested session in fresh WTS data. It accepts only a session with the same ID and generation and reports presence from the same data the server sees. For an accepted lease, `activate` queries the session again and chooses the worker:

- a vacant session has no user token, so only the [privileged worker](windows-bootstrap-handles.md#english) can serve it, and only for a grant with administrative or UAC rights;
- an occupied session starts the user's own ordinary worker and its SYSTEM companion, but only for a lease issued after local consent;
- a session in an unknown state is refused.

One support session runs at a time. The workers are supervised until the lease ends: revocation, channel loss, expiry, a session change or any worker exiting stops the whole group. On a startup failure the workers stop, the grant is not activated, and the log records the cause without secrets. For now the coordinator reports the ordinary desktop; secure-desktop detection comes with capture and input. The service advertises the capability only when the `remote_support` setting is on, which is off by default.

### Consent window

The coordinator's `consent` operation starts from fresh WTS data for the target session: it accepts only an occupied session with the same ID and generation. It opens no window while a support session runs. It prepares a token from the session user's token in the same way as for the ordinary worker, and starts the installed program with the `remote-consent` command on the user's desktop (`winsta0\default`). Startup follows the ordinary worker's protected path: the single-use startup pipe opens only to the user's logon SID, the process checks its own token and the service, and connects only to the service's own channel bound to this process. It has its own greeting, so it cannot be mistaken for a worker.

The service sends one question with the request ID, the names, the mode and the remaining seconds. It accepts only an answer for the same request within its own deadline; after the deadline it treats the request as expired. On revocation it stops waiting and ends the window's process by closing its job object. Any startup or connection failure makes the agent answer unavailable, which never activates a grant.

The window is a Windows task dialog (Common Controls 6). The manifest that selects it is resource 2 of the program, which only the window's thread activates while it is open, and `comctl32.dll` loads only then. A process manifest or DPI setting cannot be used, because it would connect every participant to its desktop at start; the window's thread therefore sets its own DPI awareness. The text follows the user's interface language (Hungarian or English), and the countdown line is not a live region. An automated test opens a real window and checks that Decline has the initial focus and Allow is inactive for the first second, then allowing, declining and expiry.

### Verification and remaining integration

Tests launch real Windows processes with the test account's privileges to verify suspension, child-process refusal, explicit termination, invalid repeated resumes and owner crashes. They install no service and perform no SYSTEM/UAC trial.

Supervision tests cover group shutdown after one worker exits, supervisor cancellation, an immediate stop signal and an already-exited process. Signed test grants exercise revocation, channel loss and expiry against real local test processes. Presence is synthetic in that test, so it is not an installed remote-control or UAC trial.

Session-binding tests cover concurrent and late waiters, changed or failed native data, observer loss and permanent stop. Using synthetic session data with real local processes, tests verify that invalidation before or during supervision also stops companion workers.

[Paired startup](windows-bootstrap-handles.md#english) handles protected transfer in both directions. The coordinator starts the workers from the service; per-operation authorization comes with actual capture and input. Every operation must validate ordinary-worker input. Final manual trials cover the installed service and worker, native WTS token selection, RDP, UAC and real desktop transitions.
