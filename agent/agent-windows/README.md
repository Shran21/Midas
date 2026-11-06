# Windows agent

A Windows-agent tervezett forráshelye. A rendszerjogú szolgáltatás, a felhasználói felület és a védett asztalt kezelő komponens külön jogosultsági határt kap. A rendszergazdai telepítés nem helyettesíti a távoli kapcsolat engedélyezését.

A `media-prototype` mappa a MIDAS-017 korai WebRTC- és kódoló-prototípusa: valódi asztalrögzítés, OpenH264, str0m és hálózatemulátor, csak loopbackon és hitelesítés nélkül. Eredménye a [mérési jegyzőkönyv](../../docs/meresek/webrtc-prototipus.md). Az éles agent megvalósítása még hátravan. Követelmények: [agent és távsegítés](../../docs/kovetelmenyek.md).

## English

Planned source directory for the Windows agent. The privileged service, user interface and secure-desktop component have separate permission boundaries. Administrative installation does not replace authorization for a remote session.

`media-prototype` is the MIDAS-017 early WebRTC and encoder proof: real desktop capture, OpenH264, str0m and a network emulator, loopback-only and unauthenticated. Its results are in the [measurement record](../../docs/meresek/webrtc-prototipus.md#english). The production agent remains pending. See the [agent and remote-support requirements](../../docs/kovetelmenyek.md#english).
