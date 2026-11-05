# MIDAS

[English](#english)

A MIDAS saját szerveren üzemeltethető, nyílt forráskódú távfelügyeleti platform terve. Egy technikus több ügyfél munkaállomásait és szervereit kezelheti ugyanarról a webes adminfelületről.

## Állapot

Az alapok megvalósítása elindult: Rust-szerver helyi állapotvégponttal, magyar–angol üzemeltetői előnézet világos és sötét témával és közös buildparancsok. A Windowsos távsegítés és az UAC kezelése a későbbi prototípus része. A 0–5. fázis felülvizsgált feladatai a [GitHub Projectben](https://github.com/users/Shran21/projects/4) követhetők: 97 ütemezett tétel és 6 külön javaslat.

- A [rendszerterv](docs/rendszerterv.md) tartalmazza az architektúrát, a platformokat, a biztonsági modellt és az ütemtervet.
- A [működési követelmények](docs/kovetelmenyek.md) írják le az agent és az adminpanel használatát, a jogosultságokat és az elfogadási próbákat.
- A [feladatfelosztás](docs/feladatlista.md) minden tételhez státuszt, mérföldkövet, függőséget és elfogadási feltételt rendel. A közreműködéshez lásd a [hozzájárulási útmutatót](CONTRIBUTING.md).

Fejlesztői indítás: `just setup`, majd `just dev`. A szerveralap támogatja a TLS 1.3-at, az elkülönített mTLS-agentfigyelőt, az ACME tanúsítványkezelést és a PostgreSQL ügyfélszigetelést. [Környezet és parancsok](docs/fejlesztes.md) · [HTTPS és agentcsatorna](docs/szerver.md) · [Adatbázis](docs/adatbazis.md) · [Hitelesítés](docs/hitelesites.md) · [Munkamenetek](docs/munkamenetek.md) · [Jogosultságok](docs/jogosultsagok.md) · [Audit](docs/audit.md) · [Biztonság](SECURITY.md)

## Mappák

- `server/`: a központi Rust-szerver.
- `web-panel/`: a React/TypeScript webpanel.
- `agent/agent-windows/` és `agent/agent-linux/`: a tervezett platformagentek helye.

Hitelesítéssel a webpanel belépőképernyőt, ügyfélválasztót, jogosultsághoz igazodó navigációt és munkamenet-kezelést ad (`just dev-auth`). Hitelesítés nélkül az előnézet kereshető, szűrhető mintaeszközöket és eszközadatlapot mutat. A mintaadatok nem jelentenek beiratkozott eszközöket; távoli művelet nem indítható.

## Tervezett működés

Távvezérlést kizárólag az adminisztrátor vagy az adott géphez távvezérlési joggal rendelkező technikus kezdeményezhet a webes adminpanelen. Felhasználói gépen az agent megmutatja, ki szeretne kapcsolódni és milyen hozzáférést kér. A felhasználó engedélyezheti vagy elutasíthatja a kérést, és bármikor befejezheti az aktív kapcsolatot. Az adminpanelen az eszközoldal és a helyi menü ugyanazokat az engedélyezett funkciókat kínálja, a monitorozástól és frissítésektől a fájlkezelőig és a távsegítésig. Mindkét felület magyarul és angolul készül.

A **Távoli vezérlés kérése** a felhasználó asztalát nyitja meg. A **Rendszergazdai távvezérlés kérése** az előre megadott rendszergazdai műveleteket is elérhetővé teszi. Megfelelő engedéllyel mindkét módban ugyanabban a kapcsolatban kell látni és kezelni a Windows UAC-promptját. Felhasználó nélküli szerverhez az ügyfél és az adott gép külön szabályzata, technikusi jogosultság és friss erős hitelesítés kell.

A háttéragent változásokat továbbít és halasztható feladatokat ütemez. Távsegítéskor a valós idejű kép és bevitel az elsődleges. Az AD-felderítés jelölteket vesz fel a leltárba. Távoli kezeléshez jóváhagyott agent, tartományba léptetéshez külön engedély kell.

## Technológia és ütemterv

Rust agent és szerver, PostgreSQL 18, mTLS-sel védett protobuf/WebSocket kapcsolat, WebRTC és eturnal TURN-relé, React/TypeScript dashboard, Tauri 2 asztali felület. A célplatformok az agentnél Windows 10/11, Windows Server 2016-tól, Debian, Ubuntu és Rocky Linux. A központi szervernél Windows Server 2022-től, Debian 12/13, Ubuntu 24.04 LTS-től és Rocky Linux 9/10 a cél. A kiadásonkénti támogatást az adott géptípuson és művelettel végzett próbák határozzák meg.

| Fázis | Fő eredmény |
| --- | --- |
| 0. Alapok | Belépés, ügyfélszigetelés, CI, használhatósági próba az agent és a dashboard prototípusán, korai WebRTC/UAC-próba |
| 1. Agent és monitorozás | Regisztráció, leltár, riasztás, eszközoldalak, AD-felderítés és terhelési mérés |
| 2. Távsegítés | Mindkét vezérlési mód, UAC, helyi leállítás, szerveres hozzáférés, fájlátvitel és TURN |
| 3. Karbantartás | Szkriptek, frissítések, háttérműveletek és ellenőrzött Windows-tartományba léptetés |
| 4. Megerősítés és pilot | Telepítők, aláírt frissítés, visszaállítási és biztonsági próba, első éles ügyfél |
| 5. Bővítések | Linuxos grafikus távsegítés, gyorssegítség, hang és felvétel, Entra ID, Web Push és riportok |

Az ügyfélportál, a ticketing és a felhasználói segítségkérés külön javaslat, jelenleg mérföldkő nélkül.

A sebezhetőségeket a [privát hibabejelentőn](https://github.com/Shran21/Midas/security/advisories/new) várjuk. A projekt licence a [GNU AGPL v3](LICENSE).

## English

MIDAS is a design for a self-hosted, open-source remote monitoring and management platform. A technician can manage workstations and servers belonging to multiple customers from a web admin panel.

### Status

Foundation implementation has started: a Rust server with a local liveness endpoint, a Hungarian/English operator preview with light and dark themes and shared build commands. Windows remote support and UAC interaction belong to the subsequent prototype. The reviewed phases 0–5 are tracked in the [GitHub Project](https://github.com/users/Shran21/projects/4): 97 scheduled items and 6 separate proposals.

- The [system plan](docs/rendszerterv.md#english) covers architecture, platforms, security and delivery.
- The [operating requirements](docs/kovetelmenyek.md#english) describe the agent and admin panel, permissions and acceptance trials.
- The [task allocation](docs/feladatlista.md#english) assigns status, milestone, dependencies and acceptance to each item. See the [contribution guide](CONTRIBUTING.md#english) for contributing.

Development startup: `just setup`, then `just dev`. The server foundation supports TLS 1.3, a separate mTLS agent listener, ACME certificate management and PostgreSQL tenant isolation. [Environment and commands](docs/fejlesztes.md#english) · [HTTPS and agent transport](docs/szerver.md#english) · [Database](docs/adatbazis.md#english) · [Authentication](docs/hitelesites.md#english) · [Sessions](docs/munkamenetek.md#english) · [Authorization](docs/jogosultsagok.md#english) · [Audit](docs/audit.md#english) · [Security](SECURITY.md#english)

### Directories

- `server/`: the central Rust server.
- `web-panel/`: the React/TypeScript web panel.
- `agent/agent-windows/` and `agent/agent-linux/`: locations for the planned platform agents.

With authentication the web panel provides sign-in, tenant selection, permission-aware navigation and session management (`just dev-auth`). Without authentication the preview offers searchable, filterable sample devices and device details. Sample data does not represent enrolled devices, and remote operations are unavailable.

### Planned experience

Only an administrator or a technician with remote-control permission for the device can initiate remote control from the web admin panel. On a user's device, the agent shows who wants to connect and what access they are requesting. The user can allow or decline and can end an active session at any time. Device pages and context menus offer the same authorized monitoring, update, file management and support actions. Both interfaces will be available in Hungarian and English.

**Request remote control** opens the user's desktop. **Request administrative remote control** also makes pre-authorized administrative operations available. With the required permission, the technician must see and interact with Windows UAC in the same connection in either mode. Access to a server without a local participant requires both customer and device policies, technician permission and fresh strong authentication.

The background agent sends changes and schedules deferrable work. Live video and input take priority during support. AD discovery adds inventory candidates. Remote management requires an approved agent, and domain joining needs separate permission.

### Technology and roadmap

Rust agent and server, PostgreSQL 18, protobuf over WebSocket secured by mTLS, WebRTC with an eturnal TURN relay, a React/TypeScript dashboard and a Tauri 2 desktop interface. The agent targets Windows 10/11, Windows Server 2016 onwards, Debian, Ubuntu and Rocky Linux. The central server targets Windows Server 2022 onwards, Debian 12/13, Ubuntu 24.04 LTS onwards and Rocky Linux 9/10. Tested device and operation combinations determine support for each release.

| Phase | Principal outcome |
| --- | --- |
| 0. Foundations | Sign-in, tenant isolation, CI, usability trials on agent and dashboard prototypes, early WebRTC/UAC proof |
| 1. Agent and monitoring | Enrollment, inventory, alerts, device pages, AD discovery and load testing |
| 2. Remote support | Both control modes, UAC, local stop, server access, file transfer and TURN |
| 3. Maintenance | Scripts, updates, background operations and verified Windows domain joining |
| 4. Hardening and pilot | Installers, signed updates, recovery and security testing, first production customer |
| 5. Extensions | Graphical Linux support, quick support, audio and recording, Entra ID, Web Push and reports |

The customer portal, ticketing and user-initiated help requests are separate proposals with no milestone assigned.

Please use the [private vulnerability reporting form](https://github.com/Shran21/Midas/security/advisories/new) for security issues. MIDAS uses the [GNU AGPL v3](LICENSE).
