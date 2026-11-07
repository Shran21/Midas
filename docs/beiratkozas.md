# Agentbeiratkozás / Agent enrollment

[English](#english)

## Magyar

2026. november 7. · MIDAS-022 · SEC-02

Az új agent helyben P-256 kulcspárt készít, és a szervert hitelesítő HTTPS-en, egy csak beiratkozást kiszolgáló végponton kér kliens-tanúsítványt. Tanúsítványt csak egyszer használható, ügyfélhez és célhoz kötött token, a kulcs birtoklásának igazolása, valamint jóváhagyás vagy előre jóváhagyott telepítési identitás után kap. A műveleti csatorna (`/agent/connect`) kizárólag érvényes, regisztrált és vissza nem vont tanúsítvánnyal érhető el.

### Folyamat

1. **Token:** a `device.enroll` joggal rendelkező kezelő (friss passkey-belépéssel) tokent kér a `POST /api/tenants/{ügyfél}/enrollment-tokens` végponton. A token ügyfélhez, opcionálisan telephelyhez, platformhoz (`windows`, `linux`) és géptípushoz (`workstation`, `server`) kötött, legfeljebb 720 órán át érvényes, és csak egyszer jelenik meg. A szerver csak a kivonatát tárolja. Az előre jóváhagyott (`preapproved`) token ellenőrzött telepítési identitásnak számít, például egy gépenként kiadott telepítőcsomagban.
2. **Kérelem:** az agent a `POST /agent/enroll` végpontra küldi a tokent, a PKCS#10 tanúsítványkérelmet, a platformot, a géptípust és a gépnevet. A szerver ellenőrzi a kérelem aláírását, vagyis a kulcs birtoklását; csak P-256 kulcsot fogad el. Rossz platform vagy géptípus esetén elutasít, a tokent viszont nem használja el. Felhasznált, lejárt vagy ismeretlen tokenre ugyanazt az elutasítást adja, az okot csak az audit rögzíti.
3. **Jóváhagyás:** előre jóváhagyott tokennél a válasz azonnal a tanúsítvány. Egyébként az eszköz függő állapotba kerül, és az agent egy egyszer megjelenő kérelemazonosítót és titkot kap. Ezekkel a `POST /agent/enroll/{azonosító}` végponton kérdezi le az állapotot, legfeljebb 30 napig. A kezelő a `GET /api/tenants/{ügyfél}/devices/pending` listából, amely csak a még érvényes kérelmeket mutatja, a `…/devices/{eszköz}/approve` vagy `…/reject` végponton dönt.
4. **Tanúsítvány:** a szerver saját profilt használ, a kérelemből csak a nyilvános kulcsot veszi át. A tanúsítvány tartalma: közös név az eszköz azonosítója, URI-név `urn:midas:device:{ügyfél}:{eszköz}`, csak kliens-hitelesítési kulcshasználat, nem CA, 365 napos érvényesség, véletlen sorozatszám. A szerver a tanúsítvány SHA-256 ujjlenyomatát tárolja.
5. **Műveleti csatorna:** a TLS-réteg az agent-CA láncát ellenőrzi. A szerver ezután a bemutatott tanúsítvány ujjlenyomatával megkeresi az aktív, vissza nem vont, le nem járt hitelesítő adatot és az aktív eszközt. Ha nincs ilyen, a kapcsolat még a WebSocket-frissítés előtt 403-mal zárul.
6. **Visszavonás:** a `…/devices/{eszköz}/revoke` végpont az eszköz minden hitelesítő adatát visszavonja, és a szerver az élő agentkapcsolatot azonnal bontja. Új kapcsolat a visszavont tanúsítvánnyal nem jön létre.

Minden token-kiadás, sikeres beiratkozási kérelem, jóváhagyás, elutasítás és visszavonás auditált, akárcsak az ismert tokenhez köthető elutasítás. Ismeretlen vagy hibás formátumú tokenre a szerver csak naplóbejegyzést ír, így hitelesítés nélküli kérések nem növelhetik az auditot. A token és a kérelemtitok nem kerül az auditba.

### Üzemeltetés

A beiratkozáshoz adatbázis és az agent-CA privát kulcsa kell: `--agent-ca agent-ca.pem --agent-ca-key agent-ca-key.pem`. A kulcsfájl csak a szolgáltatásfiók számára olvasható, kis méretű, szabályos fájl lehet. Agentfigyelő (`--agent-bind`) csak adatbázissal és CA-kulccsal indul. Az adminisztrációs végpontok a böngészős hitelesítéssel (`--auth-origin`) érhetők el.

### Ellenőrzés

- A `server/src/enrollment.rs` egységtesztjei a tanúsítványprofilt (a kérelemben kért CA-jog, szerverhasználat és név figyelmen kívül marad), a hamisított aláírású és a nem P-256 kulcsú kérelem elutasítását vizsgálják.
- A `server/tests/support/enrollment.rs` valódi PostgreSQL-lel és TLS-sel igazolja a rossz célú, a felhasznált, az ismeretlen és a lejárt token elutasítását, a hibás kérelmet, a jóváhagyás utáni tanúsítványkiadást és kapcsolódást, a nem regisztrált, de a CA által aláírt tanúsítvány elutasítását, a visszavonás utáni azonnali bontást és újracsatlakozási tilalmat, az elutasított kérelmet, a lejárt kérelem elrejtését és elutasítását, az előre jóváhagyott tokent, a bérlők elkülönítését, az auditot, valamint azt, hogy ismeretlen token nem növeli a platformszintű auditot.

### Ismert korlátok

- A tanúsítvány megújítása még nincs megvalósítva; a kiadott tanúsítvány 365 napig érvényes. A megújításnak újra kell ellenőriznie az eszköz állapotát.
- A kulcs és a tanúsítvány védett helyi tárolása a Windows-szolgáltatás (MIDAS-023) és a Linux-csomag (MIDAS-024), a telepítőcsomag védett tokenátadása a MIDAS-033 része.
- A függő eszközök és döntések webes felülete a MIDAS-038 része; addig az API használható.
- A TLS-réteg CRL-je induláskor töltődik be; a visszavonás azonnali hatását az alkalmazásszintű ujjlenyomat-ellenőrzés adja.

## English

November 7, 2026 · MIDAS-022 · SEC-02

A new agent creates a P-256 key pair locally and requests a client certificate over server-authenticated HTTPS from an endpoint that serves enrollment only. It receives a certificate only after a single-use token scoped to a customer and target, proof of key possession, and either approval or a preapproved deployment identity. The operational channel (`/agent/connect`) accepts only a valid, registered, unrevoked certificate.

### Flow

1. **Token:** an operator holding `device.enroll` (with a fresh passkey sign-in) requests a token at `POST /api/tenants/{tenant}/enrollment-tokens`. The token is bound to the customer, optionally a site, a platform (`windows`, `linux`) and a device kind (`workstation`, `server`), lasts at most 720 hours and is shown once. The server stores only its digest. A `preapproved` token counts as a verified deployment identity, for example inside a per-device installer package.
2. **Request:** the agent posts the token, a PKCS#10 request, platform, kind and device name to `POST /agent/enroll`. The server verifies the request signature, which proves key possession, and accepts only P-256 keys. A wrong platform or kind is refused without consuming the token. Used, expired and unknown tokens receive the same refusal; only the audit trail records the reason.
3. **Approval:** with a preapproved token the answer is the certificate. Otherwise the device becomes pending and the agent receives a request ID and secret, shown once, to poll `POST /agent/enroll/{id}` for up to 30 days. The operator decides from `GET /api/tenants/{tenant}/devices/pending`, which lists only unexpired requests, via `…/devices/{device}/approve` or `…/reject`.
4. **Certificate:** the server uses its own profile and takes only the public key from the request. The certificate has the device ID as common name, the URI name `urn:midas:device:{tenant}:{device}`, client authentication as its only extended key usage, no CA rights, 365-day validity and a random serial number. The server stores its SHA-256 fingerprint.
5. **Operational channel:** TLS verifies the agent CA chain. The server then looks up the presented certificate's fingerprint for an active, unrevoked, unexpired credential and an active device; otherwise the request ends with 403 before the WebSocket upgrade.
6. **Revocation:** `…/devices/{device}/revoke` revokes every credential of the device, and the server closes its live agent connection at once. The revoked certificate cannot connect again.

Token issuance, accepted enrollment requests, approvals, rejections and revocations are audited, as are refusals tied to a known token. Unknown or malformed tokens are only logged, so unauthenticated requests cannot grow the audit trail. Tokens and request secrets never reach the audit trail.

### Operations

Enrollment needs a database and the agent CA private key: `--agent-ca agent-ca.pem --agent-ca-key agent-ca-key.pem`. The key must be a small regular file readable only by the service account. The agent listener (`--agent-bind`) starts only with a database and the CA key. The administration routes require browser authentication (`--auth-origin`).

### Verification

- Unit tests in `server/src/enrollment.rs` cover the certificate profile (requested CA rights, server usage and names are ignored) and refusal of tampered signatures and non-P-256 keys.
- `server/tests/support/enrollment.rs` verifies with real PostgreSQL and TLS: refusal of wrong-target, used, unknown and expired tokens and of a malformed request; issuance and connection after approval; refusal of an unregistered certificate signed by the CA; immediate closure and no reconnection after revocation; a rejected request; hiding and refusing expired requests; a preapproved token; tenant isolation; audit records; and unknown tokens leaving the platform audit unchanged.

### Known limits

- Certificate renewal is not implemented yet; issued certificates last 365 days. Renewal must recheck device state.
- Protected local storage of the key and certificate belongs to the Windows service (MIDAS-023) and Linux package (MIDAS-024); protected token delivery in installer packages to MIDAS-033.
- The web view for pending devices and decisions is MIDAS-038; until then the API is available.
- The TLS-layer CRL loads at startup; the application-level fingerprint check makes revocation effective immediately.
