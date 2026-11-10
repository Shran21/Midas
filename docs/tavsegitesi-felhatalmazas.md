# Távsegítési felhatalmazás / Remote-support authorization

[English](#english)

## Magyar

A MIDAS-047 első része a szerver és az agent közös felhatalmazási szerződése, a szerveroldali aláírt kiadás és az agentoldali engedélyellenőrzés. Ezek könyvtári komponensek: az éles agentcsatornás kézbesítés, a Windows-folyamatok elkülönítése és a védett IPC még nem kész. Új távvezérlési HTTP-végpont vagy hirdetett agentképesség nem nyílt meg. A jelenlegi webes hozzáférés-ellenőrzés továbbra sem ad végrehajtható engedélyt.

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

A `GrantAuthority::for_agent` egy hitelesített agentkapcsolathoz kötött, memóriában élő aláírókulcsot hoz létre. A kulcs nem írható ki böngészős válaszba vagy naplóba. A hozzá tartozó publikus ellenőrzőkulcsot a későbbi védett transzport kizárólag a szervert hitelesítő csatornáról veheti át, a megfelelő ügyfél-, gép- és kapcsolat-azonosítóval együtt.

Az `Authorizer::issue_unattended_grant` ugyanabban az ellenőrzési folyamatban olvassa újra a kezelő jogosultságát, friss passkey-hitelesítését, az ügyfél és a szerver szabályzatát, a karbantartási ablakot, a natív munkamenet-megfigyelést és az agent konkrét tanúsítványát, amelyet a webes előzetes ellenőrzés is használ. A kiadáshoz a megnevezett cél bizonyítottan üres állapota szükséges. Foglalt célhoz nem készül unattended engedély.

Az aláírt adat csak a `remote.grant_issued` auditesemény sikeres tranzakciója és a kapcsolat/frissesség utóellenőrzése után adható vissza. Az audit az azonosítót, célt, módot, UAC-igényt, indoklást és lejáratot őrzi; az aláírt payloadot és az aláírást nem. Másik vagy megszakadt kapcsolathoz tartozó aláíró nem használható.

### Agentoldali ellenőrzés

A védett transzport `Channel` példánya csak hitelesített kapcsolatadatból épülhet fel. Egy `Pending` kérés új, véletlen azonosítót és helyi monoton kezdőidőt kap. Az engedély elfogadása elfogyasztja ezt a nem másolható kérést: minden céladatnak, a kezelőnek és a kért jogoknak is egyeznie kell. Új kérés új azonosítót használ, ezért egy korábbi válasz nem játszható vissza.

A helyi lejárat a kérés kezdőideje és az aláírt, legfeljebb 30 másodperces időtartam összege. A hálózati vagy szerveroldali várakozás ebből fogyaszt; a gép faliórájának átállítása nem hosszabbítja meg. A normál felhasználói folyamat nem adhat át saját „friss” kezdőidőt vagy jelenlétállítást.

Minden kép- és beviteli művelet friss, védett natív megfigyelést igényel. Foglalt vagy ismeretlen cél, másik munkamenet/generáció, lejárat, leállítás vagy kapcsolatvesztés letiltja az engedélyt. A letiltás nem vonható vissza ugyanazon engedélyen. A secure desktop csak privilegizált komponensnek és külön UAC-joggal hozzáférhető, mindkét módban. Normál mód UAC-jog nélkül nem választhat privilegizált végrehajtási útvonalat.

A sorba állított bevitel egyszer használható `InputPermit` példányt kap. Végrehajtáskor ismét ellenőrzött az engedély, a cél, a natív asztal generációja és típusa. Asztalváltás után a régi bevitel elutasított. A védett natív komponens minden asztalváltáshoz új generációt köt, a korábbi asztalra való visszatéréskor is.

### Következő integráció és elfogadás

Még szükséges a hitelesített agentcsatornás kérés/kulcs/engedély-kézbesítés, a szerveroldali visszavonás továbbítása és megújítási folyamat, a normál segéd, védett transzport és emelt komponens külön Windows-identitása, valamint az ezeket ellenőrző IPC. A könyvtár `Worker` és `Observation` értékeit a privilegizált natív komponensnek kell előállítania; ezek nem helyettesítik az operációs rendszerből végzett folyamat-, token- és asztalellenőrzést. A helyi jóváhagyás a MIDAS-048 része.

Az automatizált próbák az aláírást, adatváltoztatást, cél- és jogcserét, egyszeri felhasználást, késleltetést, lejáratot, visszavonást, foglalt/ismeretlen célt, UAC-szétválasztást, asztalváltást és az adatbázisos kiadás negatív eseteit ellenőrzik. A tényleges SYSTEM/RDP/UAC- és folyamat-elkülönítési próbákat a [végső kézi ellenőrzések](vegso-ellenorzes.md) részeként kell elvégezni. A MIDAS-047 még nem lezárt.

## English

The first part of MIDAS-047 provides a shared authorization contract, signed server issuance and agent-side enforcement. These are library components: production agent-channel delivery, Windows process isolation and protected IPC are not implemented yet. No remote-control HTTP endpoint or advertised agent capability has been added. The current web access check still provides no executable grant.

### Signed contract

`shared/remote-auth/` uses fixed Ed25519 signatures over a distinct `MIDAS/remote-unattended-grant/v1` domain and canonical protobuf payload. Requests cannot choose an algorithm. Version, identifiers, mode and lifetime are validated; unknown or duplicated fields, noncanonical encoding, missing IDs and oversized data are rejected. The 512-byte payload limit is a protocol safety bound.

The signature binds the single-use grant ID and agent-generated challenge, operator, tenant, device, agent connection, Windows session number and generation, normal/administrative mode, separate UAC permission, and issuance/expiry with a maximum 30-second lifetime.

Cryptography uses the existing `ring` dependency: [Ed25519 key pairs](https://docs.rs/ring/0.17.14/ring/signature/struct.Ed25519KeyPair.html) and [signature verification](https://docs.rs/ring/0.17.14/ring/signature/struct.UnparsedPublicKey.html). The code implements no custom signature algorithm.

### Server issuance

`GrantAuthority::for_agent` creates an in-memory signing key bound to an authenticated agent connection. The key must never enter a browser response or log. The future protected transport must obtain the public verification key solely through its authenticated server channel, together with the matching tenant, device and connection identifiers.

`Authorizer::issue_unattended_grant` rechecks operator rights, fresh passkey authentication, customer/device opt-in, maintenance window, native observation and the exact agent credential through the same decision path used by the web preflight. The selected target must be demonstrably vacant. Occupied targets receive no unattended grant.

Signed data is returned only after the `remote.grant_issued` audit transaction succeeds and connection/freshness checks pass again. Audit retains the grant ID, target, mode, UAC request, reason and expiry, never signed payload or signature. An authority for another or disconnected connection cannot issue the grant.

### Agent enforcement

The protected transport creates `Channel` only from authenticated connection information. Each non-cloneable `Pending` request receives a new random challenge and local monotonic start time. Acceptance consumes it and requires exact target, operator and requested-rights matches. New requests have new challenges, preventing replay of an earlier response.

Local expiry is request start plus the signed lifetime, at most 30 seconds. Network and server delays consume this lifetime; changing the machine's wall clock cannot extend it. An ordinary user process cannot supply its own fresh start time or presence claim.

Every capture and input operation requires a fresh protected native observation. Occupied/unknown targets, changed sessions/generations, expiry, cancellation and transport loss revoke the lease; the same lease cannot be reactivated. Secure desktop access requires both the privileged component and separate UAC permission in either mode. Normal mode without UAC permission cannot select privileged execution.

Queued input receives a single-use `InputPermit`. Execution rechecks the lease, target, native desktop generation and type. Old input is refused after a desktop switch. The protected native component must assign a new generation on every switch, including returning to a previous desktop.

### Remaining integration and acceptance

Authenticated agent-channel request/key/grant delivery, server-revocation delivery, renewal, separate Windows identities for user worker/protected transport/privileged component, and enforcing IPC remain required. The privileged native component supplies library `Worker` and `Observation` values; these do not replace OS process, token and desktop verification. Actual local consent belongs to MIDAS-048.

Automated checks cover signatures, tampering, target/rights substitution, single use, delay, expiry, revocation, occupied/unknown targets, UAC separation, desktop switching and negative database issuance cases. Actual SYSTEM/RDP/UAC and process-isolation trials remain part of [final manual verification](vegso-ellenorzes.md#english). MIDAS-047 is not complete.
