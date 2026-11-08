# Szerverindítás / Server startup

[English](#english)

## Magyar

A `just dev` továbbra is csak loopback HTTP-kiszolgálót indít. A HTTPS külön bekapcsolható, TLS 1.3 protokollal. A szerver egyelőre állapotvégpontot és elkülönített agentútvonalakat ad; nincs bejelentkezés, beiratkoztatás vagy távoli művelet.

### Saját tanúsítvány

```text
cargo run --locked -p midas-server -- --bind 127.0.0.1:8443 --tls-cert cert-chain.pem --tls-key private-key.pem
```

A PEM lánc a szervertanúsítvánnyal kezdődjön; a kulcsnak hozzá kell tartoznia. Hibás vagy hiányos beállításnál az indítás leáll, HTTP-re nem vált vissza. A HTTPS `/health/live` válasza `{"status":"ok"}`. A láncnak a kliensen megbízhatónak, a hosztnévnek a tanúsítványban szereplő névnek kell lennie; a kliens ellenőrzését ne kapcsold ki.

Az agentcsatorna külön címet/portot kap:

```text
cargo run --locked -p midas-server -- --bind 127.0.0.1:8443 --tls-cert cert-chain.pem --tls-key private-key.pem --agent-bind 127.0.0.1:8444 --agent-ca agent-ca.pem --agent-ca-key agent-ca-key.pem --agent-crl revoked-clients.pem
```

A `--agent-ca` kötelező az agentfigyelőhöz. Csak a megadott CA-ra visszavezethető, érvényes kliens-tanúsítvánnyal lehet TLS-kapcsolatot létesíteni. A választható `--agent-crl` PEM visszavonási listát ad; üres, hibás vagy lejárt lista nem eredményez engedékenyebb ellenőrzést. A CA és a CRL induláskor töltődik be; változásukhoz újraindítás kell. A `--agent-ca-key` az agent-CA privát kulcsa, ez kapcsolja be a beiratkozást; agentfigyelő csak adatbázissal és CA-kulccsal indul. A szervertanúsítvány mindkét eléréshez közös, ezért a használt hosztneveket tartalmaznia kell.

| Elérés | Útvonal | Jelenlegi működés |
| --- | --- | --- |
| HTTPS | GET `/health/live` | A kiszolgáló működését jelző 200 válasz |
| HTTPS | POST `/agent/enroll`, POST `/agent/enroll/{id}` | Agentbeiratkozás egyszer használható tokennel és tanúsítványkérelemmel ([beiratkozás](beiratkozas.md)); CA-kulcs nélkül 503 |
| mTLS | GET `/agent/connect` | WebSocket-alapú agentkapcsolat verzióegyeztetéssel és keepalive-val ([agentprotokoll](agent-protokoll.md)), csak regisztrált, vissza nem vont eszköztanúsítvánnyal; frissítés nélküli kérésre 426 |

A beiratkozási útvonal az mTLS-figyelőn, a műveleti útvonal a webes figyelőn nem létezik. A tanúsítvány elfogadása önmagában nem ad műveleti jogosultságot: a szerver a regisztrált eszközhöz, annak ügyfeléhez és állapotához köti a kapcsolatot, visszavonáskor pedig az élő kapcsolatot is bontja. A műveletek jogosultságkezelése további feladat.

### ACME

A beépített kliens TLS-ALPN-01 ellenőrzést használ; a domainek DNS-ének erre a szerverre kell mutatnia, és a nyilvános 443-as portnak a HTTPS-figyelőhöz kell eljutnia. Külön TLS-lezáró proxy ebben a módban nem használható. Alapértelmezés a Let's Encrypt tesztkörnyezete. A `--acme-production` csak külön, tudatos választással kapcsolja be az éles kibocsátót.

Hozz létre egy kizárólag a szolgáltatás fiókja és a rendszergazda által elérhető cache-mappát. Unix alatt az indítás elutasítja a csoportnak vagy más felhasználónak hozzáférést adó jogosultságokat. Windows alatt a mappa NTFS ACL-jét az üzemeltetőnek kell beállítania. A cache privát kulcsot és ACME-fiókadatot is tartalmaz; ne kerüljön Gitbe vagy a webes fájlok közé.

```text
cargo run --locked -p midas-server -- --bind 0.0.0.0:443 --acme-domain midas.example.com --acme-contact admin@example.com --acme-cache /private/midas-acme --acme-accept-terms
```

A példa helykitöltő domainekkel szerepel. A `--acme-accept-terms` a kibocsátó feltételeinek elfogadását jelzi; csak azok áttekintése után használd. Több hosztnévhez ismételd a `--acme-domain` opciót. A tanúsítvány megszerzése és megújítása háttérben történik; cache-elt tanúsítvány újra felhasználható. Az agentfigyelő ugyanazt a megújuló tanúsítványt használja. Sikertelen igényléskor nincs HTTP-visszaesés, és nincs tanúsítványellenőrzést megkerülő út.

### Riasztási e-mail

A riasztási értesítéseket a szerver SMTP-n küldi (`--smtp-host`, `--smtp-port`, `--smtp-security`, `--smtp-from`, választhatóan `--smtp-user` és `--smtp-password-file`). Beállítás nélkül a levelek sorban várnak. Részletek: [riasztások](riasztasok.md).

### Naplózás és próbák

A JSON-napló a figyelő indulását/leállását, az ACME állapotváltozását, a statikus útvonal nevét, a válaszkódot és a kiszolgálási időt rögzíti. URL-paraméter, kérésbody, fejléc és privát kulcs nem kerül az alkalmazás kérésnaplójába. Az ACME hibák tartalma helyett általános hibaesemény szerepel. Az OpenTelemetry-export külön későbbi integráció.

A tesztek valódi helyi TLS-kapcsolatot, HTTPS-állapotválaszt, helyes/idegen/hiányzó/visszavont kliens-tanúsítványt, hibás kulcspárt és útvonal-elkülönítést vizsgálnak. Nyilvános ACME-igénylés és megújítás tényleges DNS-sel még nincs igazolva; telepítési próba szükséges. PFX-import még nincs, PEM-bemenet támogatott.

## English

`just dev` remains loopback-only HTTP. HTTPS uses TLS 1.3 and is explicitly enabled with `--tls-cert` and `--tls-key`. The chain starts with the server certificate; its private key must match. Invalid configuration fails startup without falling back to HTTP. Clients must trust the chain and verify the hostname.

Add `--agent-bind` and `--agent-ca` for a separate mandatory-client-certificate listener; `--agent-ca-key` is the agent CA private key that enables enrollment, and the agent listener requires a database and that key. Optional `--agent-crl` loads PEM revocation lists and rejects invalid or expired revocation data. CA/CRL changes require restart. Both listeners share the server certificate, which must cover their hostnames.

HTTPS serves GET `/health/live` and agent enrollment at POST `/agent/enroll` ([enrollment](beiratkozas.md#english)), returning 503 without the CA key. The mTLS listener serves the agent WebSocket at GET `/agent/connect` with version negotiation and keepalive ([agent protocol](agent-protokoll.md#english)); a request without an upgrade receives 426, and only registered, unrevoked device certificates may connect. Neither listener exposes the other's agent route. Certificate authentication does not authorize operations. Enrollment, per-device state, tenant mapping, active-session revocation and application permissions remain subsequent work.

Alert notifications go out over SMTP (`--smtp-host`, `--smtp-port`, `--smtp-security`, `--smtp-from`, optionally `--smtp-user` and `--smtp-password-file`); without it messages stay queued ([alerts](riasztasok.md#english)).

ACME uses TLS-ALPN-01 with Let's Encrypt staging by default. DNS must point to this server and public port 443 must reach the HTTPS listener without a TLS-terminating proxy. Repeat `--acme-domain` for all required names. `--acme-contact`, `--acme-cache` and explicit `--acme-accept-terms` are required; review the issuer's terms before using that flag. `--acme-production` explicitly selects the production issuer. See the command examples above, replacing their placeholder names and paths.

Pre-create a private cache accessible only to the service account and administrators. Unix startup rejects group/other access; Windows operators must configure its NTFS ACL. It contains private keys and account material and must stay outside Git and web roots. Certificate issuance and renewal run in the background; both listeners use the same renewing certificate. Failure never enables plaintext or skips client verification.

JSON logs contain listener lifecycle, ACME state changes, static route names, HTTP status and elapsed time. Application request logs omit URL parameters, bodies, headers and private keys. ACME failures use a generic event instead of logging their contents. OpenTelemetry export is a subsequent integration.

Tests exercise real local TLS, HTTPS liveness, accepted/missing/foreign/revoked client certificates, mismatched keys and route separation. Live public ACME issuance and renewal with actual DNS still require deployment validation. PEM input is supported; PFX import is not yet implemented.
