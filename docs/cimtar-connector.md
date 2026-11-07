# Címtár-connector / Directory connector

[English](#english)

## Magyar

2026. november 7. · MIDAS-036 · AD-01, AD-02

Ügyfelenként egy kijelölt, beiratkozott agent olvassa az ügyfél Active Directoryját. A címtár nem nyílik ki az internetre: a connector az ügyfél hálózatában fut, és a meglévő, kimenő agentkapcsolaton kapja a feladatát és küldi az eredményét. A felderített gépek jelöltként való kezelése a MIDAS-037 része.

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
- Az eredmény (`DirectoryReport`) csak darabszámot és stabil hibakódot tartalmaz, címtáradatot nem. A hibakódok: `insecure`, `boundary`, `tls`, `connect`, `bind`, `search`, illetve `unconfigured`, ha az agentnek nincs helyi címtárfiókja.

### Ellenőrzés

- Az `agent/agent-core/tests/directory.rs` lapozó LDAPS-tesztcímtárral igazolja a következőket. A két OU 1206 gépe négy oldalon olvasható. A határon kívüli és a tartományvezérlő-OU bejegyzései kimaradnak. A címtár csak bind-, search- és unbind-műveletet kap. Idegen CA, hibás CA-adat, `ldap://`, tartományon kívüli OU, hibás és üres jelszó esetén a connector a megfelelő hibakóddal megáll.
- A `shared/protocol` tesztjei a tartomány–DN leképezést és a teljes komponensekre illeszkedő határellenőrzést vizsgálják. Az `OU=OldWorkstations` nem esik az `OU=Workstations` alá, az escape-elt DN-t a connector elutasítja.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel és mTLS-sel igazolja a jogosultságot és a bérlői elszigetelést. Azt is, hogy hibás beállítás, idegen vagy visszavont eszköz nem fogadható el, a kiosztás a kijelölt eszköz élő munkamenetébe érkezik, a módosítás újraküldi, a jelentés rögzül, és az audit is megvan.

### Ismert korlátok

- Integrált (Kerberos) hitelesítés még nincs; a connector egyszerű bejelentkezést használ olvasási jogú fiókkal, titkosított kapcsolaton.
- A helyi címtárfiók védett tárolása a Windows-szolgáltatás (MIDAS-023) és a Linux-csomag (MIDAS-024) része.
- A connector most ellenőrzi az elérhetőséget és a határt, és megszámolja a gépeket. A jelöltek tárolása, az objectGUID-alapú egyeztetés és az ütemezett szinkron a MIDAS-037 része.

## English

November 7, 2026 · MIDAS-036 · AD-01, AD-02

One designated, enrolled agent per customer reads that customer's Active Directory. The directory is never exposed to the internet: the connector runs inside the customer network and receives its task and returns its result over the existing outbound agent connection. Handling discovered computers as candidates belongs to MIDAS-037.

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
- The result (`DirectoryReport`) carries only a count and a stable error code, never directory content. Error codes are `insecure`, `boundary`, `tls`, `connect`, `bind`, `search`, and `unconfigured` when the agent has no local directory account.

### Verification

- `agent/agent-core/tests/directory.rs` uses a paging LDAPS test directory to show the following. The 1206 computers in two OUs are read in four pages. Entries outside the boundary and in the domain controllers OU are left out. The directory receives only bind, search and unbind operations. With an untrusted CA, invalid CA data, `ldap://`, an OU outside the domain, or a wrong or empty password, the connector stops with the matching error code.
- `shared/protocol` tests cover the domain-to-DN mapping and boundary checks on whole components. `OU=OldWorkstations` is not below `OU=Workstations`, and the connector refuses escaped DNs.
- The database suite verifies with real PostgreSQL and mTLS the permission and tenant isolation. It also shows that invalid settings and foreign or revoked devices are refused, that the assignment reaches the designated device's live session and is resent on change, that the report is stored, and that the audit is written.

### Known limits

- Integrated (Kerberos) authentication is not implemented yet; the connector uses a simple bind with a read-only account over an encrypted connection.
- Protected storage of the local directory account belongs to the Windows service (MIDAS-023) and the Linux package (MIDAS-024).
- For now the connector checks reachability and the boundary and counts computers. Storing candidates, objectGUID matching and scheduled synchronization belong to MIDAS-037.
