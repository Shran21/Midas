# Adatbázis / Database

[English](#english)

## Magyar

A szerver PostgreSQL 18-at és SQLx-migrációkat használ. Az első séma az ügyfelet, telephelyet, eszközcsoportot, eszközt és csoporttagságot tárolja. Az eszköz munkaállomás vagy szerver lehet, Windows vagy Linux platformmal. Ezek adatbázisrekordok; önmagukban nem jelentenek telepített vagy beiratkozott agentet.

### Jogosultságok és előkészítés

Hozz létre külön, üres `midas` adatbázist. Annak rendszergazdájaként futtasd a `server/database/bootstrap.sql` fájlt. Ez két bejelentkezés nélküli szerepet és egy külön sémát hoz létre, valamint elveszi a `public` séma általános létrehozási jogát. A parancsot csak az erre a célra létrehozott adatbázison használd; nem módosít meglévő alkalmazássémát, és ismételt szereplétrehozásnál leáll.

- `midas_owner`: a séma, táblák és migrációk tulajdonosa.
- `midas_runtime`: a futó szerver szerepe; csak a szükséges táblák olvasását és írását kapja meg. Nem törölhet táblát, nem kapcsolhatja ki az RLS-t, nem végezhet `TRUNCATE` műveletet, és nem olvashatja a migrációs nyilvántartást.

Külön bejelentkezési fiók tartozzon mindkét szerephez. A migrációs fióknak csak a `midas_owner`, a szerverfióknak csak a `midas_runtime` szerepet add. Egyik se legyen superuser, adatbázis-tulajdonos, `BYPASSRLS`, szerep- vagy adatbázis-létrehozó, illetve replikációs fiók. Jelszót védett titoktárolóból vagy a `psql` interaktív `\password` parancsával adj meg; ne tedd parancssori argumentumba, Gitbe vagy naplóba.

### Migráció és indítás

A migrációs fiók kapcsolatát a `MIDAS_MIGRATION_DATABASE_URL` környezeti változóban add át, majd:

```text
cargo run --locked -p midas-server --bin midas-db
```

A migrációk a binárisba kerülnek, sorszámuk és ellenőrzőösszegük nyilvántartott. Az újrafuttatás a már alkalmazott változást nem ismétli meg. A futó szerver nem futtat migrációt, és nem igényli a migrációs fiók titkát.

A szerver a `MIDAS_DATABASE_URL` változóból kapja a korlátozott fiók kapcsolatát. Enélkül az előnézet és a meglévő állapotvégpont továbbra is működik. Beállított, de hibás vagy túl jogosult adatbáziskapcsolattal az indítás leáll. A `MIDAS_DATABASE_POOL_SIZE` pozitív egész számmal állítható, alapértelmezése 8; a pool nem tart fenn kötelezően ennyi üres kapcsolatot. A kiszolgáló induláskor és minden új adatbáziskapcsolatnál ellenőrzi a szerepet és az RLS alapfeltételeit.

A kapcsolat alapértelmezésben `verify-full` TLS-t követel, a szervertanúsítvány és a hosztnév ellenőrzésével. Szükség esetén a kapcsolat `sslrootcert` beállításával add meg a CA-t. A URL-ben megadott gyengébb `sslmode` nem kapcsolja ki az ellenőrzést. Helyi fejlesztéshez a `MIDAS_DATABASE_LOCAL_PLAINTEXT=1` kizárólag számmal megadott loopback IP-címen enged titkosítás nélküli kapcsolatot. Távoli címhez ez az opció hibát okoz.

### Ügyfélszigetelés

Minden ügyfélhez kötött táblán `ENABLE` és `FORCE ROW LEVEL SECURITY` működik. A hiányzó ügyfélkontextus nem ad látható sort és nem enged írást. A telephely-, eszköz- és csoportkapcsolatok összetett idegen kulcsai az ügyfélazonosítót is tartalmazzák, így nem hozható létre másik ügyfél rekordjára mutató kapcsolat.

A szerver `Database::begin` hívása tranzakción belül, lokális beállítással adja meg az ügyfelet. Commit, rollback és eldobott tranzakció után ez megszűnik; a pool kiadás előtt is törli az előző ügyfélkontextust. A lekérdezések paraméterezettek, és az alkalmazás szűrése mellett az adatbázis is ellenőriz.

Az ügyfélazonosítót a hitelesített munkamenet és a [jogosultságellenőrzés](jogosultsagok.md) alapján lehet felhasználni, nem egy megbízhatónak tekintett HTTP-fejléc. Az RLS a hibás alkalmazásszűrő elleni további védelem; nem helyettesíti a hitelesítést és az RBAC-ot, és nem véd egy adatbázis-rendszergazdától vagy a futtatófiókkal megszerzett tetszőleges SQL-végrehajtástól. Jelenleg nincs nyilvános ügyfélkezelő vagy eszközkezelő API. Az alkalmazás nem naplóz SQL-szöveget vagy kötött értékeket.

### Ellenőrzés

```text
just test-database
```

Windows alatt a `python .github/scripts/install-postgres.py` ellenőrzőösszeghez kötött, helyi PostgreSQL 18.6 binárist tölt le; nem telepít rendszerszolgáltatást. A teszt új, véletlen jelszóval védett, loopbackre kötött klasztert indít, majd leállítja és eltávolítja a saját ideiglenes adatait. Meglévő, kizárólag erre szánt tesztklaszterhez a `MIDAS_TEST_DATABASE_ADMIN_URL` adható meg. Az adatbázis neve `midas_test_` előtaggal kezdődjön, és a MIDAS-szerepek még ne létezzenek. Éles adatbázison tilos futtatni, mert szerepeket és sémát hoz létre.

A CI mindkét platformon futtatja a próbát: Windowson a rögzített binárissal, Linuxon digesthez kötött PostgreSQL-konténerrel. A normál Rust-teszt külön jelzi az adatbázist igénylő próbát; a `test-database` nem tekinti sikernek a hiányzó környezetet. A próba ugyanazon backendkapcsolaton ellenőrzi a két ügyfél szétválasztását, a hiányzó és hibás kontextust, az idegen ügyfél olvasását/írását, az összetett kulcsokat, a tranzakció végét és a túl jogosult futtatófiók elutasítását.

## English

The server uses PostgreSQL 18 and embedded SQLx migrations. The initial schema stores tenants, sites, groups, devices and group membership. Workstation/server and Windows/Linux classifications describe inventory records; they do not imply installed or enrolled agents.

Create a dedicated empty `midas` database and run `server/database/bootstrap.sql` as its administrator. It creates the no-login `midas_owner` and `midas_runtime` roles, a dedicated schema and revokes general CREATE access to `public`. Use a separate login for each role. Runtime credentials must not own the database or schema, be a superuser, bypass RLS, create roles/databases, replicate or hold the owner role, including non-inherited membership. Set passwords through a secret store or interactive `psql \password`, never command-line arguments or Git.

Provide migration credentials through `MIDAS_MIGRATION_DATABASE_URL` and run `cargo run --locked -p midas-server --bin midas-db`. Migration versions/checksums are recorded, and rerunning does not repeat applied changes. The running service does not migrate or need migration credentials.

Set `MIDAS_DATABASE_URL` for the runtime login. Without it, the existing preview and liveness endpoint continue to work. An invalid or overprivileged configured database fails startup. `MIDAS_DATABASE_POOL_SIZE` is a positive integer, default 8, with no mandatory idle connections. Startup and new connections validate role attributes and RLS prerequisites.

Connections require verified TLS including hostname validation; provide a CA through `sslrootcert` as needed. Weaker URL `sslmode` values cannot disable this. Explicit `MIDAS_DATABASE_LOCAL_PLAINTEXT=1` is accepted only for numeric loopback IPs, never remote addresses.

All tenant tables enable and force RLS. Missing context exposes no rows and permits no writes. Composite foreign keys include tenant IDs. `Database::begin` uses transaction-local context, cleared by commit, rollback or dropped transactions; the pool also clears context before reuse. Parameterized application filters complement database enforcement.

Authenticated sessions and [authorization](jogosultsagok.md#english) must validate tenant access before exposing this internal layer. Untrusted HTTP headers cannot supply authority. RLS protects against faulty filters, not database administrators or arbitrary SQL execution obtained with runtime credentials. Tenant/device HTTP APIs are not implemented. Application statement and parameter logging is disabled.

Run `just test-database`. On Windows, `python .github/scripts/install-postgres.py` downloads checksum-pinned PostgreSQL 18.6 binaries without installing a system service. The test harness starts a fresh loopback cluster with a random password, then stops it and removes its own temporary data. Alternatively set `MIDAS_TEST_DATABASE_ADMIN_URL` to a dedicated disposable PostgreSQL 18 cluster: the database name must start with `midas_test_`, and MIDAS roles must not exist yet. Never run this against production; it creates roles and schema.

CI runs database tests on Windows and Linux, using the pinned Windows package and digest-pinned Linux container. Regular Rust tests label the database case as requiring a separate run; `test-database` fails on missing setup. Tests reuse the same backend connection and verify tenant isolation, missing/malformed context, cross-tenant reads/writes and references, transaction cleanup and rejection of unsafe runtime roles.
