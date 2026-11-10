# Hardver- és szoftverleltár / Hardware and software inventory

[English](#english)

## Magyar

2026. november 8. · MIDAS-026 · PERF-02

### Tartalom

A leltár (fajta, kulcs, érték) elemekből áll, értékenként legfeljebb 256 karakter; a NUL karaktert az agent elhagyja, a szerver az érvénytelen elemet kihagyja:

| Fajta | Tartalom |
| --- | --- |
| `hardware` | gépnév, processzor, logikai processzorok száma, memória |
| `os` | operációs rendszer neve, verziója, hosszú neve, kernel; Windowson a telepítés típusa (`installation`: `Client`, `Server`, `Server Core`), lásd [gépprofil](gepprofil.md) |
| `disk` | a nem cserélhető kötetek mérete és fájlrendszere |
| `software` | Windowson a registry Uninstall kulcsai (64 és 32 bites nézet, rendszerkomponensek nélkül), név → „verzió \| kiadó”; Linuxon a dpkg- vagy rpm-csomagok, név → „verzió architektúra” |
| `admin` | Windowson a beépített Administrators csoport tagjai (a csoportot a jól ismert SID azonosítja, a lokalizált név miatt); Linuxon a root és a sudo, wheel, admin csoport tagjai |
| `directory` | Windowson tartományi gépnél a DNS-tartomány (`domain`) és a saját számítógépfiók `objectGUID`-ja (`object_guid`); ez alapján egyezteti a szerver a felderített gépeket ([címtár-connector](cimtar-connector.md)) |

### Ütemezés és változások

- Az első gyűjtés véletlen, legfeljebb egyperces késleltetéssel indul, hogy a gépek ne egyszerre gyűjtsenek; utána 15 percenként fut.
- Ha egy frissítési kérés vagy az ütemezett gyűjtés folyamatban lévő feltöltésre esik, az agent megjegyzi az igényt. Az utolsó nyugta után egyszer újra gyűjt; több közben érkező kérés összevonódik, és nem indít párhuzamos gyűjtést.
- **Teljes egyeztetés:** az első alkalommal, majd 24 óránként. Új generációt kap, és darabokban (legfeljebb 200 elem és 32 KiB, így a hosszú, többbájtos nevek sem lépik túl a 64 KiB-os üzenethatárt) megy; a következő darab a nyugta után indul. Az utolsó darab után a szerver törli az adott eszköz minden más generációjú elemét.
- **Változáscsomag:** a két teljes egyeztetés között csak az új, módosult és eltűnt elemek mennek. A szerver csak akkor alkalmazza, ha a generáció egyezik a tárolttal; különben elutasítja, és az agent azonnal teljes egyeztetést indít.
- Az agent az utolsó sikeresen nyugtázott leltárt és a teljes egyeztetés idejét a védett állapotmappájában (`inventory.pb`) tárolja. Újraindulás után innen folytatja: változást küld, és a napi teljes egyeztetést az eredeti időponthoz igazítja. Megszakadt küldés nem számít sikeres egyeztetésnek; a következő kapcsolat a nyugtázatlan darabtól folytatja.

### Megjelenítés

- `GET /api/tenants/{ügyfél}/devices/{eszköz}/inventory` (`device.read`): elemek a legutóbbi változás idejével, valamint az utolsó teljes egyeztetés időpontja. A webpanel az eszközoldal leltár fülén mutatja ([eszközoldal](eszkozoldal.md)).

### Ellenőrzés

- Az agent-core egységtesztjei a különbségszámítást, a teljes és változásalapú tervezést, a darabolást és az üzenetméretet (hosszú, többbájtos szövegekkel is), a mentést csak az utolsó nyugta után, az újraindulás utáni visszatöltést, az elutasítás miatti teljes egyeztetést, a 63 bites generációt és a valódi gépen futó közös leltárt vizsgálják.
- A Windows-gyűjtő tesztje a valódi gépen telepített szoftvert, helyi rendszergazdát és OS-nevet talál. A Linux-elemzők tesztjei a dpkg-, az rpm- és a group-formátumot ellenőrzik.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel és mTLS-sel igazolja a következőket:
  - 450 szoftveres leltár darabokban megérkezik;
  - törlés, új elem és verzióváltás változásként frissül, teljes egyeztetés nélkül;
  - agent-újraindulás után csak változás megy, a generáció és az egyeztetés ideje változatlan;
  - generációeltérésnél az elutasított változás teljes egyeztetést vált ki, ez az elavult elemet törli;
  - az API az ügyfél kezelőjének válaszol, más ügyfélnek nem.

### Ismert korlátok

- A gyártó, a modell és a sorozatszám még nincs a leltárban.
- Rocky Linuxon az `rpm` lekérdezést a megerősített szolgáltatás alatt is ki kell próbálni ([Linux-agent próba](meresek/linux-agent.md)).

## English

November 8, 2026 · MIDAS-026 · PERF-02

### Content

The inventory consists of (kind, key, value) items, each value at most 256 characters; the agent drops NUL characters and the server skips invalid items:

| Kind | Content |
| --- | --- |
| `hardware` | host name, processor, logical processor count, memory |
| `os` | operating system name, version, long name, kernel; on Windows the installation type (`installation`: `Client`, `Server`, `Server Core`), see [host profile](gepprofil.md#english) |
| `disk` | size and file system of non-removable volumes |
| `software` | on Windows the registry Uninstall keys (64- and 32-bit views, without system components), name → "version \| publisher"; on Linux dpkg or rpm packages, name → "version architecture" |
| `admin` | on Windows the members of the built-in Administrators group (identified by its well-known SID because the name is localized); on Linux root plus members of sudo, wheel and admin |
| `directory` | on a Windows domain member the DNS domain (`domain`) and its own computer account's `objectGUID` (`object_guid`); the server matches discovered computers by them ([directory connector](cimtar-connector.md#english)) |

### Schedule and changes

- The first collection starts after a random delay of up to one minute so machines do not collect together, then runs every 15 minutes.
- If a refresh request or scheduled collection arrives during an upload, the agent remembers it and collects once after the final acknowledgement. Requests received during the upload coalesce without starting concurrent collections.
- **Full reconciliation:** the first time, then every 24 hours. It gets a new generation and is sent in chunks (at most 200 items and 32 KiB, so long multi-byte names stay below the 64 KiB message limit); the next chunk follows the acknowledgement. After the last chunk the server deletes every item of that device from other generations.
- **Change set:** between full reconciliations only new, changed and removed items are sent. The server applies it only if its generation matches the stored one; otherwise it refuses, and the agent starts a full reconciliation at once.
- The agent keeps the last fully acknowledged inventory and the time of its full reconciliation in its protected state folder (`inventory.pb`). After a restart it continues from there: it sends changes and keeps the daily full reconciliation on its original schedule. An interrupted upload is not a successful reconciliation; the next connection resumes from the unacknowledged chunk.

### Display

- `GET /api/tenants/{tenant}/devices/{device}/inventory` (`device.read`): items with their last change time and the time of the last full reconciliation. The web panel shows it on the device page inventory tab ([device pages](eszkozoldal.md#english)).

### Verification

- agent-core unit tests cover diffing, full versus change planning, chunking and message size (also with long multi-byte texts), saving only after the last acknowledgement, reload after restart, a full run after a refusal, 63-bit generations and the common inventory on the real machine.
- The Windows collector test finds installed software, a local administrator and the OS name on the real machine. Linux parser tests check the dpkg, rpm and group formats.
- The database suite, with real PostgreSQL and mTLS, verifies the following:
  - a 450-package inventory arrives in chunks;
  - a removal, an addition and a version change update as changes, without a full reconciliation;
  - after an agent restart only changes are sent, and the generation and reconciliation time stay the same;
  - on a generation mismatch the refused change set triggers a full reconciliation that removes the stale item;
  - the API answers the customer's operator and nobody from another customer.

### Known limits

- Manufacturer, model and serial number are not collected yet.
- On Rocky Linux the `rpm` query must be tried under the hardened service ([Linux agent trial](meresek/linux-agent.md#english)).
