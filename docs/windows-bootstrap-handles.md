# Kezelőátadás a Windows-segédnek / Handle transfer to the Windows worker

## Magyar

A `remote_ipc::bootstrap::Prepared` a szolgáltatás által létrehozott, még felfüggesztett normál segédnek ad át egy lekérdezési kezelőt a hozzá tartozó SYSTEM-közvetítőhöz. Ellenőrzi a szolgáltatást, a segéd tényleges identitását és munkamenetkötését, valamint azt, hogy a közvetítő pontosan ehhez a segédhez készült. Ugyanazon bejelentkezés másik folyamata sem helyettesítheti a kijelölt segédet.

A [DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle) közvetlenül a megtartott gyermekfolyamat kezelőtáblájába másol. Kizárólag `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` jogot ad, öröklés nélkül. Nincs PID alapján újranyitott cél, tokenátadás vagy folyamatvezérlési jog. A művelet átveszi a felfüggesztett segéd tulajdonosát; hiba esetén a gyermek leáll, és a Windows felszabadítja a kezelőtábláját. A nyilvános előkészítőn keresztül egy segédhez egyszer végezhető el az átadás.

Az átadott számérték csak a célfolyamatban érvényes. Az `import_broker` nem veszi tulajdonba és nem zárja le ezt az értéket: a Windows segítségével külön, nem örökölhető lekérdezési kezelőt készít. Elutasítja a saját folyamatot, a hibás objektumot, a kilépett folyamatot, az eltérő programképet és a nem SYSTEM/nullás munkamenetű közvetítőt. Az eredmény a meglévő `Peer`, amely minden IPC-műveletnél újra ellenőrzi a tényleges folyamatot és annak aktuális elsődleges tokenjét. Az eredeti átadott kezelőt a gyermek folyamatélettartama határolja; a szolgáltatás nem zár le utólag számérték alapján távoli kezelőt.

Indítás előtt ismét ellenőrizni kell a munkamenetet és a közvetítőt. A segédet és a közvetítőt együtt kell felügyelni; bármelyik kilépése vagy a munkamenet érvénytelenítése az egész csoport leállítását igényli.

### Ellenőrzés és hátralévő bekötés

A helyi tesztek normál folyamatokkal vizsgálják az átadott objektum azonosságát, az eredeti kezelők megőrzését, a hibás bemenetek elutasítását és a sikertelen előkészítés utáni gyermekleállítást. A külön Windows-CI egy ténylegesen futó standard felhasználói segédben ellenőrzi az importálást, a szerepet, az öröklés tiltását és a jogosultságbővítés elutasítását. Ugyanez a próba együtt állítja le a normál segédet és a SYSTEM-közvetítőt.

A `Prepared::deliver` egyszer használható helyi csatornát nyit a segéd indítása előtt. A segéd az indítójától kapott azonosítóval keresi meg a csatornát. A megnyitott példányon ellenőrzi a SYSTEM-tulajdonost, a szolgáltatói munkamenetet és a küldő folyamatazonosítóját. A szolgáltatás a kijelölt segéd ellenőrzése után adja át a kezelő értékét és a vezérlőcsatorna UUID-jét. A segéd az ellenőrzött közvetítőhöz köti a végpontot, és visszaigazolja az átvételt. Lejárat, munkamenetvesztés vagy sikertelen átadás esetén az indító leállítja a segédet.

A `start_pair` elsőként a közvetítőt indítja el. A közvetítő SYSTEM-re korlátozott indítási csatornán kapja meg a normál segéd lekérdezési kezelőjének értékét és a vezérlőcsatorna azonosítóját. Ellenőrzi a segéd programját, elsődleges tokenjét és munkamenetét, majd létrehozza az adott bejelentkezésre korlátozott vezérlőcsatornát. Csak a visszaigazolása után indul a normál segéd. A két indításnak közös határideje van. Megszakításkor vagy hibánál mindkét folyamat leáll. A Windows CI a két folyamat közötti üzenetváltást is vizsgálja.

A kiadási `midas-agent.exe` két belső parancsa használja ezt a kapcsolatot. A `remote-broker` a kijelölt segédtől várja az első verziózott üzenetet. A `remote-worker` a közvetítő válaszának ellenőrzése után jelzi, hogy csatlakozott. Az indító ezután a SYSTEM-közvetítő saját csatornáján is ellenőrzi a kapcsolat létrejöttét. Mindkét folyamat eseményre vár egy szálon. A készenlét alatt beérkező adat lezárja a csatornát. A Windows CI a kiadási programmal ellenőrzi a sikeres kapcsolatot, a hiányzó végpont elutasítását és a közvetítő kilépését a segéd leállása után. Az MSI-próba a telepített példány közvetlen, szolgáltatás nélküli indításának elutasítását is ellenőrzi.

