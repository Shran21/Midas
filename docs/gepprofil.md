# Gépprofil: grafikus és háttérképességek / Host profile: graphical and background capabilities

[English](#english)

## Magyar

2026. november 8. · MIDAS-039 · SERVER-03, PERF-04

A gépprofil megmondja, hogy egy gépen van-e grafikus asztal. A webpanel ennek alapján különíti el a grafikus képességeket (távsegítés, bejelentkező és zárolt képernyő) a háttérképességektől (metrikák, leltár, állapotellenőrzés, terminál, fájlok, szkriptek, frissítések). Server Core-on és más, grafikus asztal nélküli gépen a terminált és a háttéreszközöket kínálja.

### Felismerés az agentben

- **Windows:** a `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` kulcs `InstallationType` értéke (64 bites nézet):
  - `Client`: grafikus asztal, munkaállomás;
  - `Server`: grafikus asztal (Desktop Experience), kiszolgáló;
  - `Server Core` és `Nano Server`: grafikus asztal nélküli kiszolgáló;
  - más vagy hiányzó érték: ismeretlen.
- **Linux:** grafikus, ha a systemd alapcélja (`default.target`) a `graphical.target`, vagy ha be van állítva kijelzőkezelő (`/etc/systemd/system/display-manager.service`). Más alapcélnál grafikus asztal nélküli; systemd-adat nélkül ismeretlen. A Linuxos grafikus távsegítés külön, későbbi fejlesztés.
- Az agent a `Hello` üzenet képességei között jelenti: `host.desktop` (van grafikus asztal), `host.headless` (nincs), `host.server` (kiszolgálókiadás). Ismeretlen asztalnál egyiket sem küldi. A Windows-leltár az `os` fajtában az `installation` kulcson a nyers értéket is tartalmazza.

### A webpanelen

- Az **Áttekintés** eszközadatai között az **Asztal** sor mutatja: grafikus asztal, nincs grafikus asztal, vagy az agent nem jelentette (régebbi agent).
- A **Gépprofil** panel két oszlopban sorolja fel a grafikus és a háttérképességeket, állapottal:
  - **Elérhető:** az agent jelenti (metrikák, leltár, állapotellenőrzés);
  - **Későbbi verzióban:** még nincs kész;
  - **Grafikus asztal nélkül nem:** Server Core és más, asztal nélküli gép;
  - **Linuxon későbbi fejlesztés**, **Ismeretlen asztal** és **Az agent nem jelenti**.

  A panel a gépet írja le. A tényleges használathoz a megfelelő jogosultság is kell; ezt továbbra is a közös műveletlista és a szerver dönti el.
- Asztal nélküli gépen a távsegítés és a rendszergazdai távsegítés a menüben, a palettán és az eszközoldalon is „Nincs grafikus asztal a gépen” okkal tiltott. A gyakori műveletek ilyenkor a távsegítés helyett a terminált és a fájlkezelést kínálják.

### Ellenőrzés

- Agent-egységtesztek: a négy Windows-telepítéstípus és az ismeretlen érték leképezése, a Linuxos alapcél és kijelzőkezelő kombinációi, valamint a képességlista sorrendje.
- A Windows-integrációs teszt a valódi gépen kiolvassa az `InstallationType` értéket. A fejlesztői Windows 11-en ez `Client`, a CI Windows Serverén `Server`, és mindkét gépen bekerül a leltárba.
- Webes teszt: Server Core-profilnál a távsegítés `headless` okkal tiltott, a gyakori műveletek a terminált és a fájlokat adják, a grafikus képességek „asztal nélkül nem” állapotúak. Desktop Experience-es kiszolgálón, régebbi agentnél és Linuxon a megfelelő állapot jelenik meg.

### Ismert korlátok

- Valódi Server Core-gépen és grafikus asztal nélküli Linux-kiszolgálón még nem próbáltam; ott a leképezést az egységtesztek igazolják.
- A profil a telepítés jellegét írja le, nem azt, hogy éppen be van-e jelentkezve valaki. A bejelentkező és a zárolt képernyő kezelése a távsegítés részeként készül.

## English

November 8, 2026 · MIDAS-039 · SERVER-03, PERF-04

The host profile tells whether a host has a graphical desktop. The web panel uses it to separate graphical capabilities (remote support, sign-in and locked screen) from background capabilities (metrics, inventory, health checks, terminal, files, scripts, updates). On Server Core and other hosts without a graphical desktop it offers the terminal and background tools.

### Detection in the agent

- **Windows:** the `InstallationType` value of `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` (64-bit view):
  - `Client`: graphical desktop, workstation;
  - `Server`: graphical desktop (Desktop Experience), server;
  - `Server Core` and `Nano Server`: server without a graphical desktop;
  - any other or missing value: unknown.
- **Linux:** graphical when the systemd default target (`default.target`) is `graphical.target` or a display manager is configured (`/etc/systemd/system/display-manager.service`). Any other default target means no graphical desktop; without systemd information it is unknown. Linux graphical remote support is a separate later feature.
- The agent reports it among the `Hello` capabilities: `host.desktop` (graphical desktop present), `host.headless` (none), `host.server` (server edition). It sends neither desktop value when unknown. The Windows inventory also carries the raw value in the `os` kind under the `installation` key.

### In the web panel

- Among the device details on **Overview**, the **Desktop** row shows: graphical desktop, no graphical desktop, or not reported by the agent (older agents).
- The **Host profile** panel lists graphical and background capabilities in two columns, each with a state:
  - **Available:** the agent reports it (metrics, inventory, health checks);
  - **In a later version:** not built yet;
  - **Not without a desktop:** Server Core and other hosts without a desktop;
  - **Later on Linux**, **Desktop unknown** and **Not reported by the agent**.

  The panel describes the host. Using a capability also needs the matching permission, which the shared action list and the server still decide.
- On a host without a desktop, remote support and administrator remote support are blocked with "The host has no graphical desktop" in the menu, the palette and the device page. The common actions then offer the terminal and file management instead of remote support.

### Verification

- Agent unit tests: mapping of the four Windows installation types and an unknown value, combinations of the Linux default target and display manager, and the order of the capability list.
- The Windows integration test reads `InstallationType` on the real machine. It is `Client` on the Windows 11 development machine and `Server` on the CI Windows Server, and on both it is part of the inventory.
- Web test: with a Server Core profile, remote support is blocked as `headless`, the common actions are terminal and files, and graphical capabilities are "not without a desktop". A Desktop Experience server, an older agent and Linux show their matching states.

### Known limits

- Not yet tried on a real Server Core machine or a Linux server without a graphical desktop; there the unit tests verify the mapping.
- The profile describes the installation, not whether someone is signed in. Sign-in and locked-screen handling comes with remote support.
