# Távsegítési felhatalmazás / Remote-support authorization

[English](#english)

## Magyar

A MIDAS-047 eddig megvalósított része a közös felhatalmazási szerződés, az auditált szerveroldali kiadás, az agentoldali ellenőrzés és ezek mTLS-csatornás összekötése. A csatorna külön bekötött, megbízható natív végrehajtót igényel; a Windows- és Linux-szolgáltatás ezt még nem állítja be és nem hirdeti a képességet. A Windows-folyamatok elkülönítése, a védett IPC és az éles kép-/bevitelkezelés hátravan. Új távvezérlési HTTP-végpont nem nyílt meg, a webes hozzáférés-ellenőrzés továbbra sem ad végrehajtható engedélyt.

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

### Agentoldali ellenőrzés

A védett transzport `Channel` példánya csak hitelesített kapcsolatadatból épülhet fel. Egy `Pending` kérés új, véletlen azonosítót és helyi monoton kezdőidőt kap. Az engedély elfogadása elfogyasztja ezt a nem másolható kérést: minden céladatnak, a kezelőnek és a kért jogoknak is egyeznie kell. Új kérés új azonosítót használ, ezért egy korábbi válasz nem játszható vissza.

A helyi lejárat a kérés kezdőideje és az aláírt, legfeljebb 30 másodperces időtartam összege. A hálózati vagy szerveroldali várakozás ebből fogyaszt; a gép faliórájának átállítása nem hosszabbítja meg. A normál felhasználói folyamat nem adhat át saját „friss” kezdőidőt vagy jelenlétállítást.

Minden kép- és beviteli művelet friss, védett natív megfigyelést igényel. Foglalt vagy ismeretlen cél, másik munkamenet/generáció, lejárat, leállítás vagy kapcsolatvesztés letiltja az engedélyt. A letiltás nem vonható vissza ugyanazon engedélyen. A secure desktop csak privilegizált komponensnek és külön UAC-joggal hozzáférhető, mindkét módban. Normál mód UAC-jog nélkül nem választhat privilegizált végrehajtási útvonalat.

A sorba állított bevitel egyszer használható `InputPermit` példányt kap. Végrehajtáskor ismét ellenőrzött az engedély, a cél, a natív asztal generációja és típusa. Asztalváltás után a régi bevitel elutasított. A védett natív komponens minden asztalváltáshoz új generációt köt, a korábbi asztalra való visszatéréskor is.

### Következő integráció és elfogadás

Még szükséges az aktív munkamenetek teljes koordinátora, a jogosultság- és szabályzatváltozások azonnali továbbítása, az automatikus megújítás minden alkalommal új ellenőrzéssel, a normál segéd, védett transzport és emelt komponens külön Windows-identitása, valamint az ezeket ellenőrző IPC. A könyvtár `Worker` és `Observation` értékeit a privilegizált natív komponensnek kell előállítania; ezek nem helyettesítik az operációs rendszerből végzett folyamat-, token- és asztalellenőrzést. A helyi jóváhagyás a MIDAS-048 része.

Valódi mTLS-kapcsolaton, elkülönített adatbázissal és teszt-végrehajtóval ellenőrzött a kézbesítés, a megszakítás, az ütköző kérések és az előkészítés közben változó jogosultság, passkey, szabályzat, tanúsítvány és auditálási hiba. A teszt-végrehajtó nem Windows-folyamatazonosság vagy UAC-próba. Az automatizált próbák az aláírást, adatváltoztatást, cél- és jogcserét, egyszeri felhasználást, késleltetést, lejáratot, visszavonást, foglalt/ismeretlen célt, UAC-szétválasztást, asztalváltást és az adatbázisos kiadás negatív eseteit ellenőrzik. A tényleges SYSTEM/RDP/UAC- és folyamat-elkülönítési próbákat a [végső kézi ellenőrzések](vegso-ellenorzes.md) részeként kell elvégezni. A MIDAS-047 még nem lezárt.

## English

MIDAS-047 currently implements the shared authorization contract, audited server issuance, agent enforcement and their mTLS-channel integration. The channel requires an explicitly wired trusted native backend; neither the Windows nor Linux service configures one or advertises this capability yet. Windows process isolation, protected IPC and production capture/input integration remain. No remote-control HTTP endpoint has been added; the web access check still provides no executable grant.

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

### Agent enforcement

The protected transport creates `Channel` only from authenticated connection information. Each non-cloneable `Pending` request receives a new random challenge and local monotonic start time. Acceptance consumes it and requires exact target, operator and requested-rights matches. New requests have new challenges, preventing replay of an earlier response.

Local expiry is request start plus the signed lifetime, at most 30 seconds. Network and server delays consume this lifetime; changing the machine's wall clock cannot extend it. An ordinary user process cannot supply its own fresh start time or presence claim.

Every capture and input operation requires a fresh protected native observation. Occupied/unknown targets, changed sessions/generations, expiry, cancellation and transport loss revoke the lease; the same lease cannot be reactivated. Secure desktop access requires both the privileged component and separate UAC permission in either mode. Normal mode without UAC permission cannot select privileged execution.

Queued input receives a single-use `InputPermit`. Execution rechecks the lease, target, native desktop generation and type. Old input is refused after a desktop switch. The protected native component must assign a new generation on every switch, including returning to a previous desktop.

### Remaining integration and acceptance

The full active-session coordinator, immediate propagation of permission/policy changes, automatic renewal with fresh checks, separate Windows identities for user worker/protected transport/privileged component, and enforcing IPC remain required. The privileged native component supplies library `Worker` and `Observation` values; these do not replace OS process, token and desktop verification. Actual local consent belongs to MIDAS-048.

Real mTLS connections with an isolated database and test backend verify delivery, cancellation, conflicting requests and changes to permissions, passkey freshness, policy, credentials and audit availability during preparation. The test backend does not establish Windows process identity or validate UAC. Automated checks cover signatures, tampering, target/rights substitution, single use, delay, expiry, revocation, occupied/unknown targets, UAC separation, desktop switching and negative database issuance cases. Actual SYSTEM/RDP/UAC and process-isolation trials remain part of [final manual verification](vegso-ellenorzes.md#english). MIDAS-047 is not complete.
