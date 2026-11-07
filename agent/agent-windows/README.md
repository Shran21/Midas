# Windows agent

A Windows-agent tervezett forráshelye. A rendszerjogú szolgáltatás, a felhasználói felület és a védett asztalt kezelő komponens külön jogosultsági határt kap. A rendszergazdai telepítés nem helyettesíti a távoli kapcsolat engedélyezését.

A `media-prototype` mappa a MIDAS-017 korai WebRTC- és kódoló-prototípusa: valódi asztalrögzítés, OpenH264, str0m és hálózatemulátor, csak loopbackon és hitelesítés nélkül. Eredménye a [mérési jegyzőkönyv](../../docs/meresek/webrtc-prototipus.md). Az `uac-prototype` a MIDAS-018 próbája: LocalSystem-szolgáltatás, amely a konzolmunkamenetben követi a bemeneti asztalt, így a böngészőből a biztonságos asztalon megjelenő UAC is látható és vezérelhető. Csak elkülönített tesztgépre; leírása és VM-es próbamenete: [UAC-próba](../../docs/meresek/uac-prototipus.md). Az éles agent megvalósítása még hátravan; a kapcsolatot, a verzióegyeztetést és az újracsatlakozást az [`agent-core`](../agent-core) adja ([agentprotokoll](../../docs/agent-protokoll.md)). Követelmények: [agent és távsegítés](../../docs/kovetelmenyek.md).

## English

Planned source directory for the Windows agent. The privileged service, user interface and secure-desktop component have separate permission boundaries. Administrative installation does not replace authorization for a remote session.

`media-prototype` is the MIDAS-017 early WebRTC and encoder proof: real desktop capture, OpenH264, str0m and a network emulator, loopback-only and unauthenticated. Its results are in the [measurement record](../../docs/meresek/webrtc-prototipus.md#english). `uac-prototype` is the MIDAS-018 proof: a LocalSystem service that follows the input desktop in the console session, so the UAC prompt on the secure desktop can be seen and controlled from the browser. Isolated test machines only; see the [UAC proof](../../docs/meresek/uac-prototipus.md#english) for its design and VM trial procedure. The production agent remains pending; connection, version negotiation and reconnection come from [`agent-core`](../agent-core) ([agent protocol](../../docs/agent-protokoll.md#english)). See the [agent and remote-support requirements](../../docs/kovetelmenyek.md#english).
