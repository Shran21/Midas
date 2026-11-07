# Linux-agent próbája / Linux agent trial

[English](#english)

## Magyar

2026. november 7. · MIDAS-024 · Próbaleírás és jegyzőkönyv

Az elfogadási feltétel: a Debian, az Ubuntu és a Rocky Linux csomagból indítja az agentet, ügyfélhez kötötten és biztonságos szolgáltatásjoggal. A próba csomagot telepít, ezért disztribúciónként külön tesztgépen vagy virtuális gépen végezd.

### Felépítés

- **Program:** `/usr/bin/midas-agent`. A `run` parancs előtérben fut, a systemd így indítja. Az `enroll` parancs a tokent a standard bemenetről olvassa.
- **Konfiguráció:** `/etc/midas-agent/config.json` (root:midas-agent, 0640; a mappa 0750). Minta: `/usr/share/doc/midas-agent/config.example.json`.
- **Állapot:** `/var/lib/midas-agent` (midas-agent, 0700). Itt van a helyben készült kulcs, a tanúsítvány és a függő kérelem.
- **Szolgáltatás:** `midas-agent.service`, a `midas-agent` rendszerfelhasználóval.
  - Nincs képessége (capability), és új jogosultságot sem szerezhet.
  - A fájlrendszer írásvédett, kivéve a saját állapotmappáját; az otthoni mappák rejtettek.
  - Csak IPv4-, IPv6- és Unix-socketet használhat.
  - Rendszerhívás-szűrő tiltja a privilegizált hívásokat.
  - Kilépés után 5 másodperc múlva újraindul, és csak meglévő konfigurációval indul.
- **Ügyfélhez kötés:** a beiratkozási token ügyfélhez, platformhoz (`linux`) és géptípushoz kötött ([beiratkozás](../beiratkozas.md)). A kiadott tanúsítvány az eszközt az ügyfélhez rendeli.
- **CI-bizonyíték:** minden commiton lefut a `systemd-analyze security --offline=yes` elemzés a unitra, 3,0-s kitettségi küszöbbel (`.github/scripts/check-linux-unit.py`).

### Előfeltételek

- Egy-egy Debian 12, Ubuntu 24.04 és Rocky Linux 9 tesztgép (vagy VM) sudo-joggal és hálózati eléréssel a MIDAS-szerverhez.
- A csomagok a [csomagolási leírás](../../agent/agent-linux/packaging/README.md) szerint, Linuxon készülnek: `.deb` és `.rpm`.
- A szerver CA-tanúsítványa PEM-ben, és disztribúciónként egy Linux-szerverre szóló beiratkozási token.

### Lépések (disztribúciónként)

1. **Telepítés:** Debian és Ubuntu alatt `sudo apt install ./midas-agent_*.deb`, Rocky alatt `sudo dnf install ./midas-agent-*.rpm`. Ellenőrizd:
   - `getent passwd midas-agent` nologin shellű rendszerfelhasználót mutat;
   - `systemctl is-enabled midas-agent` eredménye `enabled`;
   - `systemctl status midas-agent` szerint a szolgáltatás a hiányzó konfiguráció miatt nem indult el.
2. **Konfiguráció:** másold a mintát az `/etc/midas-agent/config.json` helyre, és töltsd ki (szerver, portok, `server_ca`, `"kind": "server"`, név). Ezután: `sudo chown root:midas-agent /etc/midas-agent/config.json`, `sudo chmod 0640 /etc/midas-agent/config.json`.
3. **Beiratkozás:** `read -rs TOKEN; printf '%s\n' "$TOKEN" | sudo -u midas-agent /usr/bin/midas-agent enroll; unset TOKEN`. Jóváhagyás után (`POST /api/tenants/{ügyfél}/devices/{eszköz}/approve`) indítsd a szolgáltatást: `sudo systemctl start midas-agent`.
4. **Kapcsolódás:** a `journalctl -u midas-agent` naplóban `connected` állapot jelenik meg, a szerver naplójában pedig „agent session started” bejegyzés ugyanazzal az ügyféllel és eszközzel.
5. **Jogosultság:** `systemctl show -p User,NoNewPrivileges,CapabilityBoundingSet midas-agent` és `sudo systemd-analyze security midas-agent` futtatása. A pontszámot írd a jegyzőkönyvbe. A `ps -o user= -C midas-agent` eredménye `midas-agent` legyen.
6. **Újraindítás és hálózatkiesés:** a `sudo kill -9 $(pidof midas-agent)` után a szolgáltatás 5 másodpercen belül újraindul. Gépújraindítás után bejelentkezés nélkül újra kapcsolódik. A hálózatot legalább két percre bontsd, majd állítsd vissza: két percen belül ismét `connected`.
7. **Eltávolítás:** `sudo apt remove midas-agent` vagy `sudo dnf remove midas-agent`. A szolgáltatás leáll és tiltott lesz; a konfiguráció és az állapot megmarad, és kézzel törölhető.

### Jegyzőkönyv

| Lépés | Debian 12 | Ubuntu 24.04 | Rocky Linux 9 | Megjegyzés |
| --- | --- | --- | --- | --- |
| 1. Telepítés, felhasználó, engedélyezés | | | | |
| 3–4. Beiratkozás és kapcsolódás | | | | |
| 5. Felhasználó és `systemd-analyze security` pontszám | | | | |
| 6. Újraindulás, gépújraindítás, hálózatkiesés | | | | |
| 7. Eltávolítás | | | | |

### Fejlesztői gépen és CI-ban ellenőrizve

- A közös agentlogika (konfiguráció, beiratkozási állapot, kapcsolat) az agent-core-ban van, és a Windows-agenttel közösen tesztelt. Az adatbázisos tesztcsomag a beiratkozást és a csatlakozást valódi szerverrel igazolja.
- A Linux-agent egységtesztjei a csomag útvonalait és a unit legfontosabb korlátozásait ellenőrzik. A Linux-CI lefordítja a release programot, elindítja, és küszöbbel méri a unit kitettségét.
- A csomagtelepítés a három disztribúción csak a fenti próbán igazolható.

### Ismert korlátok

- A unit a későbbi metrikák (MIDAS-025) és ellenőrzések (MIDAS-027) miatt nem rejti el a `/proc` rendszerszintű adatait; ha egy későbbi funkció netlinket igényel, a címcsalád-korlátot bővíteni kell.
- Helyi állapotlekérdező felület Linuxon még nincs; az állapot a naplóban látszik.

## English

November 7, 2026 · MIDAS-024 · Trial description and record

Acceptance: Debian, Ubuntu and Rocky Linux start the agent from a package, bound to the customer and with safe service privileges. The trial installs a package, so use a separate test machine or virtual machine per distribution.

### Design

- **Program:** `/usr/bin/midas-agent`. `run` stays in the foreground, which is how systemd starts it. `enroll` reads the token from standard input.
- **Configuration:** `/etc/midas-agent/config.json` (root:midas-agent, 0640; the folder is 0750). Example: `/usr/share/doc/midas-agent/config.example.json`.
- **State:** `/var/lib/midas-agent` (midas-agent, 0700). It holds the locally created key, the certificate and a pending request.
- **Service:** `midas-agent.service`, running as the `midas-agent` system user.
  - It has no capabilities and cannot gain new privileges.
  - The file system is read-only except its own state folder, and home directories are hidden.
  - It may use only IPv4, IPv6 and Unix sockets.
  - A system-call filter blocks privileged calls.
  - It restarts 5 seconds after exiting and starts only with a configuration present.
- **Customer binding:** the enrollment token is bound to a customer, the `linux` platform and a device kind ([enrollment](../beiratkozas.md#english)). The issued certificate ties the device to that customer.
- **CI evidence:** every commit runs `systemd-analyze security --offline=yes` on the unit with an exposure limit of 3.0 (`.github/scripts/check-linux-unit.py`).

### Prerequisites

- One Debian 12, Ubuntu 24.04 and Rocky Linux 9 test machine (or VM), each with sudo and network access to the MIDAS server.
- Packages built on Linux per the [packaging notes](../../agent/agent-linux/packaging/README.md#english): `.deb` and `.rpm`.
- The server CA certificate as PEM and one Linux server enrollment token per distribution.

### Steps (per distribution)

1. **Install:** on Debian and Ubuntu `sudo apt install ./midas-agent_*.deb`, on Rocky `sudo dnf install ./midas-agent-*.rpm`. Check that:
   - `getent passwd midas-agent` shows a system user with a nologin shell;
   - `systemctl is-enabled midas-agent` returns `enabled`;
   - `systemctl status midas-agent` shows the service did not start because the configuration is missing.
2. **Configure:** copy the example to `/etc/midas-agent/config.json` and fill it in (server, ports, `server_ca`, `"kind": "server"`, name). Then run `sudo chown root:midas-agent /etc/midas-agent/config.json` and `sudo chmod 0640 /etc/midas-agent/config.json`.
3. **Enroll:** `read -rs TOKEN; printf '%s\n' "$TOKEN" | sudo -u midas-agent /usr/bin/midas-agent enroll; unset TOKEN`. After approval (`POST /api/tenants/{tenant}/devices/{device}/approve`) start the service with `sudo systemctl start midas-agent`.
4. **Connect:** `journalctl -u midas-agent` shows the `connected` state, and the server log shows "agent session started" for the same customer and device.
5. **Privileges:** run `systemctl show -p User,NoNewPrivileges,CapabilityBoundingSet midas-agent` and `sudo systemd-analyze security midas-agent`, and record the score. `ps -o user= -C midas-agent` must print `midas-agent`.
6. **Restart and network loss:** after `sudo kill -9 $(pidof midas-agent)` the service restarts within 5 seconds. After a reboot it reconnects without anyone signing in. Disconnect the network for at least two minutes, then restore it: it shows `connected` again within two minutes.
7. **Remove:** `sudo apt remove midas-agent` or `sudo dnf remove midas-agent`. The service stops and is disabled; configuration and state remain and can be deleted manually.

### Record

| Step | Debian 12 | Ubuntu 24.04 | Rocky Linux 9 | Notes |
| --- | --- | --- | --- | --- |
| 1. Install, user, enablement | | | | |
| 3–4. Enrollment and connection | | | | |
| 5. User and `systemd-analyze security` score | | | | |
| 6. Restart, reboot, network loss | | | | |
| 7. Removal | | | | |

### Checked on the development machine and in CI

- The shared agent logic (configuration, enrollment state, connection) lives in agent-core and is tested together with the Windows agent. The database suite verifies enrollment and connection against a real server.
- Linux agent unit tests check the package paths and the unit's key restrictions. Linux CI builds the release program, starts it and measures the unit's exposure against the limit.
- Package installation on the three distributions is verified only by the trial above.

### Known limits

- The unit does not hide system-wide `/proc` data, because later metrics (MIDAS-025) and checks (MIDAS-027) need it; if a later feature needs netlink, the address-family restriction must be widened.
- There is no local status interface on Linux yet; the status appears in the log.
