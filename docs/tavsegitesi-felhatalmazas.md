# Távsegítési felhatalmazás / Remote-support authorization

[English](#english)

## Magyar

A MIDAS-047 eddig megvalósított része a közös felhatalmazási szerződés, az auditált szerveroldali kiadás, az agentoldali ellenőrzés és ezek mTLS-csatornás összekötése. A csatorna külön bekötött, megbízható natív végrehajtót igényel. A Windows-szolgáltatás [koordinátora](windows-worker-lifecycle.md) elkészült, de a képességet csak az agent `remote_support` beállításának bekapcsolásakor hirdeti; ez alapból ki van kapcsolva. A Linux-szolgáltatás nem kínálja. A Windows-folyamathoz kötött [helyi vezérlőcsatorna](windows-remote-ipc.md) könyvtára is elkészült. A folyamatok védett indítása és a csatorna bekötése elkészült, az éles kép-/bevitelkezelés hátravan. A [helyi jóváhagyáshoz](#helyi-jóváhagyás-midas-048) kötött felügyelt kérésnek saját HTTP-végpontja van; felügyelet nélküli kéréshez nem nyílt végpont, a webes hozzáférés-ellenőrzés ott továbbra sem ad végrehajtható engedélyt.

### Aláírt szerződés

A `shared/remote-auth/` rögzített Ed25519-algoritmussal ír alá. Az aláírt adat külön `MIDAS/remote-unattended-grant/v1` tartományjelölést és kanonikus protobuf-adatot tartalmaz. Nincs kérésből választható algoritmus. A verzió, az azonosítók, a mód és az időtartam ellenőrzött; ismeretlen vagy ismételt mező, nem kanonikus kódolás, hiányzó azonosító és túlméretes adat elutasított. A 512 bájtos payloadhatár protokollvédelmi korlát.

Az aláírás a következőket köti össze:

- egyszeri felhatalmazás-azonosító és agent által generált kérésazonosító;
- kezelő, ügyfél, eszköz és agentkapcsolat;
- a Windows-munkamenet száma és generációja;
- normál vagy rendszergazdai mód, ettől külön UAC-jog;
- kiadás és lejárat, legfeljebb 30 másodperces érvényességgel.

A kriptográfiai műveleteket a már használt `ring` könyvtár végzi: [Ed25519-kulcspár](https://docs.rs/ring/0.17.14/ring/signature/struct.Ed25519KeyPair.html), [aláírás-ellenőrzés](https://docs.rs/ring/0.17.14/ring/signature/struct.UnparsedPublicKey.html). A kód nem valósít meg saját aláírási algoritmust.

### Szerveroldali kiadás

A `GrantAuthority::for_agent` egy hitelesített agentkapcsolathoz kötött, memóriában élő aláírókulcsot hoz létre. A kulcs nem írható ki böngészős válaszba vagy naplóba. A publikus ellenőrzőkulcsot a képességet ténylegesen kezelő agent az mTLS-kapcsolat nyitóválaszából veszi át. A kapott ügyfél- és gépazonosítónak egyeznie kell a natív végrehajtóhoz beállított, beiratkozásból származó azonosítókkal. Minden új kapcsolat új kulcsot és kapcsolat-azonosítót kap.

Az `Authorizer::issue_unattended_grant` ugyanabban az ellenőrzési folyamatban olvassa újra a kezelő jogosultságát, friss passkey-hitelesítését, az ügyfél és a szerver szabályzatát, a karbantartási ablakot, a natív munkamenet-megfigyelést és az agent konkrét tanúsítványát, amelyet a webes előzetes ellenőrzés is használ. A kiadáshoz a megnevezett cél bizonyítottan üres állapota szükséges. Foglalt célhoz nem készül unattended engedély.

Az aláírt adat csak a `remote.grant_issued` auditesemény sikeres tranzakciója és a kapcsolat/frissesség utóellenőrzése után adható vissza. Az audit az azonosítót, célt, módot, UAC-igényt, indoklást és lejáratot őrzi; az aláírt payloadot és az aláírást nem. Másik vagy megszakadt kapcsolathoz tartozó aláíró nem használható.

### Kézbesítés és megszakítás

A `remote.grants` protokollképességet kizárólag a `Settings.remote` beállításával bekötött natív végrehajtó mellett hirdeti az agent. A képességnév kézi hozzáadása a listához önmagában nem kapcsolja be. Régebbi szerver mellett a monitorozás működhet tovább engedélycsatorna nélkül.

Az `Enrollment::authorize_remote` belső művelet először ellenőrzi a kezelő jogait és a kérést, majd a konkrét agentkapcsolaton kér előkészítést. Az agent a kérés beérkezésekor indítja a monoton időmérést, és saját kihívást generál. A natív végrehajtó friss megfigyelése alapján válaszol; foglalt vagy ismeretlen célhoz nem ad kihívást. A szerver a választ a saját kapcsolat- és céladataihoz köti, majd az aktuális adatbázisállapot ellenőrzésével és auditálásával ír alá. Az agent kézbesítéskor újabb natív megfigyelést végez, ellenőrzi az aláírást és az egyszeri kérést, majd a nem másolható `Lease` példányt a natív végrehajtónak adja. Pozitív visszaigazolás csak sikeres átadás után érkezik. A szerver az előkészítést a natív kérés előtt (`remote.grant_prepare`), a kézbesítés eredményét külön (`remote.grant_delivery`) auditálja. Sikertelen előkészítési auditnál nem küld kérést; sikertelen kézbesítési auditnál megszakítja a már átadott engedélyt és nem ad pozitív eredményt a hívónak. A kiadás, a kézbesítés és egy majdani tényleges média-munkamenet külön esemény.

Egy kapcsolat egyszerre egy ilyen felhatalmazást kezel; ütköző kérés elutasított. A szerveroldali tulajdonos megszakítása vagy eldobása visszavonási üzenetet küld, a lejárat ugyancsak leállítja a műveletet. Az agent a visszavonást a natív hívástól függetlenül rögzíti; későn befejeződő előkészítés vagy ellenőrzés nem aktiválhatja újra. Kapcsolatvesztéskor az összes hozzá kötött engedély érvénytelen. A visszaigazolt szerveroldali állapot a felhatalmazás élettartamát jelzi, önmagában nem bizonyít működő videó- vagy beviteli kapcsolatot.

A natív ellenőrzések külön, korlátozott munkasorban futnak, nem tartják fel a hálózati eseményhurkot. Ugyanaz a végrehajtó újracsatlakozáskor sem indíthat második, párhuzamos natív hívást. A már futó blokkoló hívás megszakítása helyett az engedély azonnal érvényét veszti; a natív megvalósításnak saját határidőkkel is rendelkeznie kell. Ezt a viselkedést a [Tokio blokkoló feladatainak működése](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) indokolja.

### Helyi jóváhagyás (MIDAS-048)

A bejelentkezett felhasználó asztalához az adminpanel felügyelt kérést küld; ehhez nem kell felügyelet nélküli szabályzat, de minden más feltétel ugyanaz. A `POST /api/tenants/{id}/devices/{device}/remote-requests` a kijelölt munkamenetre és módra indít kérést. A normál mód a `device.read`, `remote.view` és `remote.control` jogot kéri, a rendszergazdai mód ezen felül a `remote.admin`, a UAC-kezelés a `remote.uac.interact` jogot. Az utóbbi kettő friss passkey-hitelesítést is igényel. A szerver a kéréshez az eszköz egyetlen távsegítési helyét is lefoglalja, ezért egy gépen egyszerre egy kérés vagy munkamenet lehet, egy kezelőnek pedig legfeljebb négy befejezetlen kérése. A kérés állapotát csak a kérelmező olvashatja (`GET …/remote-requests/{request}`), és csak ő vonhatja vissza vagy bonthatja (`DELETE`). A befejezett állapot öt percig olvasható.

A szerver az előkészítő üzenetbe az adatbázisból vett kezelőnevet és ügyfélnevet teszi, valamint a 60 másodperces válaszidőt. Az agent a neveket megtisztítja: a vezérlő-, irány- és láthatatlan formázó karaktereket elhagyja, a szóközöket összevonja, a 200 karakternél hosszabb nevet elutasítja. A kérést csak foglalt munkamenetnél teszi fel. Üres vagy ismeretlen munkamenetnél senkit sem kérdez, és „nem elérhető” választ küld.

Kihívást az agent csak engedélyező válasz után készít. Ekkor ismét megfigyeli a munkamenetet: annak ugyanannak a foglalt asztalnak kell lennie, és a kérés nem lehet visszavont. Az engedély élettartama ettől a pillanattól indul, nem a kéréstől. Az így kapott lease csak a válaszoló foglalt asztalt szolgálhatja ki. Ha a munkamenet kiürül vagy megváltozik, a lease végleg leáll, és nem válik felügyelet nélküli engedéllyé. Elutasítás, lejárat, visszavonás vagy elérhetetlenség után nincs kihívás, így semmilyen késve érkező válasz vagy kézbesítés nem aktiválhat engedélyt. A kezelő a böngészőből sem jóváhagyni, sem a választ helyettesíteni nem tud.

A szerver a választ a kérés saját időkorlátján belül fogadja el. Engedélyezés után újra ellenőrzi a jogokat, a passkey frissességét, az agent tanúsítványát és a megfigyelést, majd aláír. Az engedély legfeljebb 30 másodpercig, a böngészős munkamenet lejáratáig, friss passkeyt igénylő módban pedig a hitelesítés ötperces ablakáig érvényes. Auditált: a kérés (`remote.request`), a helyi válasz (`remote.consent`: `allowed`, `declined`, `expired`, `unavailable`, `cancelled`), a kiadás (`remote.grant_issued`, `unattended: false`) és a kézbesítés (`remote.grant_delivery`). Az aláírt adat nem kerül auditba.

A Windows-szolgáltatás a kérdést külön folyamatként teszi fel, amely a cél munkamenet bejelentkezett felhasználójának tokenjével, a felhasználó asztalán fut, sosem SYSTEM-ként és sosem a 0. munkamenetben ([részletek](windows-worker-lifecycle.md)). Az ablak megnevezi a technikust, az ügyfelet, a módot és a kért képességeket, köztük a UAC-kezelést. A kezdő fókusz az Elutasításon van, az Engedélyezés az első másodpercben nem aktív, és az idő leteltekor az ablak magától elutasít. Futó támogatási munkamenet alatt új ablak nem nyílik, így a technikus távoli bevitellel nem hagyhat jóvá másik kérést. A felhasználó saját más folyamatai az első másodperc után ugyanúgy kezelhetik az ablakot, mint bármely más asztali alkalmazását; ez a Windows azonos felhasználói szintjének határa.

### Rendszergazdai mód (MIDAS-050)

A rendszergazdai távvezérlés külön kérés: a `remote.admin` jogot is igényli, és csak friss passkey-megerősítéssel indul. A panel ezért rendszergazdai vagy UAC-kérés előtt passkey-megerősítést kér, ha a legutóbbi megerősítés a felhasználó válaszidejére (60 másodperc) és a kézbesítésre számolt 90 másodpercen belül elavulna. Így a megerősítés a válasz megérkezésekor is érvényes, és a kezelőnek nem kell kijelentkeznie: a kijelölt ügyfél, gép és munkamenet megmarad. Ha a megerősítés ennek ellenére elavul, a kérés `fresh_passkey_required` okkal sikertelen, és engedély nem készül.

A rendszergazdai mód a futó alkalmazásokon nem változtat. Foglalt munkamenetben ugyanaz a korlátozott, a felhasználó saját jogával futó segéd rögzít és hajtja végre a bevitelt, mint normál módban. Emelt jogú segéd csak UAC-joggal indul, és ott is csak a biztonságos asztalt és a rendszergazdai ablakokat kezeli ([képátvitel](tavsegitesi-kep.md)). A munkamenet indulása után minden további művelet, így a kép indítása is, az aktuális jogosultságot ellenőrzi: a kezelőnek ekkor is meg kell lennie a mód összes jogának (`remote.admin`, UAC-nál `remote.uac.interact`). A friss passkey az indításhoz tartozik, és nem kérjük újra. Elutasított ellenőrzés nem fogyasztja el a kérés egyetlen képindítását. A rendszergazdai eszközök (távoli parancssor, fájl- és szolgáltatáskezelés) a 3. fázisban érkeznek, műveletenként saját engedéllyel.

### Agentoldali ellenőrzés

A védett transzport `Channel` példánya csak hitelesített kapcsolatadatból épülhet fel. Egy `Pending` kérés új, véletlen azonosítót és helyi monoton kezdőidőt kap. Az engedély elfogadása elfogyasztja ezt a nem másolható kérést: minden céladatnak, a kezelőnek és a kért jogoknak is egyeznie kell. Új kérés új azonosítót használ, ezért egy korábbi válasz nem játszható vissza.

A helyi lejárat a kérés kezdőideje és az aláírt, legfeljebb 30 másodperces időtartam összege. A hálózati vagy szerveroldali várakozás ebből fogyaszt; a gép faliórájának átállítása nem hosszabbítja meg. A normál felhasználói folyamat nem adhat át saját „friss” kezdőidőt vagy jelenlétállítást.

A `Lease::ended()` folyamatos lekérdezés nélkül értesíti a natív felügyeletet a lejáratról, a visszavonásról és a csatorna elvesztéséről. Az összes aktuális és későn induló várakozó ugyanazt a végleges leállítást látja. A [Windows-felügyelet](windows-worker-lifecycle.md) ezzel a jellel már összeköthető, a szolgáltatás teljes koordinátora azonban még nincs bekötve. A várakozás nem ellenőriz jelenlétet vagy asztalt, ezért a műveletenkénti ellenőrzések továbbra is kötelezők.

Minden kép- és beviteli művelet friss, védett natív megfigyelést igényel. Foglalt vagy ismeretlen cél, másik munkamenet/generáció, lejárat, leállítás vagy kapcsolatvesztés letiltja az engedélyt. A letiltás nem vonható vissza ugyanazon engedélyen. A secure desktop csak privilegizált komponensnek és külön UAC-joggal hozzáférhető, mindkét módban. Normál mód UAC-jog nélkül nem választhat privilegizált végrehajtási útvonalat.

A sorba állított bevitel egyszer használható `InputPermit` példányt kap. Végrehajtáskor ismét ellenőrzött az engedély, a cél, a natív asztal generációja és típusa. Asztalváltás után a régi bevitel elutasított. A védett natív komponens minden asztalváltáshoz új generációt köt, a korábbi asztalra való visszatéréskor is.

### Következő integráció és elfogadás

Még szükséges a jogosultság- és szabályzatváltozások azonnali továbbítása, az automatikus megújítás minden alkalommal új ellenőrzéssel, valamint a segédek tényleges műveletei (képernyő- és bevitelkezelés). A könyvtár `Worker` és `Observation` értékeit a privilegizált natív komponensnek kell előállítania; ezek nem helyettesítik az operációs rendszerből végzett folyamat-, token- és asztalellenőrzést. A helyi jóváhagyás elkészült, és UAC-jog esetén a biztonságos asztal ugyanabban a kapcsolatban kezelhető ([képátvitel](tavsegitesi-kep.md)); a jóváhagyott munkamenet helyi Befejezés gombja és megújítása a MIDAS-051 része.

Valódi mTLS-kapcsolaton, elkülönített adatbázissal és teszt-végrehajtóval ellenőrzött a kézbesítés, a megszakítás, az ütköző kérések és az előkészítés közben változó jogosultság, passkey, szabályzat, tanúsítvány és auditálási hiba. A teszt-végrehajtó nem Windows-folyamatazonosság vagy UAC-próba. Az automatizált próbák az aláírást, adatváltoztatást, cél- és jogcserét, egyszeri felhasználást, késleltetést, lejáratot, visszavonást, foglalt/ismeretlen célt, UAC-szétválasztást, asztalváltást és az adatbázisos kiadás negatív eseteit ellenőrzik. A tényleges SYSTEM/RDP/UAC- és folyamat-elkülönítési próbákat a [végső kézi ellenőrzések](vegso-ellenorzes.md) részeként kell elvégezni. A MIDAS-047 még nem lezárt.

## English

MIDAS-047 currently implements the shared authorization contract, audited server issuance, agent enforcement and their mTLS-channel integration. The channel requires an explicitly wired trusted native backend. The Windows service [coordinator](windows-worker-lifecycle.md#english) is implemented, but it advertises the capability only when the agent setting `remote_support` is on, which is off by default. The Linux service does not offer it. The process-bound [Windows control IPC library](windows-remote-ipc.md#english) is also implemented. Protected process startup and channel integration are done; production capture/input integration remains. An attended request bound to [local consent](#local-consent-midas-048) has its own HTTP endpoint; no endpoint exists for unattended requests, where the web access check still provides no executable grant.

### Signed contract

`shared/remote-auth/` uses fixed Ed25519 signatures over a distinct `MIDAS/remote-unattended-grant/v1` domain and canonical protobuf payload. Requests cannot choose an algorithm. Version, identifiers, mode and lifetime are validated; unknown or duplicated fields, noncanonical encoding, missing IDs and oversized data are rejected. The 512-byte payload limit is a protocol safety bound.

The signature binds the single-use grant ID and agent-generated challenge, operator, tenant, device, agent connection, Windows session number and generation, normal/administrative mode, separate UAC permission, and issuance/expiry with a maximum 30-second lifetime.

Cryptography uses the existing `ring` dependency: [Ed25519 key pairs](https://docs.rs/ring/0.17.14/ring/signature/struct.Ed25519KeyPair.html) and [signature verification](https://docs.rs/ring/0.17.14/ring/signature/struct.UnparsedPublicKey.html). The code implements no custom signature algorithm.

### Server issuance

`GrantAuthority::for_agent` creates an in-memory signing key bound to an authenticated agent connection. The key must never enter a browser response or log. An agent with the native backend obtains the public verification key from the mTLS welcome message. Tenant and device IDs must match the backend configuration derived from enrollment. Every new connection receives a fresh key and connection ID.

`Authorizer::issue_unattended_grant` rechecks operator rights, fresh passkey authentication, customer/device opt-in, maintenance window, native observation and the exact agent credential through the same decision path used by the web preflight. The selected target must be demonstrably vacant. Occupied targets receive no unattended grant.

Signed data is returned only after the `remote.grant_issued` audit transaction succeeds and connection/freshness checks pass again. Audit retains the grant ID, target, mode, UAC request, reason and expiry, never signed payload or signature. An authority for another or disconnected connection cannot issue the grant.

### Delivery and cancellation

Only an agent with a native backend wired through `Settings.remote` advertises `remote.grants`; manually adding the capability name does not enable it. Monitoring can continue against an older server without a grant channel.

The internal `Enrollment::authorize_remote` operation first checks operator rights and request data, then prepares the specific agent connection. The agent starts its monotonic timer and generates its own challenge on receipt. A fresh native observation determines the response; occupied and unknown targets produce no challenge. The server binds the response to its own connection/target state, rechecks current database authorization and audits issuance. On delivery, the agent takes another native observation, verifies the signature and single-use request, and transfers the non-cloneable `Lease` to the native backend. A positive acknowledgement requires successful handoff. The server audits preparation before the native request (`remote.grant_prepare`) and records delivery outcome separately (`remote.grant_delivery`). Failed preparation audit sends no request; failed delivery audit cancels the handed-off grant and returns no positive result to the caller. Issuance, delivery and a future actual media session are distinct events.

Each connection handles one such authorization at a time; conflicting requests are refused. Cancelling or dropping its server-side owner sends revocation, and expiry stops the operation. The agent records revocation independently of the native call, preventing late preparation or verification from reactivating it. Transport loss invalidates all grants bound to that connection. Acknowledged server state describes authorization lifetime; it does not prove a functioning video/input session.

Native checks run through a bounded work queue, outside the network event loop. Reconnection cannot start a second concurrent native call on the same backend. A running blocking call is not forcibly aborted; its authority is immediately invalidated instead, and the native implementation must also enforce its own deadlines. This follows [Tokio's blocking-task behavior](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

### Local consent (MIDAS-048)

The admin panel sends an attended request to a signed-in user's desktop. It needs no unattended policy, but every other condition still applies. `POST /api/tenants/{id}/devices/{device}/remote-requests` starts a request for the selected session and mode. Normal mode requires `device.read`, `remote.view` and `remote.control`; administrative mode adds `remote.admin`, and UAC handling adds `remote.uac.interact`. The last two also require a fresh passkey sign-in. The server reserves the device's single remote-support slot for the request, so a device has one request or session at a time, and an operator has at most four unfinished requests. Only the requester can read the request state (`GET …/remote-requests/{request}`) and withdraw or end it (`DELETE`). A finished state stays readable for five minutes.

The server puts the operator's display name and the customer name from the database into the prepare message, with a 60-second answer time. The agent cleans the names: it drops control, direction and invisible formatting characters, collapses whitespace and refuses names longer than 200 characters. It asks only for an occupied session. For a vacant or unknown session it asks nobody and reports the request as unavailable.

The agent creates a challenge only after an allowing answer. It then observes the session again: it must be the same occupied desktop, and the request must not be revoked. The grant lifetime starts at that moment, not at the request. The resulting lease serves only the occupied desktop that answered. If the session becomes vacant or changes, the lease stops for good and never turns into an unattended grant. A declined, expired, revoked or unavailable request has no challenge, so no late answer or delivery can activate a grant. The operator can neither approve nor replace the answer from the browser.

The server accepts the answer only within the request's own time limit. After an allowing answer it checks the permissions, passkey freshness, the agent credential and the observation again, then signs. The grant lasts at most 30 seconds, no longer than the browser session, and in modes that need a fresh passkey no longer than the five-minute sign-in window. Audit covers the request (`remote.request`), the local answer (`remote.consent`: `allowed`, `declined`, `expired`, `unavailable`, `cancelled`), issuance (`remote.grant_issued`, `unattended: false`) and delivery (`remote.grant_delivery`). Signed data never enters the audit.

The Windows service asks through a separate process that runs with the token of the target session's signed-in user on that user's desktop, never as SYSTEM and never in session 0 ([details](windows-worker-lifecycle.md#english)). The window names the technician, the customer, the mode and the requested capabilities, including UAC handling. Decline has the initial focus, Allow is inactive for the first second, and the window declines by itself when the time runs out. No new window opens while a support session runs, so the technician cannot approve another request through remote input. After the first second, the user's own other processes can operate the window like any other desktop application of that user; this is the limit of one Windows user level.

### Administrative mode (MIDAS-050)

Administrative remote control is a separate request: it also requires `remote.admin`, and it starts only with a fresh passkey confirmation. The panel therefore asks for a passkey confirmation before an administrative or UAC request when the last one would go stale within 90 seconds, which covers the user's answer time (60 seconds) and delivery. The confirmation is then still valid when the answer arrives, and the operator does not have to sign out: the selected customer, device and session stay. If the confirmation goes stale anyway, the request fails with `fresh_passkey_required`, and no grant is made.

Administrative mode does not change running applications. In an occupied session the same restricted worker, running with the user's own rights, captures and performs input as in normal mode. A privileged worker starts only with UAC rights, and even then it handles only the secure desktop and administrator windows ([video](tavsegitesi-kep.md#english)). After the session starts, every further operation, including starting the picture, checks current authorization: the operator must still hold every permission of the mode (`remote.admin`, and `remote.uac.interact` with UAC). The fresh passkey belongs to the start and is not asked again. A refused check does not use up the request's single picture setup. Administrative tools (remote shell, file and service management) arrive in phase 3, each with its own permission per operation.

### Agent enforcement

The protected transport creates `Channel` only from authenticated connection information. Each non-cloneable `Pending` request receives a new random challenge and local monotonic start time. Acceptance consumes it and requires exact target, operator and requested-rights matches. New requests have new challenges, preventing replay of an earlier response.

Local expiry is request start plus the signed lifetime, at most 30 seconds. Network and server delays consume this lifetime; changing the machine's wall clock cannot extend it. An ordinary user process cannot supply its own fresh start time or presence claim.

`Lease::ended()` notifies native supervision of expiry, revocation and channel loss without polling. All current and late waiters observe the same permanent stop. [Windows supervision](windows-worker-lifecycle.md#english) can consume this signal; the complete service coordinator is not wired yet. Waiting observes neither presence nor desktop state, so per-operation validation remains mandatory.

Every capture and input operation requires a fresh protected native observation. Occupied/unknown targets, changed sessions/generations, expiry, cancellation and transport loss revoke the lease; the same lease cannot be reactivated. Secure desktop access requires both the privileged component and separate UAC permission in either mode. Normal mode without UAC permission cannot select privileged execution.

Queued input receives a single-use `InputPermit`. Execution rechecks the lease, target, native desktop generation and type. Old input is refused after a desktop switch. The protected native component must assign a new generation on every switch, including returning to a previous desktop.

### Remaining integration and acceptance

Immediate propagation of permission/policy changes, automatic renewal with fresh checks and the workers' actual operations (capture and input) remain required. The privileged native component supplies library `Worker` and `Observation` values; these do not replace OS process, token and desktop verification. Local consent is implemented, and with UAC rights the secure desktop is handled in the same connection ([video](tavsegitesi-kep.md#english)); the local End control and renewal of a consented session belong to MIDAS-051.

Real mTLS connections with an isolated database and test backend verify delivery, cancellation, conflicting requests and changes to permissions, passkey freshness, policy, credentials and audit availability during preparation. The test backend does not establish Windows process identity or validate UAC. Automated checks cover signatures, tampering, target/rights substitution, single use, delay, expiry, revocation, occupied/unknown targets, UAC separation, desktop switching and negative database issuance cases. Actual SYSTEM/RDP/UAC and process-isolation trials remain part of [final manual verification](vegso-ellenorzes.md#english). MIDAS-047 is not complete.
