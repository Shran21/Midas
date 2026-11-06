# 0002 — Távsegítési transzport / Remote support transport

2026-10-31 · Elfogadva, integrációs próba szükséges / Accepted, integration trial required

## Magyar

A távsegítés képét és bevitelét WebRTC továbbítja a natív komponens és a böngésző között. A jelzéscsere hitelesített szerverkapcsolaton történik. Közvetlen kapcsolat hiányában eturnal TURN-relét használunk.

A médiát DTLS-SRTP védi a hálózati megfigyelőtől és a relétől. A jelzéskiszolgáló és a dashboardot kiszolgáló szerver továbbra is a bizalmi modell része. A kapcsolat létrejötte nem helyettesíti a helyi beleegyezést vagy az előzetesen engedélyezett szerveres szabályzatot.

A konkrét Rust-integrációt a 0. fázis próbája választja ki. Vizsgálni kell a külső kódoló illesztését, az adatcsatornát, a kapcsolat megszakítását, valamint a normál munkamenetből kezelt valódi Windows UAC-t. A próba a webrtc-rs és a libwebrtc integrációs lehetőségeit veti össze.

A képrögzítés és a kódolás csak engedélyezett munkamenet alatt futhat. A késleltetés elsőbbséget kap, a sorok és pufferek korlátosak. A mérések a közvetlen és TURN-útvonalat, a hardveres és szoftveres kódolást is lefedik.

### Döntés a 0. fázis próbája alapján (2026-11-06)

A [MIDAS-017 mérési jegyzőkönyve](../meresek/webrtc-prototipus.md) alapján a natív komponens a str0m könyvtárat használja: tisztán Rust, sans-IO, a UDP-, TURN- és sorkezelés a MIDAS kezében marad, a külső kódoló és az adatcsatorna közvetlenül illeszthető. A libwebrtc tartalék marad, ha a MIDAS-058 valódi hálózati mátrixa a MIDAS-046 javításai után sem teljesíti a feltételeket. A döntést a 0. fázis zárása (MIDAS-020) felülvizsgálja.

## English

WebRTC carries remote video and input between the native component and browser. Signaling uses an authenticated server connection. Use an eturnal TURN relay when a direct connection is unavailable.

DTLS-SRTP protects media from network observers and the relay. Signaling and dashboard servers remain part of the trust model. Establishing transport does not replace local consent or an explicitly authorized unattended-server policy.

The phase 0 trial selects the Rust integration. Verify external encoder integration, data channels, termination and real Windows UAC interaction within a normal session. Compare integration options for webrtc-rs and libwebrtc.

Capture and encoding run only during authorized sessions. Prioritize latency while bounding queues and buffers. Measurements cover direct and relayed connections plus hardware and software encoding.

### Decision from the phase 0 trial (2026-11-06)

Based on the [MIDAS-017 measurement record](../meresek/webrtc-prototipus.md#english), the native component uses str0m: pure Rust, sans-IO, with UDP, TURN and queue handling under MIDAS control and direct external encoder and data channel integration. libwebrtc remains the fallback if the MIDAS-058 real network matrix still fails the criteria after the MIDAS-046 improvements. The phase 0 closure (MIDAS-020) reviews this decision.