A résztvevők örökölt kezelők, így szabványos kimenet nélkül indulnak. Hiba esetén a kilépési kódjuk `0x20` plusz az utoljára elért indítási lépés: `0x21` indulás, `0x22` futtatókörnyezet, `0x23` identitás, `0x24` indítási adatok helye, `0x25` indítási csatorna, `0x26` indítási rekord, `0x27` előkészítés, `0x28` nyugta, `0x29` kapcsolat, `0x2A` készenlét. A közvetlen, szolgáltatás nélküli indítás így `0x22` (identitás elutasítva) vagy `0x23` (nincs szolgáltatás adta indítási adat) kóddal áll le, csatornához nem nyúl. A Windows által adott NTSTATUS-kód (például `0xC0000142`) azt jelzi, hogy a folyamat a programkód előtt állt le.

A szolgáltatás függőségei miatt a `midas-agent.exe` a `shell32.dll`-t és az `ole32.dll`-t késleltetve tölti be (`/DELAYLOAD`). Ez a két könyvtár betöltéskor a `user32`-t is inicializálja, ami olyan asztal nélkül, amelyet a folyamat tokenje használhat, `0xC0000142` hibával leállítja a folyamatot. A résztvevő parancsok nem hívják őket, ezért ezektől függetlenül indulnak; a szolgáltatás az első valódi hívásnál tölti be őket. A Windows CI ezt a kiadási programmal, egy asztalhoz nem jogosult standard felhasználói tokennel ellenőrzi.

A szolgáltatáskoordinátor, a helyi jóváhagyás, a külön emelt végrehajtó és a képernyő-/bevitelkezelés még hátravan. A kézi Windows 11/WTS/RDP/UAC-próbák a végső elfogadás részei.

## English

`remote_ipc::bootstrap::Prepared` transfers a query handle for the associated SYSTEM companion into the service-created, still-suspended ordinary worker. It verifies the service, the worker's actual identity and session binding, and that the companion was prepared for this exact worker. Another process in the same logon cannot replace the selected worker.

[DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle) copies directly into the retained child's handle table. It grants only `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, without inheritance. There is no PID-based target reopening, token transfer or process-control access. Preparation takes ownership of the suspended worker; failure stops the child and Windows releases its handle table. The public preparation API allows one transfer per worker.

The numeric value is valid only in the target process. `import_broker` never adopts or closes that value: it asks Windows for a separate non-inheritable query handle. It rejects the current process, invalid objects, exited processes, a different executable image and a companion outside SYSTEM/session zero. The result is the existing `Peer`, which rechecks the actual process and its current primary token on every IPC operation. The original transferred handle lives for the child's process lifetime; the service never later closes a foreign numeric handle.

Startup rechecks both the session and companion. Supervise the worker and companion together; either process exiting or session invalidation must stop the whole group.

### Verification and remaining integration

Local tests use ordinary processes to check transferred-object identity, preservation of original handles, invalid-input refusal and child termination after failed preparation. Separate Windows CI verifies import, role, non-inheritance and denial of access expansion inside an actual running standard-user worker. The same trial stops the ordinary worker and SYSTEM companion together.

`Prepared::deliver` opens a single-use local pipe before resuming the worker. The worker locates the pipe using the identifier supplied by its launcher. It checks the SYSTEM owner, service session and server PID on the open instance. The service verifies the selected worker before delivering the handle value and the control endpoint UUID. The worker binds the endpoint to the verified companion and acknowledges receipt. Expiry, session loss or delivery failure stops the worker during startup.

`start_pair` starts the companion first. A SYSTEM-only startup pipe delivers the numeric query handle for the ordinary worker and the control endpoint identifier. The companion checks the worker image, primary token and session before binding the control pipe for that logon. Only its acknowledgement releases the ordinary worker for startup. Both stages share a deadline. Failure or cancellation stops both processes. Windows CI also checks that the two processes can exchange control frames.

Two internal commands of the release `midas-agent.exe` use this connection. The `remote-broker` command expects the first versioned message from the assigned worker. The `remote-worker` command reports connection after verifying the companion's reply. The launcher then requires independent confirmation on the SYSTEM companion's own pipe. Both processes wait for disconnection on a single-threaded runtime. Any data received during standby closes the channel. Windows CI exercises a successful connection, refusal of a missing endpoint and companion exit after worker termination with the release executable. The MSI trial also checks that the installed executable refuses direct invocation of either role without the service.

Participants start without inherited handles and therefore without standard output. On failure their exit code is `0x20` plus the last startup step they reached: `0x21` started, `0x22` runtime, `0x23` identity, `0x24` startup locator, `0x25` startup pipe, `0x26` startup record, `0x27` prepared, `0x28` receipt, `0x29` connected, `0x2A` standby. Direct invocation without the service therefore stops with `0x22` (identity refused) or `0x23` (no service-provided startup data) and touches no pipe. An NTSTATUS code from Windows, such as `0xC0000142`, means the process stopped before program code ran.

Because of the service's dependencies, `midas-agent.exe` delay-loads `shell32.dll` and `ole32.dll` (`/DELAYLOAD`). Both initialize `user32` when they load, which stops a process with `0xC0000142` unless its token may use its desktop. The participant commands never call them and therefore start without them; the service loads them on first actual use. Windows CI checks this with the release executable and a standard-user token that has no desktop access.

The service coordinator, local consent, separate elevated executor and capture/input remain to be integrated. Final acceptance includes manual Windows 11/WTS/RDP/UAC trials.
