# Címtár-connector / Directory connector

[English](#english)

## Magyar

2026. november 7. · MIDAS-036, MIDAS-037 · AD-01, AD-02, AD-03, AD-04

Ügyfelenként egy kijelölt, beiratkozott agent olvassa az ügyfél Active Directoryját. A címtár nem nyílik ki az internetre: a connector az ügyfél hálózatában fut, és a meglévő, kimenő agentkapcsolaton kapja a feladatát és küldi az eredményét. A felderített gépek jelöltként jelennek meg; agenthez csak az agent saját, ellenőrzött azonosítója kapcsolja őket.

### Kijelölés

- A `directory.discover` joggal rendelkező kezelő (friss passkey-belépéssel) a `PUT /api/tenants/{ügyfél}/directory-connector` végponton jelöli ki a connectort. Megadja az eszközt, a tartományt, az `ldaps://` címet, a címtár CA-tanúsítványát és legfeljebb tíz OU-t, mint olvasási határt.
- Csak az adott ügyfél aktív, beiratkozott eszköze jelölhető ki; más ügyfél vagy visszavont eszköz nem. Titkosítatlan `ldap://` cím, a tartományon kívüli OU és üres határ nem fogadható el.
- Minden módosítás új revíziót kap, és auditált (`directory.connector_set`). A `GET` ugyanazon az útvonalon a beállítást és a legutóbbi ellenőrzés eredményét adja, a CA-tanúsítvány nélkül. Más ügyfél kezelője nem látja és nem módosítja.

### Működés

- A szerver a kijelölt eszköznek a kapcsolat elején, valamint minden módosításkor azonnal elküldi a `DirectoryAssignment` üzenetet ([agentprotokoll](agent-protokoll.md)). Más eszköz nem kap kiosztást, és a szerver csak a kijelölt eszköz aktuális revízióra vonatkozó jelentését fogadja el.
- A connector kizárólag `ldaps://` kapcsolatot nyit, és csak a kiosztásban kapott CA-ban bízik, a rendszer gyökértanúsítványaiban nem.
- Csak olvasási jogú szolgáltatásfiókkal jelentkezik be. A fiók a helyi agentkonfigurációban van, a szerverre nem kerül; üres jelszót nem használ, mert az hitelesítetlen belépést jelentene.
- Csak bejelentkezést és keresést végez. A számítógép-objektumokat lapozva (oldalanként 500) olvassa a megadott OU-kban.
- A határon kívüli bejegyzést akkor is eldobja, ha a címtár visszaadná.
- Egyszerre legfeljebb egy ellenőrzés fut; a futás közben érkező kiosztások közül csak a legutóbbi kerül sorra, így a tartományvezérlő nem kap párhuzamos kapcsolatokat.
- A határon belüli számítógép-objektumokból csak ezek az attribútumok jutnak el a szerverhez: `objectGUID`, `cn`, `dNSHostName`, a megkülönböztető név, `operatingSystem`, `operatingSystemVersion`, `lastLogonTimestamp` és a letiltott fiók jelzése (`userAccountControl`). Jelszó, csoporttagság vagy más attribútum nem.
- A gépek `DirectoryChunk` darabokban mennek (legfeljebb 200 gép és 32 KiB darabonként), utánuk a vizsgálatot lezáró `DirectoryReport`. Ugyanaz a vizsgálatazonosító (`scan`) köti össze őket. A jelentés a darabszámot és a stabil hibakódot tartalmazza. A hibakódok: `insecure`, `boundary`, `tls`, `connect`, `bind`, `search`, illetve `unconfigured`, ha az agentnek nincs helyi címtárfiókja.
- A connector kiosztáskor, újracsatlakozáskor és utána hatóránként olvassa újra a címtárat.

### Jelöltek és egyeztetés

- A szerver a darabot csak a kijelölt eszköztől és az aktuális revízióra fogadja el. A határt maga is újraellenőrzi: határon kívüli, túl hosszú vagy hibás bejegyzést eldob, akkor is, ha a connector elküldte.
- A gép kulcsa az ügyfél, a tartomány és az `objectGUID`. Ismételt vizsgálat frissíti az adatait, a gépnév változása nem hoz létre új gépet.
- Sikeres vizsgálat lezárásakor a szerver a vizsgálatban már nem szereplő gépeket megszűntként jelöli, de csak akkor, ha a vizsgálat minden gépe megérkezett. Hiányos vizsgálat nem töröl. A megszűnt gép még 30 napig látszik.
- **Felderített, agent nincs** (`candidate`): a címtárban van, de egyetlen agent sem igazolta, hogy ő ez a gép. A címtár-bejegyzés nem élő állapot és nem bizonyítja, hogy a gép megbízható. Csak regisztrált, jóváhagyott agenttel vezérelhető.
- **Kezelt** (`managed`): pontosan egy aktív eszköz Windows-agentje jelentette a leltárban, hogy ez a gép. Az agent a helyi biztonsági alrendszertől kapja a saját számítógépfiókja `objectGUID`-ját (`GetComputerObjectNameW`, `NameUniqueId`) és a DNS-tartományt. Ez a leltár `directory` fajtájában (`domain`, `object_guid`) érkezik, az agent mTLS-identitásához kötve.
- **Ütközés** (`conflict`): több aktív eszköz állítja ugyanazt az azonosítót; a szerver egyiket sem kapcsolja hozzá.
- Az egyeztetés az ügyfél adatain belül fut (sorszintű ügyfélszigetelés mellett). Más ügyfél eszköze, akár azonos GUID-dal, nem kapcsolódik össze.
- Gépnév vagy IP-cím nem identitás. Az azonos nevű, de igazolt azonosító nélküli eszköz csak tippként szerepel (`name_matches`), egyezésként nem.
- A felderítés nem telepít agentet, nem léptet tartományba és nem mozgat címtárobjektumot.

### Lekérdezés

- `GET /api/tenants/{ügyfél}/directory/computers` (ügyfélszintű `directory.discover`): a gépek adataikkal, állapotukkal (`candidate`, `managed`, `conflict`), az igazolt eszközzel és a névtippekkel. Előbb a meglévők, utána az elmúlt 30 napban megszűntek, legfeljebb 20 000 tétel. A webpanel a Felderítés és regisztráció nézetben mutatja ([beiratkozás](beiratkozas.md)).

### Ellenőrzés

- Az `agent/agent-core/tests/directory.rs` lapozó LDAPS-tesztcímtárral igazolja a következőket. A két OU 1206 gépe négy oldalon olvasható. A határon kívüli és a tartományvezérlő-OU bejegyzései kimaradnak. A címtár csak bind-, search- és unbind-műveletet kap. Idegen CA, hibás CA-adat, `ldap://`, tartományon kívüli OU, hibás és üres jelszó esetén a connector a megfelelő hibakóddal megáll.
- A `shared/protocol` tesztjei a tartomány–DN leképezést és a teljes komponensekre illeszkedő határellenőrzést vizsgálják. Az `OU=OldWorkstations` nem esik az `OU=Workstations` alá, az escape-elt DN-t a connector elutasítja.
- A connector tesztje azt is igazolja, hogy mind az 1206 gép saját `objectGUID`-dal, nevekkel, OS-sel, a bejelentkezés idejével és a letiltás jelzésével érkezik. A határon kívüli és a DC-bejegyzés kimarad, a darabok a méretkorlát alatt maradnak.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel és mTLS-sel igazolja a jogosultságot és a bérlői elszigetelést. Azt is, hogy hibás beállítás, idegen vagy visszavont eszköz nem fogadható el, a kiosztás a kijelölt eszköz élő munkamenetébe érkezik, a módosítás újraküldi, a jelentés rögzül, és az audit is megvan.
- A felderítést is igazolja:
  - a határon kívüli bejegyzést a szerver eldobja;
  - más eszköz vagy régi revízió darabja nem tárolódik;
  - az agent által jelentett GUID kezeltté teszi a gépet;
  - az azonos nevű eszköz csak tipp;
  - más ügyfél azonos GUID-ú eszköze nem kapcsolódik;
  - hiányos vizsgálat nem töröl, a teljes vizsgálat megszűntnek jelöli a hiányzó gépet;
  - más ügyfél kezelője és `directory.discover` nélküli technikus nem látja a listát.
- A Windows-agent valódi gépes tesztje szerint tartományon kívüli gép nem jelent címtár-azonosítót, tartományi gép a tartományt és a GUID-ot jelenti.

### Ismert korlátok

- Integrált (Kerberos) hitelesítés még nincs; a connector egyszerű bejelentkezést használ olvasási jogú fiókkal, titkosított kapcsolaton.
- A helyi címtárfiók védett tárolása a Windows-szolgáltatás (MIDAS-023) és a Linux-csomag (MIDAS-024) része.
- Saját azonosítót csak a Windows-agent jelent. Linuxos tartományi gép (realmd, SSSD) még csak jelöltként jelenik meg.
- A connector minden vizsgálatnál a teljes határt olvassa; változáskövetés (`uSNChanged`) még nincs.
- Tartományi gépen a valódi azonosítást a felhasználó saját próbája igazolja; a fejlesztői gép nem tartomány tagja.

## English

November 7, 2026 · MIDAS-036, MIDAS-037 · AD-01, AD-02, AD-03, AD-04

One designated, enrolled agent per customer reads that customer's Active Directory. The directory is never exposed to the internet: the connector runs inside the customer network and receives its task and returns its result over the existing outbound agent connection. Discovered computers appear as candidates; only the agent's own verified identity links them to an agent.

### Designation

- An operator holding `directory.discover` (with a fresh passkey sign-in) designates the connector at `PUT /api/tenants/{tenant}/directory-connector`. The request names the device, domain, `ldaps://` URL, directory CA certificate and up to ten OUs as the read boundary.
- Only an active, enrolled device of that customer qualifies; another customer's device or a revoked device does not. Plain `ldap://`, OUs outside the domain and an empty boundary are refused.
- Each change gets a new revision and is audited (`directory.connector_set`). `GET` on the same path returns the settings and the latest check, without the CA certificate. Operators of other customers can neither see nor change it.

### Operation

- The server sends `DirectoryAssignment` to the designated device at session start and immediately on every change ([agent protocol](agent-protokoll.md#english)). No other device receives an assignment, and the server accepts only the designated device's report for the current revision.
- The connector opens only `ldaps://` connections and trusts only the CA from the assignment, never system roots.
- It signs in with a read-only service account. The account lives in the local agent configuration and never reaches the server; an empty password is refused because it would mean an unauthenticated bind.
- It only binds and searches. It reads computer objects inside the given OUs with paging (500 per page).
- It discards any entry outside the boundary, even if the directory returns one.
- At most one check runs at a time; of the assignments arriving meanwhile only the latest is queued, so the domain controller never receives parallel connections.
- Of the computer objects inside the boundary only these attributes reach the server: `objectGUID`, `cn`, `dNSHostName`, the distinguished name, `operatingSystem`, `operatingSystemVersion`, `lastLogonTimestamp` and the disabled flag (`userAccountControl`). No password, group membership or other attribute does.
- Computers travel in `DirectoryChunk`s (at most 200 computers and 32 KiB each), followed by the `DirectoryReport` that closes the scan; the scan id (`scan`) ties them together. The report carries the count and a stable error code. Error codes are `insecure`, `boundary`, `tls`, `connect`, `bind`, `search`, and `unconfigured` when the agent has no local directory account.
- The connector reads the directory on assignment, on reconnecting and then every six hours.

### Candidates and matching

- The server accepts chunks only from the designated device for the current revision. It checks the boundary again and drops entries outside it, too long or malformed, even if the connector sent them.
- A computer is keyed by customer, domain and `objectGUID`. Repeated scans update its data; a renamed computer does not become a new one.
- When a successful scan closes, computers it no longer listed are marked removed, but only if every computer of the scan arrived; an incomplete scan removes nothing. Removed computers stay visible for 30 days.
- **Discovered, no agent** (`candidate`): the directory has it, but no agent proved to be it. A directory entry is no live status and no proof of trust; only an enrolled, approved agent can control the computer.
- **Managed** (`managed`): exactly one active device's Windows agent reported in its inventory that it is this computer. The agent learns its own computer account's `objectGUID` (`GetComputerObjectNameW`, `NameUniqueId`) and DNS domain from the local security authority. They arrive in the inventory kind `directory` (`domain`, `object_guid`), bound to the agent's mTLS identity.
- **Conflict** (`conflict`): several active devices claim the same identity; the server links none of them.
- Matching runs inside the customer's data under row-level tenant isolation. Another customer's device never links, even with the same GUID.
- A host name or IP address is no identity. A device with the same name but no proven identity is listed only as a hint (`name_matches`), never as a match.
- Discovery never installs agents, joins domains or moves directory objects.

### Query

- `GET /api/tenants/{tenant}/directory/computers` (tenant-wide `directory.discover`): computers with their data, status (`candidate`, `managed`, `conflict`), the proven device and name hints. Present computers come first, then those removed in the last 30 days, at most 20,000 items. The web panel shows them in the Discovery and enrollment view ([enrollment](beiratkozas.md#english)).

### Verification

- `agent/agent-core/tests/directory.rs` uses a paging LDAPS test directory to show the following. The 1206 computers in two OUs are read in four pages. Entries outside the boundary and in the domain controllers OU are left out. The directory receives only bind, search and unbind operations. With an untrusted CA, invalid CA data, `ldap://`, an OU outside the domain, or a wrong or empty password, the connector stops with the matching error code.
- `shared/protocol` tests cover the domain-to-DN mapping and boundary checks on whole components. `OU=OldWorkstations` is not below `OU=Workstations`, and the connector refuses escaped DNs.
- The connector test also shows that all 1206 computers arrive with their own `objectGUID`, names, OS, logon time and disabled flag, entries outside the boundary and the DC entry are left out, and chunks stay below the size limit.
- The database suite verifies with real PostgreSQL and mTLS the permission and tenant isolation. It also shows that invalid settings and foreign or revoked devices are refused, that the assignment reaches the designated device's live session and is resent on change, that the report is stored, and that the audit is written.
- It also verifies discovery:
  - the server drops an entry outside the boundary;
  - chunks from another device or an old revision are not stored;
  - a GUID reported by the agent makes the computer managed;
  - a device with the same name is only a hint;
  - another customer's device with the same GUID does not link;
  - an incomplete scan removes nothing, and a complete one marks the missing computer removed;
  - another customer's operator and a technician without `directory.discover` cannot see the list.
- The Windows agent's real-machine test shows that a computer outside a domain reports no directory identity, while a domain member reports the domain and the GUID.

### Known limits

- Integrated (Kerberos) authentication is not implemented yet; the connector uses a simple bind with a read-only account over an encrypted connection.
- Protected storage of the local directory account belongs to the Windows service (MIDAS-023) and the Linux package (MIDAS-024).
- Only the Windows agent reports its own identity. A Linux domain member (realmd, SSSD) still appears only as a candidate.
- Every scan reads the whole boundary; change tracking (`uSNChanged`) is not implemented yet.
- On a domain member the real identification is confirmed by the user's own trial; the development machine is not domain-joined.
