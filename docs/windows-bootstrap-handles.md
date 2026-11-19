# Kezelőátadás a Windows-segédnek / Handle transfer to the Windows worker

## Magyar

A `remote_ipc::bootstrap::Prepared` a szolgáltatás által létrehozott, még felfüggesztett normál segédnek ad át egy lekérdezési kezelőt a hozzá tartozó SYSTEM-közvetítőhöz. Ellenőrzi a szolgáltatást, a segéd tényleges identitását és munkamenetkötését, valamint azt, hogy a közvetítő pontosan ehhez a segédhez készült. Ugyanazon bejelentkezés másik folyamata sem helyettesítheti a kijelölt segédet.

A [DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle) közvetlenül a megtartott gyermekfolyamat kezelőtáblájába másol. Kizárólag `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE` jogot ad, öröklés nélkül. Nincs PID alapján újranyitott cél, tokenátadás vagy folyamatvezérlési jog. A művelet átveszi a felfüggesztett segéd tulajdonosát; hiba esetén a gyermek leáll, és a Windows felszabadítja a kezelőtábláját. A nyilvános előkészítőn keresztül egy segédhez egyszer végezhető el az átadás.

Az átadott számérték csak a célfolyamatban érvényes. Az `import_broker` nem veszi tulajdonba és nem zárja le ezt az értéket: a Windows segítségével külön, nem örökölhető lekérdezési kezelőt készít. Elutasítja a saját folyamatot, a hibás objektumot, a kilépett folyamatot, az eltérő programképet és a nem SYSTEM/nullás munkamenetű közvetítőt. Az eredmény a meglévő `Peer`, amely minden IPC-műveletnél újra ellenőrzi a tényleges folyamatot és annak aktuális elsődleges tokenjét. Az eredeti átadott kezelőt a gyermek folyamatélettartama határolja; a szolgáltatás nem zár le utólag számérték alapján távoli kezelőt.

Indítás előtt ismét ellenőrizni kell a munkamenetet és a közvetítőt. A segédet és a közvetítőt együtt kell felügyelni; bármelyik kilépése vagy a munkamenet érvénytelenítése az egész csoport leállítását igényli.

### Ellenőrzés és hátralévő bekötés

A helyi tesztek normál folyamatokkal vizsgálják az átadott objektum azonosságát, az eredeti kezelők megőrzését, a hibás bemenetek elutasítását és a sikertelen előkészítés utáni gyermekleállítást. A külön Windows-CI egy ténylegesen futó standard felhasználói segédben ellenőrzi az importálást, a szerepet, az öröklés tiltását és a jogosultságbővítés elutasítását. Ugyanez a próba együtt állítja le a normál segédet és a SYSTEM-közvetítőt.

A számérték és a végpontadatok védett kézbesítése még nincs a termékbe kötve. A CI ehhez külön tesztcsatornát használ, és a WTS-munkamenetjelzést tesztforrás adja. A fogadott számérték önmagában nem hitelesítés vagy távoli műveleti engedély. A terméknek továbbra is védett kezdeti kézbesítésre, kétoldali, rögzített folyamatellenőrzésre, aláírt munkamenetengedélyre és műveletenkénti jogosultságvizsgálatra van szüksége. A végrehajtói belépési pontok, a szolgáltatáskoordinátor és a képernyő-/bevitelkezelés hátravan; új távoli képesség nincs bekapcsolva. A végső Windows 11/WTS/RDP/UAC-próbák továbbra is kötelezők.

## English

`remote_ipc::bootstrap::Prepared` transfers a query handle for the associated SYSTEM companion into the service-created, still-suspended ordinary worker. It verifies the service, the worker's actual identity and session binding, and that the companion was prepared for this exact worker. Another process in the same logon cannot replace the selected worker.

[DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle) copies directly into the retained child's handle table. It grants only `PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE`, without inheritance. There is no PID-based target reopening, token transfer or process-control access. Preparation takes ownership of the suspended worker; failure stops the child and Windows releases its handle table. The public preparation API allows one transfer per worker.

The numeric value is valid only in the target process. `import_broker` never adopts or closes that value: it asks Windows for a separate non-inheritable query handle. It rejects the current process, invalid objects, exited processes, a different executable image and a companion outside SYSTEM/session zero. The result is the existing `Peer`, which rechecks the actual process and its current primary token on every IPC operation. The original transferred handle lives for the child's process lifetime; the service never later closes a foreign numeric handle.

Startup rechecks both the session and companion. Supervise the worker and companion together; either process exiting or session invalidation must stop the whole group.

### Verification and remaining integration

Local tests use ordinary processes to check transferred-object identity, preservation of original handles, invalid-input refusal and child termination after failed preparation. Separate Windows CI verifies import, role, non-inheritance and denial of access expansion inside an actual running standard-user worker. The same trial stops the ordinary worker and SYSTEM companion together.

Protected delivery of the numeric value and endpoint metadata is not yet integrated into the product. CI uses a separate fixture channel and a test source for WTS session events. A received number is not authentication or a remote execution grant. The product still requires protected bootstrap delivery, mutual pinned-process verification, signed session grants and per-operation authorization. Executor entry points, service coordination and capture/input remain; no new remote capability is enabled. Final Windows 11/WTS/RDP/UAC trials remain mandatory.
