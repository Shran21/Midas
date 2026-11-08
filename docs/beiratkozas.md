# Agentbeiratkozás / Agent enrollment

[English](#english)

## Magyar

2026. november 7. · MIDAS-022 · SEC-02

Az új agent helyben P-256 kulcspárt készít, és a szervert hitelesítő HTTPS-en, egy csak beiratkozást kiszolgáló végponton kér kliens-tanúsítványt. Tanúsítványt csak egyszer használható, ügyfélhez és célhoz kötött token, a kulcs birtoklásának igazolása, valamint jóváhagyás vagy előre jóváhagyott telepítési identitás után kap. A műveleti csatorna (`/agent/connect`) kizárólag érvényes, regisztrált és vissza nem vont tanúsítvánnyal érhető el.

### Folyamat

1. **Token:** a `device.enroll` joggal rendelkező kezelő (friss passkey-belépéssel) tokent kér a `POST /api/tenants/{ügyfél}/enrollment-tokens` végponton. A token ügyfélhez, opcionálisan telephelyhez, platformhoz (`windows`, `linux`) és géptípushoz (`workstation`, `server`) kötött, legfeljebb 720 órán át érvényes, és csak egyszer jelenik meg. A szerver csak a kivonatát tárolja. Az előre jóváhagyott (`preapproved`) token ellenőrzött telepítési identitásnak számít, például egy gépenként kiadott telepítőcsomagban. Központi telepítéshez (GPO, Intune) a token legfeljebb 1000 gépre adható ki (`uses`); az ilyen token nem lehet előre jóváhagyott, minden gép jóváhagyásra vár, és a webpanelen csoportosan jóváhagyható.
2. **Kérelem:** az agent a `POST /agent/enroll` végpontra küldi a tokent, a PKCS#10 tanúsítványkérelmet, a platformot, a géptípust és a gépnevet. A szerver ellenőrzi a kérelem aláírását, vagyis a kulcs birtoklását; csak P-256 kulcsot fogad el. Rossz platform vagy géptípus esetén elutasít, a tokent viszont nem használja el. Felhasznált, lejárt vagy ismeretlen tokenre ugyanazt az elutasítást adja, az okot csak az audit rögzíti.
3. **Jóváhagyás:** előre jóváhagyott tokennél a válasz azonnal a tanúsítvány. Egyébként az eszköz függő állapotba kerül, és az agent egy egyszer megjelenő kérelemazonosítót és titkot kap. Ezekkel a `POST /agent/enroll/{azonosító}` végponton kérdezi le az állapotot, legfeljebb 30 napig. A kezelő a `GET /api/tenants/{ügyfél}/devices/pending` listából, amely csak a még érvényes kérelmeket mutatja, a `…/devices/{eszköz}/approve` vagy `…/reject` végponton dönt.
4. **Tanúsítvány:** a szerver saját profilt használ, a kérelemből csak a nyilvános kulcsot veszi át. A tanúsítvány tartalma: közös név az eszköz azonosítója, URI-név `urn:midas:device:{ügyfél}:{eszköz}`, csak kliens-hitelesítési kulcshasználat, nem CA, 365 napos érvényesség, véletlen sorozatszám. A szerver a tanúsítvány SHA-256 ujjlenyomatát tárolja.
5. **Műveleti csatorna:** a TLS-réteg az agent-CA láncát ellenőrzi. A szerver ezután a bemutatott tanúsítvány ujjlenyomatával megkeresi az aktív, vissza nem vont, le nem járt hitelesítő adatot és az aktív eszközt. Ha nincs ilyen, a kapcsolat még a WebSocket-frissítés előtt 403-mal zárul.
6. **Visszavonás:** a `…/devices/{eszköz}/revoke` végpont az eszköz minden hitelesítő adatát visszavonja, és a szerver az élő agentkapcsolatot azonnal bontja. Új kapcsolat a visszavont tanúsítvánnyal nem jön létre.

Minden token-kiadás, sikeres beiratkozási kérelem, jóváhagyás, elutasítás és visszavonás auditált, akárcsak az ismert tokenhez köthető elutasítás. Ismeretlen vagy hibás formátumú tokenre a szerver csak naplóbejegyzést ír, így hitelesítés nélküli kérések nem növelhetik az auditot. A token és a kérelemtitok nem kerül az auditba.

### Windows-telepítőcsomag (MSI)

A `agent/agent-windows/installer/midas-agent.wxs` (WiX Toolset v3) a `%ProgramFiles%\MIDAS\Agent` mappába telepíti az agentet, és natívan regisztrálja az automatikusan induló, hibánál újrainduló `MidasAgent` szolgáltatást. Felépítés: `python .github/scripts/check-msi.py` (`just release` után).

| MSI-tulajdonság | Jelentés |
| --- | --- |
| `MIDAS_SERVER` | a szerver gépneve (kötelező) |
| `MIDAS_SERVER_CA` | a szerver CA-ját tartalmazó PEM-fájl útvonala (kötelező) |
| `MIDAS_PORT`, `MIDAS_ENROLLMENT_PORT` | agent- és beiratkozási port (alapból 8444 és 443) |
| `MIDAS_KIND` | `workstation` vagy `server` |
| `MIDAS_TOKEN_FILE` | a beiratkozási tokent tartalmazó fájl útvonala |

- A token soha nem MSI-tulajdonság-érték és nem parancssori argumentum: az MSI csak a fájl útvonalát adja át. Telepítéskor az agent `configure` parancsa (rendszerjogon, a fájlok másolása után) beolvassa a tokent, és a csak SYSTEM és Administrators számára hozzáférhető adatkönyvtárba (`%ProgramData%\MIDAS\agent\enrollment-token`) helyezi.
- A szolgáltatás indításkor ezzel iratkozik be. Ha a szerver dönt (kiadta a tanúsítványt, jóváhagyásra vár vagy elutasította), a tokent törli; elérhetetlen szervernél megtartja és később újrapróbál. Már beiratkozott gép újratelepítése vagy frissítése nem használja fel újra a tokent.
- **Kézi telepítés:** egygépes tokent egy csak a rendszergazda által olvasható fájlba írva: `msiexec /i midas-agent.msi MIDAS_SERVER=midas.example MIDAS_SERVER_CA=C:\Telepites\ca.pem MIDAS_TOKEN_FILE=C:\Telepites\token.txt`. Utána a fájl törölhető.
- **Központi telepítés (GPO, Intune):** többgépes (`uses`) token egy olyan helyen, amelyet csak a célgépek számítógépfiókja olvashat (például korlátozott megosztás vagy az Intune Win32-alkalmazás csomagja); a gépek jóváhagyásra várnak, és a webpanelen csoportosan jóváhagyhatók.
- A CI a Windows-futón felépíti és ténylegesen telepíti az MSI-t egy próbatokennel, és ellenőrzi:
  - a token nincs benne az MSI részletes naplójában és egyetlen folyamat parancssorában sem;
  - a token a védett adatkönyvtárban van, felhasználói hozzáférés nélkül;
  - a szolgáltatás fut;
  - eltávolításkor a szolgáltatás megszűnik.

### Webpanel: Felderítés és regisztráció

A **Felderítés és regisztráció** menüpont ügyfélszintű `device.enroll` vagy `directory.discover` joggal látszik. A nézet elején figyelmeztetés áll: a felderítés nem telepít agentet, nem léptet tartományba és nem módosít címtárat; agent telepítőcsomaggal, GPO-val, Intune-nal vagy jóváhagyott telepítési feladattal kerül a gépre.

- **Jóváhagyásra váró regisztrációk** (`device.enroll`): a kérelmek géppel, rendszerrel, típussal és a kérés idejével. Jóváhagyás és elutasítás egyenként vagy kijelöléssel csoportosan. A csoportos döntés előtt a megerősítő ablak megnevezi az ügyfelet és felsorolja az érintett gépeket, utána a nézet gépenként mutatja az eredményt, az esetleges hibával (például friss passkey hiánya) együtt.
- **Telepítőtoken** (`device.enroll`): telephely (ha a technikus látja a telephelyeket), rendszer, géptípus, érvényesség (1–720 óra) és az előre jóváhagyás választása. A token csak egyszer, a létrehozás után látszik, másolható és elrejthető. A nézet figyelmeztet, hogy a token telepítőcsomagban vagy védett fájlban menjen, ne URL-ben, parancssorban vagy e-mailben.
- **Címtárban felderített gépek** (`directory.discover`): a connector tartománya és utolsó vizsgálata, majd a gépek keresővel és állapotszűrővel (felderített, agent nincs; kezelt; ütközés; megszűnt). Kezelt gépnél az igazolt eszköz neve az eszközoldalra visz; az azonos nevű, de nem igazolt eszköz csak figyelmeztető tippként szerepel ([címtár-connector](cimtar-connector.md)).
- **Regisztrációs döntések** (`device.enroll`): a legutóbbi 100 jóváhagyás, elutasítás és visszavonás a döntéshozóval; az előre jóváhagyott token döntés nélküli kiadásként látszik. API: `GET /api/tenants/{ügyfél}/devices/decisions` (ügyfélszintű `device.enroll`).
- A nézet a munkamenet-csatorna változásértesítésére frissül ([eszközoldal](eszkozoldal.md)).

### Üzemeltetés

A beiratkozáshoz adatbázis és az agent-CA privát kulcsa kell: `--agent-ca agent-ca.pem --agent-ca-key agent-ca-key.pem`. A kulcsfájl csak a szolgáltatásfiók számára olvasható, kis méretű, szabályos fájl lehet. Agentfigyelő (`--agent-bind`) csak adatbázissal és CA-kulccsal indul. Az adminisztrációs végpontok a böngészős hitelesítéssel (`--auth-origin`) érhetők el.

### Ellenőrzés

- A `server/src/enrollment.rs` egységtesztjei a tanúsítványprofilt (a kérelemben kért CA-jog, szerverhasználat és név figyelmen kívül marad), a hamisított aláírású és a nem P-256 kulcsú kérelem elutasítását vizsgálják.
- A `server/tests/support/enrollment.rs` valódi PostgreSQL-lel és TLS-sel igazolja a rossz célú, a felhasznált, az ismeretlen és a lejárt token elutasítását, a hibás kérelmet, a jóváhagyás utáni tanúsítványkiadást és kapcsolódást, a nem regisztrált, de a CA által aláírt tanúsítvány elutasítását, a visszavonás utáni azonnali bontást és újracsatlakozási tilalmat, az elutasított kérelmet, a lejárt kérelem elrejtését és elutasítását, az előre jóváhagyott tokent, a bérlők elkülönítését, az auditot, valamint azt, hogy ismeretlen token nem növeli a platformszintű auditot.

### Ismert korlátok

- A tanúsítvány megújítása még nincs megvalósítva; a kiadott tanúsítvány 365 napig érvényes. A megújításnak újra kell ellenőriznie az eszköz állapotát.
- A kulcs és a tanúsítvány védett helyi tárolása a Windows-szolgáltatás (MIDAS-023) és a Linux-csomag (MIDAS-024) része.
- Az MSI kódaláírása még nincs; éles terjesztés előtt aláírt csomag kell. Valódi tartományi GPO- és Intune-telepítést a felhasználó saját próbája igazol.
- A TLS-réteg CRL-je induláskor töltődik be; a visszavonás azonnali hatását az alkalmazásszintű ujjlenyomat-ellenőrzés adja.

## English

November 7, 2026 · MIDAS-022 · SEC-02

A new agent creates a P-256 key pair locally and requests a client certificate over server-authenticated HTTPS from an endpoint that serves enrollment only. It receives a certificate only after a single-use token scoped to a customer and target, proof of key possession, and either approval or a preapproved deployment identity. The operational channel (`/agent/connect`) accepts only a valid, registered, unrevoked certificate.

### Flow

1. **Token:** an operator holding `device.enroll` (with a fresh passkey sign-in) requests a token at `POST /api/tenants/{tenant}/enrollment-tokens`. The token is bound to the customer, optionally a site, a platform (`windows`, `linux`) and a device kind (`workstation`, `server`), lasts at most 720 hours and is shown once. The server stores only its digest. A `preapproved` token counts as a verified deployment identity, for example inside a per-device installer package. For managed deployment (GPO, Intune) a token may cover up to 1000 devices (`uses`); such a token cannot be preapproved, every device waits for approval, and they can be approved in bulk in the web panel.
2. **Request:** the agent posts the token, a PKCS#10 request, platform, kind and device name to `POST /agent/enroll`. The server verifies the request signature, which proves key possession, and accepts only P-256 keys. A wrong platform or kind is refused without consuming the token. Used, expired and unknown tokens receive the same refusal; only the audit trail records the reason.
3. **Approval:** with a preapproved token the answer is the certificate. Otherwise the device becomes pending and the agent receives a request ID and secret, shown once, to poll `POST /agent/enroll/{id}` for up to 30 days. The operator decides from `GET /api/tenants/{tenant}/devices/pending`, which lists only unexpired requests, via `…/devices/{device}/approve` or `…/reject`.
4. **Certificate:** the server uses its own profile and takes only the public key from the request. The certificate has the device ID as common name, the URI name `urn:midas:device:{tenant}:{device}`, client authentication as its only extended key usage, no CA rights, 365-day validity and a random serial number. The server stores its SHA-256 fingerprint.
5. **Operational channel:** TLS verifies the agent CA chain. The server then looks up the presented certificate's fingerprint for an active, unrevoked, unexpired credential and an active device; otherwise the request ends with 403 before the WebSocket upgrade.
6. **Revocation:** `…/devices/{device}/revoke` revokes every credential of the device, and the server closes its live agent connection at once. The revoked certificate cannot connect again.

Token issuance, accepted enrollment requests, approvals, rejections and revocations are audited, as are refusals tied to a known token. Unknown or malformed tokens are only logged, so unauthenticated requests cannot grow the audit trail. Tokens and request secrets never reach the audit trail.

### Windows installer package (MSI)

`agent/agent-windows/installer/midas-agent.wxs` (WiX Toolset v3) installs the agent into `%ProgramFiles%\MIDAS\Agent` and natively registers the `MidasAgent` service, starting automatically and restarting on failure. Build: `python .github/scripts/check-msi.py` (after `just release`).

| MSI property | Meaning |
| --- | --- |
| `MIDAS_SERVER` | server host name (required) |
| `MIDAS_SERVER_CA` | path of the PEM file with the server CA (required) |
| `MIDAS_PORT`, `MIDAS_ENROLLMENT_PORT` | agent and enrollment ports (8444 and 443 by default) |
| `MIDAS_KIND` | `workstation` or `server` |
| `MIDAS_TOKEN_FILE` | path of the file holding the enrollment token |

- The token is never an MSI property value or a command-line argument: the MSI passes only the file path. During installation the agent's `configure` command (as SYSTEM, after the files are copied) reads the token and places it in the data directory readable only by SYSTEM and Administrators (`%ProgramData%\MIDAS\agent\enrollment-token`).
- The service enrolls with it on start. Once the server decided (issued a certificate, put the device on hold or refused it) the token is deleted; with an unreachable server it is kept and retried later. Reinstalling or upgrading an enrolled device does not use the token again.
- **Manual installation:** a single-device token in a file readable only by the administrator: `msiexec /i midas-agent.msi MIDAS_SERVER=midas.example MIDAS_SERVER_CA=C:\Setup\ca.pem MIDAS_TOKEN_FILE=C:\Setup\token.txt`. The file can be deleted afterwards.
- **Managed deployment (GPO, Intune):** a multi-device (`uses`) token in a place only the target computer accounts can read (for example a restricted share or the Intune Win32 app package); the devices wait for approval and can be approved in bulk in the web panel.
- CI builds the MSI on the Windows runner and actually installs it with a test token, then checks:
  - the token is in neither the verbose MSI log nor any process command line;
  - the token sits in the protected data directory without user access;
  - the service runs;
  - removal deletes the service.

### Web panel: Discovery and enrollment

The **Discovery and enrollment** section appears with tenant-wide `device.enroll` or `directory.discover`. It opens with a reminder: discovery never installs agents, joins domains or changes the directory; agents are deployed with a package, GPO, Intune or an approved deployment job.

- **Enrollments waiting for approval** (`device.enroll`): requests with device, system, kind and request time. Approve or reject one by one or several selected at once. Before a bulk decision the confirmation names the customer and lists the affected devices; afterwards the view shows each device's result, including any error (such as a missing fresh passkey).
- **Deployment token** (`device.enroll`): site (when the operator can list sites), system, kind, lifetime (1–720 hours) and preapproval. The token is shown only once, right after creation, and can be copied or hidden. The view warns to hand it over in an installer package or a protected file, never in a URL, command line or e-mail.
- **Computers discovered in the directory** (`directory.discover`): the connector's domain and last scan, then computers with search and a status filter (discovered, no agent; managed; conflict; removed). For a managed computer the proven device name leads to its device page; a device with the same name but no proof appears only as a warning hint ([directory connector](cimtar-connector.md#english)).
- **Enrollment decisions** (`device.enroll`): the latest 100 approvals, rejections and revocations with who decided; a preapproved token shows as issued without a decision. API: `GET /api/tenants/{tenant}/devices/decisions` (tenant-wide `device.enroll`).
- The view refreshes on change notices from the session channel ([device pages](eszkozoldal.md#english)).

### Operations

Enrollment needs a database and the agent CA private key: `--agent-ca agent-ca.pem --agent-ca-key agent-ca-key.pem`. The key must be a small regular file readable only by the service account. The agent listener (`--agent-bind`) starts only with a database and the CA key. The administration routes require browser authentication (`--auth-origin`).

### Verification

- Unit tests in `server/src/enrollment.rs` cover the certificate profile (requested CA rights, server usage and names are ignored) and refusal of tampered signatures and non-P-256 keys.
- `server/tests/support/enrollment.rs` verifies with real PostgreSQL and TLS: refusal of wrong-target, used, unknown and expired tokens and of a malformed request; issuance and connection after approval; refusal of an unregistered certificate signed by the CA; immediate closure and no reconnection after revocation; a rejected request; hiding and refusing expired requests; a preapproved token; tenant isolation; audit records; and unknown tokens leaving the platform audit unchanged.

### Known limits

- Certificate renewal is not implemented yet; issued certificates last 365 days. Renewal must recheck device state.
- Protected local storage of the key and certificate belongs to the Windows service (MIDAS-023) and Linux package (MIDAS-024).
- The MSI is not code-signed yet; production distribution needs a signed package. Real domain GPO and Intune deployment is confirmed by the user's own trial.
- The TLS-layer CRL loads at startup; the application-level fingerprint check makes revocation effective immediately.
