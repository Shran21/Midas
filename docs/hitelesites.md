# Hitelesítés / Authentication

[English](#english)

## Magyar

A `midas_server::auth` belső szolgáltatás passkeyt, jelszó+TOTP belépést és egyszer használható helyreállító kódot kezel. PostgreSQL-ben tárolja a technikusi identitásokat, a hitelesítőket és a próbálkozáskorlátokat. A [böngészős munkamenet, HTTP-végpontok és CSRF-védelem](munkamenetek.md) már bekapcsolhatók a HTTPS-szerveren. A belépőfelület és első beállítás külön feladat; a webpanel továbbra is előnézet.

### Identitás és hozzáférés

A technikus identitása több ügyfélhez kapcsolódhat, ezért az `operators`, `passkeys`, `recovery_codes` és `authentication_attempts` táblák globális hitelesítési adatok. Nem tartalmaznak ügyféloldali eszközadatot, és nem a tenant-RLS választja ki a bejelentkező felhasználót. A korlátozott futtatófiók éri el őket; a későbbi RBAC ellenőrzi külön, melyik ügyfélhez, géphez és művelethez van joga a hitelesített technikusnak.

Az `Authenticated` eredményt csak sikeres ellenőrzés állítja elő. Tartalmazza a technikus azonosítóját, a hitelesítők verzióját, az ellenőrzés időpontját és módját: `passkey`, `password_totp` vagy `recovery_code`. A tartalék mód nem jelent friss passkey-megerősítést. A munkamenet- és műveleti rétegnek ezt, az érvényességet és a fiók aktuális állapotát is ellenőriznie kell.

A fióklétrehozás, passkey-regisztráció indítása, TOTP-beállítás és helyreállítókód-csere kizárólag megbízható belső beállítási vagy adminisztrációs művelet. Ezeket tilos jogosultságellenőrzés nélkül HTTP-végpontként bekötni. Nincs nyitott regisztráció vagy alapértelmezett fiók/jelszó. Hitelesítőváltozás növeli a fiók verzióját; a korábbi verzióval indult passkey-folyamat nem fejezhető be.

### Passkey

A `webauthn-rs` ellenőrzi a regisztrációt és a kriptográfiai aláírást, kötelező felhasználó-ellenőrzéssel. Az RP ID a konfigurált HTTPS-origin pontos hosztneve; más port, aldomain vagy origin nem kap automatikus felhatalmazást. Az origint a végleges telepítési címmel kell megadni, mert változtatása érinti a regisztrált hitelesítőket.

A szerveroldali kihívásállapot nem kerül a böngészőhöz. Véletlen azonosító és külön, 32 bájtos véletlen böngészőkötés kapcsolja össze a folyamat két felét. A HTTP-réteg ezt védett, rövid életű kötősütiből oldja fel; nem fogad el tetszőlegesen megadott JSON-mezőt böngészőkötésként. A kihívás alapértelmezetten öt percig él, egyszer használható, sikertelen befejezéskor is elfogy. Újraindításkor a folyamatot újra kell kezdeni.

A kihívástár alapértelmezett kapacitása 1024, állítható. Telítettségkor az új kérés átmeneti hibát kap, a már kiadott érvényes kérést nem dobjuk el. Ez a függő hitelesítési kérések védelme, nem a platform erőforrásplafonja. A tár egy szerverfolyamathoz tartozik; több példány esetén ugyanahhoz a példányhoz kell irányítani a folyamatot, vagy később közös, szerveroldali tárolást kell bevezetni. Fiókonként legfeljebb 32 passkey regisztrálható.

A hitelesítő azonosítója globálisan egyedi. A számláló és a mentési állapot frissítése adatbázis-tranzakcióban történik. Ha a hitelesítő számlálót használ, egy régebbi, párhuzamosan előállított válasz sem írhatja vissza a már elfogadott értéket. A szinkronizált, nulla számlálójú passkeyket a kihívás egyszerisége védi.

### Tartalék belépés és helyreállítás

A helyi jelszó Argon2id-hasht kap egyedi véletlen sóval, `m=19456 KiB`, `t=2`, `p=1` paraméterekkel. A jelszó legalább 12 karakter; Unicode és szóköz megengedett, nincs kötelező karakterkeverék. Az 1024 bájtos bemeneti korlát túllépése hibát okoz, nincs csonkolás. A hash külön blokkoló munkaszálon készül, alapértelmezetten két párhuzamos feladattal. A telített feldolgozó átmeneti hibát ad, nem épít korlátlan várólistát. Ismeretlen vagy helyi jelszó nélküli fióknál is történik jelszóhash-ellenőrzés.

A TOTP hatjegyű, SHA-1 alapú, 30 másodperces időlépéssel és legfeljebb egy szomszédos lépésnyi eltéréssel. Csak helyes megerősítő kód után lesz aktív a jelszó+TOTP pár. A beállításkor használt kód is elfogy. Az adatbázis atomikusan tárolja az utolsó elfogadott lépést, így párhuzamos kérés sem használhatja fel kétszer ugyanazt a kódot. Hibás jelszó önmagában nem fogyaszt el egy helyes TOTP-kódot.

A 20 bájtos véletlen TOTP-titkot XChaCha20-Poly1305 védi, véletlen 24 bájtos nonce-szal és a technikus azonosítójához kötött hitelesített kontextussal. Az `AuthConfig` 32 bájtos titkosítási kulcsot igényel. Ezt az üzemeltetési titoktárolóból, az adatbázistól elkülönítve kell átadni; ne kerüljön forrásba, parancssori argumentumba vagy naplóba. Fájlos átadásnál kizárólag a szolgáltatásfiók és az illetékes rendszergazda olvashassa, Windows alatt megfelelő ACL-lel. A kulcs biztonságos mentése szükséges. Hiányzó vagy hibás kulcs helyett nem generálunk automatikusan újat. Kulcsrotáció még nincs: a kulcs cseréje a korábbi TOTP-titkokat olvashatatlanná teszi.

A tíz helyreállító kód egyenként 256 bit véletlent tartalmaz, hexadecimális szövegként. Csak kiadáskor adhatók át a felhasználónak; az adatbázis kizárólag SHA-256 lenyomatot tárol. A felhasználás tranzakcióban törli a kódot. A kód újraindítás után is felhasznált marad; új készlet kiadása a teljes régit érvényteleníti. A helyreállító kód önálló belépési titok, biztonságos, a géptől elkülönített helyen tárolandó.

### Próbálkozáskorlátozás

A szerver minden belépési kérés előtt közös, adatbázisban tárolt forrás-IP- és fiókkorlátot alkalmaz. Alapérték: öt perc alatt 100 kérés forrásonként és 20 fiókonként; a passkey-folyamat indítása és befejezése külön kérés. A korlátok állíthatók, a megújulás az adatbázis óráját használja, és az újraindítás nem nullázza őket. Ismeretlen felhasználónév is beleszámít. Adatbázishibánál nincs hitelesítés. A lejárt számlálók időszakosan törlődnek.

A HTTP-réteg a tényleges TCP-kapcsolat IP-jét adja át. Megbízhatóproxy-konfiguráció még nincs. Tetszőleges `X-Forwarded-For` elfogadása megkerülné a forráskorlátot. A passkey-opciók a fiókhoz tartozó hitelesítők listáját tartalmazzák; ebből következtetés vonható le a fiók passkey-beállítására. A HTTP-réteg közös hitelesítési hibát ad és tartós próbálkozáskorlátot használ; ez mérsékli, de nem szünteti meg a hitelesítőlistából származó felderítési lehetőséget.

### Ellenőrzés

`just test` ellenőrzi a hashparamétereket, sókat, titkosított TOTP-titkok sértetlenségét és fiókhoz kötését, valamint a kihívástár élettartamát. `just test-database` valódi PostgreSQL-en futtatja a hitelesítési és ügyfélszigetelési próbákat. A passkeyteszt fejlesztési függőségként használt szoftveres hitelesítővel valódi aláírást állít elő. Ez nem helyettesíti a Windows Hello, fizikai biztonsági kulcs és mobilböngésző későbbi használhatósági próbáit.

Források: [webauthn-rs](https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/), [OWASP jelszótárolás](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html), [TOTP, RFC 6238](https://www.rfc-editor.org/rfc/rfc6238.html).

## English

The internal `midas_server::auth` service implements passkeys, password+TOTP fallback and single-use recovery codes with durable PostgreSQL storage. [Browser sessions, HTTP endpoints and CSRF](munkamenetek.md#english) can now be enabled on the HTTPS server. Sign-in UI and initial setup remain separate work; the web panel is still a preview.

Operators are global identities that can later receive access to multiple tenants. Authentication tables are not tenant inventory and do not use tenant RLS to identify a signing-in operator. The restricted runtime role accesses them; future RBAC must separately authorize each tenant, device and operation. `Authenticated` carries the operator ID, credential version, verification time and method. TOTP/recovery never imply fresh passkey verification. Session and operation layers must validate these fields and the current account state.

Account creation, credential enrollment and recovery-code replacement are trusted internal provisioning operations. Guard them with setup authorization or administrative authentication/RBAC before exposing HTTP routes. There is no open registration or default account/password. Credential changes increment a version and invalidate ceremonies begun against an older version.

`webauthn-rs` validates registration and signatures with user verification required. RP ID is the configured HTTPS origin's exact host; ports, subdomains and alternate origins are not implicitly trusted. Use the final deployment origin. Challenge state stays on the server, bound to a random ID and a separate 32-byte browser context. HTTP middleware resolves this context from a protected short-lived binding cookie, never a caller-supplied JSON field. Challenges expire after five minutes by default and are consumed on every completion attempt. Restart requires starting again. The bounded in-process cache defaults to 1024 pending entries and rejects new work when full; it does not evict live ceremonies. Multiple instances need consistent routing or a future shared server-side store. Up to 32 passkeys may be registered per account. Credential IDs are globally unique; transactional counter updates reject out-of-order assertions when counters are used. Zero-counter synchronized passkeys rely on single-use challenges.

Passwords use Argon2id with independent random salts and `m=19456 KiB,t=2,p=1`. Minimum length is 12 characters; Unicode/spaces are accepted with no composition rule. Inputs over 1024 bytes fail without truncation. Bounded blocking workers default to two concurrent hashes and fail temporarily when saturated. Unknown/passwordless users still undergo dummy hash verification.

TOTP uses six digits, SHA-1, 30-second steps and adjacent-step tolerance. Enrollment requires proof of possession and consumes its confirmation code. Atomic database updates enforce one use per accepted step, including concurrent attempts; incorrect passwords cannot consume valid codes. Seeds are 20 random bytes, encrypted with XChaCha20-Poly1305, a random 24-byte nonce and operator-bound authenticated context. Supply the 32-byte encryption key through `AuthConfig` from a separate protected secret store. Never log it or put it in Git/command-line arguments. File delivery requires restrictive service-account permissions/Windows ACLs and a protected backup. No automatic replacement key or key rotation is implemented; changing the key makes existing seeds unreadable.

Ten recovery codes each contain 256 random bits, encoded as hexadecimal text. Return them only at issuance; only SHA-256 digests are stored. Transactional deletion permits one successful use, surviving restart. Replacement invalidates the entire old set. A recovery code is a standalone sign-in secret and should be stored securely away from the device.

All login paths share durable source-IP and account request limits, defaulting to 100/source and 20/account per five minutes. Passkey start and finish each count. Settings are configurable, expiry uses database time and restart does not reset limits. Unknown usernames count too; database failure denies authentication. Expired buckets are periodically deleted. HTTP integration uses the TCP peer address and ignores forwarded headers; explicit trusted-proxy configuration is not implemented. Passkey options reveal credential configuration. Uniform HTTP authentication errors and durable attempt limits mitigate, but do not eliminate, this discovery risk.

Run `just test` for password/encryption/challenge checks and `just test-database` for actual PostgreSQL authentication/isolation cases. The dev-only software authenticator produces real signatures. Windows Hello, physical security keys and mobile-browser usability still require later end-to-end validation.

References: [webauthn-rs](https://docs.rs/webauthn-rs/0.5.5/webauthn_rs/), [OWASP password storage](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html), [RFC 6238](https://www.rfc-editor.org/rfc/rfc6238.html).
