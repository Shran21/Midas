# Eszközök a webpanelen / Devices in the web panel

[English](#english)

## Magyar

2026. november 8. · MIDAS-031 · UI-01, UI-02, UI-03, UI-05, UI-07, SERVER-04

### Eszközlista

- Az **Eszközök** nézet a kiválasztott ügyfél aktív és visszavont eszközeit mutatja, amelyekre a technikusnak `device.read` joga van. A függő és az elutasított beiratkozás a beiratkozási nézetben marad.
- Fent négy összesítés látható: a kezelt eszközök, az elérhető eszközök, a riasztással rendelkezők és az újraindításra várók. Az utóbbi kettő kattintásra szűr.
- Lista és kártya nézet van. A választást a böngésző megjegyzi.
- Keresni név, OS, telephely és csoport szerint lehet. Szűrni típusra, állapotra (elérhető, figyelmet igényel, nem elérhető, visszavont), operációs rendszerre, telephelyre és csoportra lehet.
- Az állapotnak mindig van szöveges neve is. „Figyelmet igényel” az online eszköz, ha nyitott riasztása, figyelmeztető vagy kritikus ellenőrzése van.
- Terhelési adat csak online eszköznél jelenik meg; offline gépnél a régi mérés nem látszik aktuálisnak.

### Helyi menü és műveletek

- A műveletmenü a listasoron és a kártyán háromféleképpen nyílik: jobbklikkel, Shift+F10-zel vagy a menübillentyűvel, illetve a ⋯ menügombbal. Mobilon és billentyűzettel a menügomb a megbízható út.
- A menü fejléce megnevezi a célgépet, az ügyfelet és a telephelyet.
- A menü nyilakkal, Home/End gombbal, Enterrel és Escape-pel kezelhető. Bezáráskor a fókusz a menügombra tér vissza.
- A menü, a kártya és az eszközoldal ugyanabból a műveletlistából dolgozik (`deviceActions`). A lista a szerver által eszközönként visszaadott, érvényes jogosultságokból épül (`permissions`).
- Ezeket a jogokat a szerver ugyanazzal a szabálykiértékeléssel számolja, mint amivel az eszközre szóló kéréseket engedélyezi. Az ügyfélszintű műveletnél (visszavonás: `device.enroll`) az ügyfélszintű jog dönt (`tenant_wide`). A szerver ettől függetlenül minden kérést maga is ellenőriz.
- Csak a technikus számára engedélyezett művelet jelenik meg:

  | Művelet | Jog | Agentet igényel |
  | --- | --- | --- |
  | Eszközoldal, leltár, szabályzat | `device.read` | nem |
  | Monitorozás és riasztások | `monitor.read` | nem |
  | Állapot frissítése most | `monitor.read` | igen |
  | Eszköz visszavonása | ügyfélszintű `device.enroll`, friss passkey, megerősítés | nem |
  | Távsegítség, rendszergazdai távvezérlés, terminál, fájlkezelő | a hozzájuk tartozó jog | igen, a 2. fázisban |
  | Szkriptek, frissítések kezelése | a hozzájuk tartozó jog | nem, a 4. fázisban |

- A még el nem készült műveletek „Későbbi verzióban” jelzéssel, letiltva látszanak, hogy a technikus lássa, mire lesz joga.
- Az agentet igénylő művelet offline gépen letiltott, a magyarázat: „Csak online gépen indítható”. Visszavont gépen: „Visszavont eszközön nem indítható”. A friss passkeyt kérő műveletnél ez külön jelzés.
- A visszavonás megerősítő ablaka megnevezi az ügyfelet, a gépet és a telephelyet. Bezáráskor a fókusz visszatér az indítóhoz.

### Eszközoldal

- A célazonosító sáv görgetéskor is látható. Mutatja az ügyfelet, a gép nevét, az OS-t, a típust, a telephelyet és az állapotot. A morzsamenü és a lap címe is a kiválasztott gépet nevezi meg.
- **Offline gépnél** egy magyarázat mondja el, mi tekinthető meg: a legutóbbi állapot, a leltár, a riasztások és a szabályzat, a legutóbbi kapcsolat idején érvényes adatokkal. Azt is leírja, hogy az agentet igénylő műveletek csak online gépen indulnak. Visszavont gépnél külön figyelmeztetés látszik.
- A **gyakori műveletek** a fülek felett vannak; a részletek a fülekben nyílnak. Fülek:
  - **Áttekintés:** adatok, ellenőrzések és aktuális terhelés.
  - **Monitorozás és riasztások:** minden ellenőrzés a hatással érintett elemekkel és a változás idejével; az elmúlt óra CPU-, memória- és lemezgrafikonja; az eszköz riasztásai nyugtázással, ha van `alert.acknowledge` jog.
  - **Hardver és szoftverleltár:** hardver, OS, kötetek, helyi rendszergazdák és kereshető szoftverlista.
  - **Szabályzat és csoportok:** az érvényes beállítások értékkel, alapértékkel és forrással, valamint a csoporttagság. Ügyfélszintű `access.manage` joggal a csoporttagság módosítható.
- Csak azok a fülek látszanak, amelyekhez a technikusnak joga van. A fülek nyilakkal is válthatók.

### Élő állapot

- A böngésző munkamenet-csatornája (`/api/session/socket`) a kiválasztott ügyfelet figyeli: `{"type": "watch", "tenant": "…"}`. A szerver tagságot ellenőriz, és `watching` vagy `watch_refused` választ küld.
- A szerver változáskor részletek nélküli `{"type": "devices_changed", "tenant": "…"}` értesítést küld, fél másodpercen belül összevonva. Változás:
  - kapcsolódás és kiesés;
  - állapotjelentés és leltár;
  - riasztás nyílása, megszűnése és nyugtázása;
  - beiratkozási döntés, visszavonás és csoporttagság.
- A webpanel ilyenkor újra lekéri azt, amit a technikus láthat. Ezen felül a látható oldal percenként, az eszközoldal 30 másodpercenként frissít, mert a mérések percenként érkeznek.
- A tagságot a szerver 15 másodpercenként újraellenőrzi; a tagság megszűnése után nem küld értesítést.
- Az **Állapot frissítése most** (`POST …/devices/{eszköz}/refresh`, `monitor.read`) a kapcsolódó agentnek `RefreshRequest` üzenetet küld. Az agent ekkor azonnal lefuttatja az ellenőrzéseket, akkor is elküldi az eredményt, ha az nem változott, és összegyűjti a leltárváltozást.
  - Offline gépnél a válasz 409 `device_offline`.
  - Egy eszközre 30 másodpercen belül ismételt kérés sikeres, de nem ébreszti fel újra az agentet.
  - A kérés auditált (`device.refresh`).

### Kereső és parancspaletta

- A fejléc **Keresés** gombja vagy a Ctrl+K (macOS-en Cmd+K) nyitja meg. A paletta a kiválasztott ügyfelet is megnevezi.
- Nézetek, eszközök és eszközműveletek között keres; a lekérdezés minden szavának szerepelnie kell (például „WS-014 frissít”).
- Csak a technikus számára látható nézetek, a szerver által jogosultság szerint szűrt eszközlista, valamint a `deviceActions` szerint engedélyezett és éppen indítható műveletek jelennek meg. Offline gépen tiltott, későbbi verzióra jelölt vagy ügyfélszintű jog nélküli művelet nem kereshető. A futtatást a szerver itt is önállóan ellenőrzi.
- A nyilak választanak, az Enter megnyit vagy futtat, az Escape bezár, és a fókusz visszatér oda, ahonnan a paletta megnyílt. A lista kombinált listamezőként (combobox) jelenik meg, a képernyőolvasó a kijelölt találatot is felolvassa.
- A visszavonáshoz ugyanaz a megerősítő ablak jelenik meg, mint a menüből.

### API

| Végpont | Jog |
| --- | --- |
| `GET /api/tenants/{ügyfél}/devices` | eszközönként `device.read`; a válasz eszközönként tartalmazza az érvényes jogokat |
| `GET /api/tenants/{ügyfél}/devices/{eszköz}` | `device.read` az eszközre |
| `GET …/devices/{eszköz}/metrics` | `monitor.read` az eszközre; az elmúlt óra percmérései és az utolsó 24 óra órás összesítése |
| `GET …/devices/{eszköz}/alerts` | `monitor.read` az eszközre; a legutóbbi 50 riasztás, a nyitottak elöl |
| `POST …/devices/{eszköz}/refresh` | `monitor.read` az eszközre, online agent |
| `GET /api/tenants/{ügyfél}` | tag; az ügyfélszintű jogok (`tenant_wide`) és a friss passkeyt igénylő jogok (`fresh_passkey`) is |

A **Riasztások** nézet ügyfélszintű `monitor.read` joggal a nyitott riasztásokat és az előzményeket mutatja. Szűkebb hatókörű technikusnál a riasztással rendelkező saját eszközeit sorolja fel, és az eszközoldalra vezet.

### Ellenőrzés

- A webpanel tesztje (`pnpm test:web`) a következőket vizsgálja:
  - csak engedélyezett művelet jelenik meg;
  - az ügyfélszintű visszavonást nem kínálja eszközre szűkített jog;
  - offline gépen a tárolt adat elérhető marad, az agentet igénylő művelet tiltott;
  - visszavont gépen nincs újabb visszavonás;
  - működnek az állapotok és a szűrők.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel és mTLS-es agenttel igazolja a következőket:
  - a lista a jogokkal és a csoportokkal együtt érkezik;
  - a Windows nélküli UAC- és a nem engedélyezett felügyelet nélküli jog hiányzik;
  - más ügyfél kezelője nem éri el;
  - a telephelyre és csoportra jogosult technikus pontosan a két eszközt látja;
  - eszközre szóló tiltásnál a lista jogai mind a négy végpont válaszával egyeznek;
  - az állapotjelentés változásértesítést küld;
  - a frissítéskérés az agentnél azonnal új jelentést indít;
  - a megtagadott monitorozási jog a frissítést is tiltja;
  - offline gépnél 409 a válasz;
  - az audit egy frissítést rögzít.
- Helyi próba: `just dev-auth --fleet 12` a demóügyfelekhez 12 szimulált agentet regisztrál előre jóváhagyott tokennel. Ezek valódi mTLS-kapcsolattal küldenek mérést, leltárt és ellenőrzést. A szimulált gépek tanúsítványa a `.local/dev-auth/fleet` mappában marad, újraindításkor ugyanazok a gépek jelentkeznek.

### Ismert korlátok

- A távsegítség, a terminál, a fájlkezelő, a szkriptek és a frissítések a menüben letiltva, „Későbbi verzióban” jelzéssel látszanak. A távsegítség a 2. fázisban, a többi a 4. fázisban készül el.
- A lista legfeljebb 5000 eszközt ad vissza, lapozás nélkül. Nagy flottán a terhelést a MIDAS-035 méri.
- A függő újraindítás az `updates` ellenőrzés `reboot_pending` okából látszik. Ha ugyanott súlyosabb frissítési hiba is van, az kerül előre, és az újraindítás jelzése elmarad.

## English

November 8, 2026 · MIDAS-031 · UI-01, UI-02, UI-03, UI-05, UI-07, SERVER-04

### Device list

- **Devices** lists the selected customer's active and revoked devices the operator holds `device.read` for. Pending and rejected enrollments stay in the enrollment view.
- The top shows managed devices, available devices, devices with alerts and devices waiting for a restart; the last two filter on click.
- There are list and card layouts; the browser remembers the choice.
- Search covers name, OS, site and group. Filters cover kind, condition (online, needs attention, offline, revoked), operating system, site and group.
- Every condition also has a text label. An online device needs attention when it has an open alert or a warning or critical check.
- Load figures show only for online devices, so an offline device's last sample never looks current.

### Context menu and actions

- The action menu opens on a row or card in three ways: right-click, Shift+F10 or the menu key, or the ⋯ menu button. The menu button is the reliable path on mobile and with a keyboard.
- The menu header names the target device, customer and site.
- Arrow keys, Home/End, Enter and Escape work, and focus returns to the menu button when it closes.
- The menu, the cards and the device page use one action list (`deviceActions`), built from the effective permissions the server returns for each device (`permissions`).
- The server computes those permissions with the same rule evaluation it uses to authorize requests for that device. A tenant-wide operation (revocation: `device.enroll`) follows the tenant-wide permission (`tenant_wide`). The server still checks every request on its own.
- Only actions the operator is allowed to use appear:

  | Action | Permission | Needs the agent |
  | --- | --- | --- |
  | Device page, inventory, policy | `device.read` | no |
  | Monitoring and alerts | `monitor.read` | no |
  | Refresh status now | `monitor.read` | yes |
  | Revoke device | tenant-wide `device.enroll`, fresh passkey, confirmation | no |
  | Remote support, administrative remote control, terminal, file manager | their own permission | yes, in phase 2 |
  | Scripts, update management | their own permission | no, in phase 4 |

- Actions not built yet show disabled, marked "In a later version", so operators see what they will be allowed to do.
- An action that needs the agent is disabled on an offline device ("Only on an online device") and on a revoked one ("Not available on a revoked device"). Actions that ask for a fresh passkey say so.
- The revocation confirmation names the customer, device and site; when it closes, focus returns to the control that opened it.

### Device page

- The target bar stays visible while scrolling. It shows the customer, device name, OS, kind, site and condition, and the breadcrumbs and page title also name the selected device.
- **On an offline device** a note explains what can be viewed: the latest status, inventory, alerts and policy, as of the last connection. It also says that actions needing the agent start only on an online device. A revoked device has its own warning.
- **Common actions** sit above the tabs; details open in the tabs:
  - **Overview:** details, checks and current load.
  - **Monitoring and alerts:** every check with its affected items and change time; last-hour CPU, memory and disk charts; the device's alerts, which can be acknowledged with `alert.acknowledge`.
  - **Hardware and software inventory:** hardware, OS, volumes, local administrators and a searchable software list.
  - **Policy and groups:** effective settings with value, default and source, and group membership. Membership can be changed with tenant-wide `access.manage`.
- Only tabs the operator may open are shown. Arrow keys switch tabs.

### Live state

- The browser session channel (`/api/session/socket`) watches the selected customer: `{"type": "watch", "tenant": "…"}`. The server checks membership and answers `watching` or `watch_refused`.
- On a change the server sends `{"type": "devices_changed", "tenant": "…"}` without details, coalesced within half a second. Changes are:
  - connecting and dropping out;
  - health reports and inventory;
  - alerts opening, resolving and being acknowledged;
  - enrollment decisions, revocation and group membership.
- The panel then refetches what the operator may see. The visible page also refreshes every minute, and the device page every 30 seconds, because samples arrive each minute.
- The server rechecks membership every 15 seconds and sends no more notices after it ends.
- **Refresh status now** (`POST …/devices/{device}/refresh`, `monitor.read`) sends the connected agent a `RefreshRequest`. The agent then runs its checks at once, sends the result even if unchanged, and collects its inventory changes.
  - An offline device answers 409 `device_offline`.
  - A repeat within 30 seconds for the same device succeeds without waking the agent again.
  - Requests are audited (`device.refresh`).

### Search and command palette

- The **Search** button in the header or Ctrl+K (Cmd+K on macOS) opens it. The palette names the selected customer.
- It searches views, devices and device actions; every word of the query must match (for example "WS-014 refresh").
- Only views the operator can open, the server's permission-filtered device list and actions that `deviceActions` allows and that can run now appear. Actions blocked offline, planned for later or lacking a tenant-wide permission cannot be found. The server still checks every run on its own.
- Arrow keys select, Enter opens or runs, Escape closes and focus returns to where the palette was opened. The list is a combobox with listbox semantics, so screen readers announce the selected result.
- Revocation shows the same confirmation as the menu.

### API

| Endpoint | Permission |
| --- | --- |
| `GET /api/tenants/{tenant}/devices` | `device.read` per device; each item carries the effective permissions |
| `GET /api/tenants/{tenant}/devices/{device}` | `device.read` for the device |
| `GET …/devices/{device}/metrics` | `monitor.read` for the device; last hour of minute samples and the last 24 hourly rollups |
| `GET …/devices/{device}/alerts` | `monitor.read` for the device; the latest 50 alerts, open ones first |
| `POST …/devices/{device}/refresh` | `monitor.read` for the device, connected agent |
| `GET /api/tenants/{tenant}` | member; also the tenant-wide permissions (`tenant_wide`) and those needing a fresh passkey (`fresh_passkey`) |

With tenant-wide `monitor.read`, **Alerts** shows open alerts and history. For an operator with a narrower scope it lists their devices with alerts and leads to each device page.

### Verification

- The web panel test (`pnpm test:web`) checks the following:
  - only permitted actions appear;
  - a device-scoped grant does not offer the tenant-wide revocation;
  - an offline device keeps stored data and blocks actions needing the agent;
  - a revoked device offers no second revocation;
  - conditions and filters work.
- The database suite, with real PostgreSQL and an mTLS agent, verifies the following:
  - the list arrives with permissions and groups;
  - UAC without Windows and unattended access without opt-in are absent;
  - another customer's operator gets nothing;
  - a technician scoped to a site and a group sees exactly those two devices;
  - with a device-scoped denial, the listed permissions match all four endpoints' answers;
  - a health report sends a change notice;
  - a refresh makes the agent report at once;
  - the denied monitoring permission also blocks a refresh;
  - an offline device answers 409;
  - the audit records one refresh.
- Local trial: `just dev-auth --fleet 12` enrolls 12 simulated agents into the demo customers with preapproved tokens. They send metrics, inventory and checks over real mTLS connections. Their certificates stay in `.local/dev-auth/fleet`, so the same devices return after a restart.

### Known limits

- Remote support, terminal, file manager, scripts and updates appear disabled in the menu, marked "In a later version". Remote support arrives in phase 2, the rest in phase 4.
- The list returns at most 5000 devices without paging; MIDAS-035 measures load on a large fleet.
- A pending restart comes from the `updates` check's `reboot_pending` reason. If a more severe update problem is present, it takes precedence and the restart flag is not shown.
