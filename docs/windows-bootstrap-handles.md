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

A szolgáltatáskoordinátor, a végrehajtói belépési pontok és a képernyő-/bevitelkezelés még hátravan. A kézi Windows 11/WTS/RDP/UAC-próbák a végső elfogadás részei.

## English

`remote_ipc::bootstrap::Prepared` transfers a query handle for the associated SYSTEM companion into the service-created, still-suspended ordinary worker. It verifies the service, the worker's actual identity and session binding, and that the companion was prepared for this exact worker. Another process in the same logon cannot replace the selected worker.

[DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle) copies directly into the retained child's handle table. It grants only `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, without inheritance. There is no PID-based target reopening, token transfer or process-control access. Preparation takes ownership of the suspended worker; failure stops the child and Windows releases its handle table. The public preparation API allows one transfer per worker.

The numeric value is valid only in the target process. `import_broker` never adopts or closes that value: it asks Windows for a separate non-inheritable query handle. It rejects the current process, invalid objects, exited processes, a different executable image and a companion outside SYSTEM/session zero. The result is the existing `Peer`, which rechecks the actual process and its current primary token on every IPC operation. The original transferred handle lives for the child's process lifetime; the service never later closes a foreign numeric handle.

Startup rechecks both the session and companion. Supervise the worker and companion together; either process exiting or session invalidation must stop the whole group.

### Verification and remaining integration

Local tests use ordinary processes to check transferred-object identity, preservation of original handles, invalid-input refusal and child termination after failed preparation. Separate Windows CI verifies import, role, non-inheritance and denial of access expansion inside an actual running standard-user worker. The same trial stops the ordinary worker and SYSTEM companion together.

`Prepared::deliver` opens a single-use local pipe before resuming the worker. The worker locates the pipe using the identifier supplied by its launcher. It checks the SYSTEM owner, service session and server PID on the open instance. The service verifies the selected worker before delivering the handle value and the control endpoint UUID. The worker binds the endpoint to the verified companion and acknowledges receipt. Expiry, session loss or delivery failure stops the worker during startup.

`start_pair` starts the companion first. A SYSTEM-only startup pipe delivers the numeric query handle for the ordinary worker and the control endpoint identifier. The companion checks the worker image, primary token and session before binding the control pipe for that logon. Only its acknowledgement releases the ordinary worker for startup. Both stages share a deadline. Failure or cancellation stops both processes. Windows CI also checks that the two processes can exchange control frames.

The service coordinator, executor entry points and capture/input remain to be integrated. Final acceptance includes manual Windows 11/WTS/RDP/UAC trials.
