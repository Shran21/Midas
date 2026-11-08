# Agentprotokoll / Agent protocol

[English](#english)

## Magyar

2026. november 7. · MIDAS-021

Az agent egy tartós, kimenő, hitelesített kapcsolattal csatlakozik a szerverhez (PERF-01). A közös szerződés a [`shared/protocol`](../shared/protocol), a kapcsolatkezelés az [`agent/agent-core`](../agent/agent-core) csomagban van; a szerveroldal a `server/src/agent.rs`.

### Kapcsolat

- **Transzport:** WebSocket az mTLS-figyelő `/agent/connect` útvonalán ([szerverindítás](szerver.md)). Érvényes, az agent-CA-ra visszavezethető kliens-tanúsítvány nélkül a TLS-kapcsolat sem jön létre. WebSocket-frissítés nélküli kérésre a válasz 426.
- **Üzenetek:** bináris keretben egy-egy protobuf `Envelope` (prost). Egy üzenet legfeljebb 64 KiB; ezt mindkét oldal a WebSocket-rétegben is érvényesíti.
- **Jogosultság:** csak a [beiratkozáskor](beiratkozas.md) kiadott, regisztrált és vissza nem vont tanúsítvány kapcsolódhat; visszavonáskor a szerver az élő kapcsolatot bontja. Műveletet a kapcsolat önmagában nem engedélyez.

| Üzenet | Irány | Tartalom |
| --- | --- | --- |
| `Hello` | agent → szerver | támogatott verziótartomány, agentverzió (legfeljebb 64 bájt), platform (legfeljebb 32 bájt), platformképességek (legfeljebb 32, [állapotellenőrzések](allapotellenorzes.md)) |
| `Welcome` | szerver → agent | egyeztetett verzió, keepalive-időköz másodpercben, szerververzió |
| `Reject` | szerver → agent | ok (`UNSUPPORTED_VERSION`, `INVALID_HELLO`) és a szerver által támogatott tartomány; utána a szerver bont |
| `Ping` / `Pong` | mindkét irány | élőség-ellenőrzés véletlen azonosítóval |
| `DirectoryAssignment` / `DirectoryReport` | szerver → kijelölt connector, illetve vissza | címtár-olvasási feladat és eredménye ([címtár-connector](cimtar-connector.md)) |
| `MetricsBatch` / `MetricsAck` | agent → szerver, illetve vissza | legfeljebb 60 perces minta és a tárolás utáni nyugta ([metrikák](metrikak.md)) |
| `InventoryUpdate` / `InventoryAck` | agent → szerver, illetve vissza | teljes leltár vagy változáscsomag darabja és a nyugta ([leltár](leltar.md)) |
| `HealthReport` / `HealthAck` | agent → szerver, illetve vissza | az ellenőrzések legutóbbi eredménye és a tárolás utáni nyugta ([állapotellenőrzések](allapotellenorzes.md)) |
| `PolicyAssignment` | szerver → agent | az eszköz öröklés utáni, érvényes beállításai; kapcsolódáskor és változáskor ([szabályzatok](szabalyzatok.md)) |
| `RefreshRequest` | szerver → agent | a technikus friss adatot kért: az agent azonnal ellenőriz, változatlan eredményt is küld, és leltárt gyűjt ([eszközoldal](eszkozoldal.md)) |

### Verziók és kompatibilitás

- Az agent első üzenete a `Hello`; a szerver 10 másodpercen belül várja. A szerver a két tartomány **legmagasabb közös verzióját** választja, és ez a kapcsolat végéig érvényes.
- Közös verzió hiányában a szerver `Reject` üzenetben megadja a saját tartományát, így a hiba oka az agent oldalán is látható. Az agent ekkor a leghosszabb várakozási idővel próbálkozik újra, mert a helyzetet jellemzően szerver- vagy agentfrissítés oldja meg.
- **Kompatibilis változás** (új, opcionális mező vagy új üzenettípus) nem emel verziót: a fogadó az ismeretlen mezőt és üzenettípust figyelmen kívül hagyja. Meglévő mező jelentésének megváltoztatása vagy eltávolítása új verziót igényel; a szervernek átmenetileg a régit is támogatnia kell, amíg az agentek frissülnek.
- A jelenlegi kiadás az 1-es verziót beszéli.

### Élőség és újracsatlakozás

- A szerver a `Welcome` üzenetben adja meg a keepalive-időközt (alapértelmezés 30 s; az agent 1–300 s közé korlátozza). Az agent ennyi időnként `Ping` üzenetet küld. Ha a következő időpontig nem érkezik `Pong`, a kapcsolatot elveszettnek tekinti. Így egy néma hálózati kiesést legfeljebb két időközön belül észlel.
- A szerver bontja azt a kapcsolatot, amelyen három időköznyi ideig nem érkezik üzenet.
- Minden küldés időkorláthoz kötött, így a beragadt küldőpuffer is kiesésnek számít, és nem tartja fel a leállítást.
- Kiesés után az agent önállóan újracsatlakozik. A várakozás exponenciálisan nő 1 másodperctől 5 percig, a véletlen szórás az adott lépés felső felében van, hogy egy egyszerre leszakadt flotta ne egy ütemben térjen vissza. Legalább 60 másodpercig stabil kapcsolat után a várakozás újra a legrövidebbről indul.
- Minden feloldott címet külön időkorláttal próbál; Windowson az elutasított kapcsolat kb. 2 másodperc újrapróbálás után hiúsul meg, ez nem akadályozhatja a következő címet.
- Az agent állapota (`Connecting`, `Connected`, `Waiting` okkal és várakozási idővel, `Stopped`) figyelhető; a későbbi állapotnézet és tálcaikon (MIDAS-034) erre épülhet.

### Ellenőrzés

A `server/tests/agent_protocol.rs` valódi mTLS-kapcsolaton igazolja:

- eltérő támogatott tartományok (1–2 és 1–1, 1–2 és 2–3, 2–3 és 1–2) a legmagasabb közös verzión kapcsolódnak, és több keepalive-cikluson át üzenetet váltanak;
- közös verzió hiányában az agent a szerver tartományát tartalmazó elutasítást kap;
- hibás első üzenetet, érvénytelen tartományt és túl hosszú mezőt a szerver elutasít, majd bont;
- a néma kapcsolatot a szerver három időköz után bontja;
- megszakított, majd visszaállított hálózat után, valamint elnémuló kapcsolatnál az agent beavatkozás nélkül újracsatlakozik;
- a leállítás akkor is azonnali, ha a szerver a kézfogás közben nem válaszol.

A `shared/protocol` egységtesztjei az egyeztetést, az újabb mezők és üzenettípusok tűrését és a méretkorlátot, az `agent/agent-core` tesztjei a várakozási idő növekedését, felső korlátját és szórását vizsgálják.

### Ismert korlátok

- A Windows-szolgáltatás és a Linux-csomag, amelyek az agent-core kapcsolatát futtatják, a MIDAS-023 és a MIDAS-024 része.
- A kliens HTTP/1.1-es WebSocket-frissítést használ; a HTTP/2-n keresztüli WebSocket nem cél.

## English

November 7, 2026 · MIDAS-021

The agent connects to the server over one persistent, outbound, authenticated connection (PERF-01). The shared contract lives in [`shared/protocol`](../shared/protocol), connection handling in [`agent/agent-core`](../agent/agent-core), and the server side in `server/src/agent.rs`.

### Connection

- **Transport:** WebSocket on the mTLS listener at `/agent/connect` ([server startup](szerver.md#english)). Without a valid client certificate chaining to the agent CA, not even the TLS session is established. A request without a WebSocket upgrade receives 426.
- **Messages:** one protobuf `Envelope` (prost) per binary frame. A message is at most 64 KiB, also enforced by both sides at the WebSocket layer.
- **Authorization:** only a registered, unrevoked certificate issued at [enrollment](beiratkozas.md#english) may connect; revocation closes the live connection. The session alone authorizes no operation.

| Message | Direction | Content |
| --- | --- | --- |
| `Hello` | agent → server | supported version range, agent version (at most 64 bytes), platform (at most 32 bytes), platform capabilities (at most 32, [health checks](allapotellenorzes.md#english)) |
| `Welcome` | server → agent | negotiated version, keepalive interval in seconds, server version |
| `Reject` | server → agent | reason (`UNSUPPORTED_VERSION`, `INVALID_HELLO`) and the server's supported range; the server then closes |
| `Ping` / `Pong` | both directions | liveness check with a random identifier |
| `DirectoryAssignment` / `DirectoryReport` | server → designated connector and back | directory read task and its result ([directory connector](cimtar-connector.md#english)) |
| `MetricsBatch` / `MetricsAck` | agent → server and back | up to 60 minute samples and the acknowledgement after storage ([metrics](metrikak.md#english)) |
| `InventoryUpdate` / `InventoryAck` | agent → server and back | a chunk of a full inventory or change set and its acknowledgement ([inventory](leltar.md#english)) |
| `HealthReport` / `HealthAck` | agent → server and back | the latest check results and the acknowledgement after storage ([health checks](allapotellenorzes.md#english)) |
| `PolicyAssignment` | server → agent | the device's effective settings after inheritance; on connecting and on change ([policies](szabalyzatok.md#english)) |
| `RefreshRequest` | server → agent | an operator asked for current data: the agent checks at once, sends even an unchanged result and collects its inventory ([device pages](eszkozoldal.md#english)) |

### Versions and compatibility

- The agent's first message is `Hello`, expected within 10 seconds. The server picks the **highest version both ranges contain**, which stays in force for the session.
- Without a common version, the server's `Reject` carries its own range, so the agent can report why it failed. The agent then retries after the longest delay, because a server or agent update usually resolves the mismatch.
- **Compatible changes** (a new optional field or message type) keep the version: receivers ignore unknown fields and message types. Changing or removing the meaning of an existing field needs a new version, and the server must keep supporting the old one while agents update.
- The current release speaks version 1.

### Liveness and reconnection

- The server sets the keepalive interval in `Welcome` (default 30 s; the agent bounds it to 1–300 s). The agent sends a `Ping` every interval and treats the connection as lost if no `Pong` arrives by the next one, so silent network loss is detected within two intervals.
- The server closes a session that has been silent for three intervals.
- Every send is time-bounded, so a stalled send buffer counts as a loss and cannot delay stopping.
- After a loss the agent reconnects on its own. The delay doubles from 1 second up to 5 minutes, with jitter in the upper half of each step so a fleet disconnected at the same moment does not return in lockstep. After at least 60 seconds of stable connection the delay starts again from the shortest step.
- Each resolved address has its own time limit; on Windows a refused connection fails only after about two seconds of retries, which must not starve the next address.
- The agent status (`Connecting`, `Connected`, `Waiting` with reason and delay, `Stopped`) is observable; the later status view and tray icon (MIDAS-034) can build on it.

### Verification

`server/tests/agent_protocol.rs` verifies over real mTLS that:

- different supported ranges (1–2 with 1–1, 1–2 with 2–3, 2–3 with 1–2) connect on the highest common version and keep exchanging messages over several keepalive cycles;
- without a common version the agent receives a refusal carrying the server's range;
- the server refuses and closes on a wrong first message, an invalid range or an oversized field;
- the server closes a silent session after three intervals;
- the agent reconnects unaided after the network is cut and restored, and after the connection goes silent;
- stopping is immediate even when the server stays silent during the handshake.

Unit tests in `shared/protocol` cover negotiation, tolerance of newer fields and message types and the size limit; tests in `agent/agent-core` cover delay growth, its cap and jitter.

### Known limits

- The Windows service and Linux package that run the agent-core connection belong to MIDAS-023 and MIDAS-024.
- The client uses an HTTP/1.1 WebSocket upgrade; WebSocket over HTTP/2 is not a goal.
