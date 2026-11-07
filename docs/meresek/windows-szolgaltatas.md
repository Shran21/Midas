# Windows-agent szolgáltatás próbája / Windows agent service trial

[English](#english)

## Magyar

2026. november 7. · MIDAS-023 · Próbaleírás és jegyzőkönyv

Az elfogadási feltétel: a szolgáltatás újraindítás és hálózatkiesés után magától visszatér, idegen folyamat és más Windows-munkamenet pedig nem éri el az IPC-műveleteket. A próba szolgáltatást telepít, ezért külön tesztgépen vagy virtuális gépen végezd, ne a fejlesztői gépen.

### Felépítés

- **`midas-agent.exe`:** a `%ProgramFiles%\MIDAS\agent\` mappából telepíthető, onnan, ahová csak rendszergazda írhat. A szolgáltatás neve `MidasAgent`; LocalSystem fiókkal, automatikusan indul. Hibás leállás után 5, 30 és 60 másodperc múlva újraindul, a számláló naponta nullázódik.
- **Adatkönyvtár:** `%ProgramData%\MIDAS\agent\`, örökölt jogok nélkül, csak SYSTEM és Administrators számára. Itt van a `config.json`, a helyben készült kulcs (`agent.key`), a tanúsítvány (`agent.crt`), a függő kérelem (`pending.json`) és a napló (`logs\agent.log`).
- **Állapotcső:** `\\.\pipe\midas-agent`. A hálózati hozzáférés tiltott, távoli kliens nem csatlakozhat. A szolgáltatás minden kliensnél ellenőrzi két dolgot: a folyamat az aktív konzolmunkamenetben fut-e, és a telepített `midas-agent.exe`-e. Az egyetlen művelet az állapot olvasása, titkot vagy kulcsot nem ad ki. A `midas-agent status` parancs csak 0-s munkamenetben, vagyis szolgáltatásként futó szervertől fogad választ.
- **Kapcsolat:** az [agentprotokoll](../agent-protokoll.md) szerint kiesés után szórt, exponenciális várakozással újracsatlakozik.

### Előfeltételek

- Windows 11-es tesztgép, rendszergazdai fiókkal és egy második, standard felhasználóval. A második felhasználó gyors felhasználóváltással vagy RDP-vel külön munkamenetet nyit.
- Elérhető MIDAS-szerver beiratkozással: `--agent-ca`, `--agent-ca-key`, adatbázis ([beiratkozás](../beiratkozas.md)). Kell hozzá a szerver CA-tanúsítványa PEM-ben, és egy `device.enroll` joggal létrehozott, Windows munkaállomásra szóló token.
- Release build: `cargo build --release --locked -p midas-agent-windows`.

### Lépések

1. **Kiinduló állapot:** rendszergazdai PowerShellben futtasd: `Get-Service MidasAgent -ErrorAction SilentlyContinue`. Nem szabad találatot adnia.
2. **Másolás:** hozd létre a `C:\Program Files\MIDAS\agent\` mappát, és másold bele a `target\release\midas-agent.exe` fájlt.
3. **Telepítés:** `& "C:\Program Files\MIDAS\agent\midas-agent.exe" install --server <szerver> --port 8444 --enrollment-port 443 --server-ca <server-ca.pem>`. Ellenőrizd:
   - `Get-Service MidasAgent` az Automatic indulást mutatja;
   - `sc.exe qfailure MidasAgent` a három újraindítási műveletet mutatja;
   - `icacls C:\ProgramData\MIDAS\agent` csak a SYSTEM és az Administrators bejegyzést mutatja.
4. **Beiratkozás:** a tokent standard bemeneten add át, ne parancssori argumentumként, így nem kerül a parancselőzménybe: `Read-Host "Token" | & "...\midas-agent.exe" enroll`. Ezután hagyd jóvá az eszközt (`POST /api/tenants/{ügyfél}/devices/{eszköz}/approve`). A szolgáltatás egy percen belül átveszi a tanúsítványt, majd kapcsolódik.
5. **Állapot a konzolról:** a konzolmunkamenetben futtasd: `& "...\midas-agent.exe" status`. Várt eredmény: `{"state":"connected","protocol":1}`.
6. **Hálózatkiesés:** bontsd a hálózatot (kábel, vagy a virtuális gép hálózati adaptere) legalább két percre, majd állítsd vissza. A `status` közben `waiting`, utána két percen belül ismét `connected`. A szervernaplóban új „agent session started” bejegyzés jelenik meg ugyanazzal az eszközzel.
7. **Újraindítás:** indítsd újra a gépet, de ne jelentkezz be. A szervernaplóban a gép bejelentkezés nélkül is újra kapcsolódik. Bejelentkezés után a `status` ismét `connected`.
8. **Más munkamenet:** a második felhasználó a saját munkamenetében futtassa: `& "C:\Program Files\MIDAS\agent\midas-agent.exe" status`. Várt eredmény: nincs állapot, a hívás hibával vagy üresen tér vissza; a szolgáltatás naplójában „status client refused” és `OtherSession` szerepel.
9. **Idegen folyamat:** a konzolmunkamenetben másold a programot egy felhasználói mappába (például `Copy-Item "...\midas-agent.exe" $env:TEMP`), és onnan futtasd a `status` parancsot. Próbáld PowerShell-ből is: `$p=New-Object System.IO.Pipes.NamedPipeClientStream('.','midas-agent','In'); $p.Connect(3000); (New-Object System.IO.StreamReader($p)).ReadToEnd()`. Várt eredmény: egyik sem kap állapotot, a naplóban `ForeignProcess` szerepel.
10. **Leállítás és eltávolítás:** `Stop-Service MidasAgent` után a szervernaplóban lezárul a munkamenet. Végül `& "...\midas-agent.exe" uninstall`, majd a `C:\Program Files\MIDAS\agent\` és a `C:\ProgramData\MIDAS\agent\` mappa törlése, ha a gépet visszaállítod.

### Jegyzőkönyv

| Lépés | Elvárt | Eredmény | Megjegyzés |
| --- | --- | --- | --- |
| 3. Telepítés | Automatic, 3 újraindítási művelet, szűk ACL | | |
| 4. Beiratkozás | Jóváhagyás után `connected` | | |
| 5. Állapot a konzolról | `connected` | | |
| 6. Hálózatkiesés | `waiting`, majd `connected` | | |
| 7. Újraindítás | Bejelentkezés nélkül is kapcsolódik | | |
| 8. Más munkamenet | Nincs állapot, `OtherSession` | | |
| 9. Idegen folyamat | Nincs állapot, `ForeignProcess` | | |
| 10. Leállítás | A munkamenet lezárul, eltávolítható | | |

### Fejlesztői gépen ellenőrizve

- A Windows-os integrációs tesztek (`agent/agent-windows/service/tests/ipc.rs`) igazolják a következőket. Az engedélyezett program a saját munkamenetéből megkapja az állapotot. Idegen program (PowerShell) ugyanabból a munkamenetből nem kap semmit. Más munkamenet-szabály mellett a saját program sem kap választ. A csőnév második szerverrel nem foglalható le.
- Az egységtesztek a munkamenet- és programellenőrzést (kis- és nagybetű, `\\?\` előtag, más mappa), a konfiguráció ellenőrzését, az állapotfájlok atomi írását és a titokmentes állapotkimenetet vizsgálják.
- A szolgáltatás telepítése, az újraindítás és a második munkamenet csak a fenti próbán igazolható.

### Ismert korlátok

- Az állapotcső csak az aktív konzolmunkamenetnek válaszol. Csak RDP-n használt gépen így a bejelentkezett felhasználó sem látja az állapotot; a felhasználói állapotnézet és tálcaikon (MIDAS-034) ezt a szabályt pontosíthatja.
- A `logs\agent.log` még nem forog; a méretkorlát és a megőrzés a későbbi üzemeltetési feladatok része.
- A szolgáltatás LocalSystem fiókkal fut, mert a későbbi távsegítési és karbantartási funkciók ezt igénylik; a hálózati és a helyi műveletek jogosultságát a szerver és a kapcsolódó feladatok szabályozzák.

## English

November 7, 2026 · MIDAS-023 · Trial description and record

Acceptance: the service recovers on its own after a restart and a network loss, and neither a foreign process nor another Windows session can reach the IPC operations. The trial installs a service, so run it on a separate test machine or virtual machine, not on the development machine.

### Design

- **`midas-agent.exe`:** installs only from `%ProgramFiles%\MIDAS\agent\`, a location only administrators can write. The service is `MidasAgent`; it runs as LocalSystem and starts automatically. After a failure it restarts after 5, 30 and 60 seconds, with the counter reset daily.
- **Data directory:** `%ProgramData%\MIDAS\agent\`, without inherited access, for SYSTEM and Administrators only. It holds `config.json`, the locally created key (`agent.key`), the certificate (`agent.crt`), a pending request (`pending.json`) and the log (`logs\agent.log`).
- **Status pipe:** `\\.\pipe\midas-agent`. Network access is denied and remote clients are refused. For every client the service checks two things: the process runs in the active console session, and it is the installed `midas-agent.exe`. The only operation reads status, never a secret or key. `midas-agent status` accepts an answer only from a server in session 0, that is, running as a service.
- **Connection:** reconnects with jittered exponential backoff after a loss, per the [agent protocol](../agent-protokoll.md#english).

### Prerequisites

- A Windows 11 test machine with an administrator account and a second, standard user. The second user opens a separate session through fast user switching or RDP.
- A reachable MIDAS server with enrollment: `--agent-ca`, `--agent-ca-key`, database ([enrollment](../beiratkozas.md#english)). You also need the server CA certificate as PEM and a Windows workstation token created with `device.enroll`.
- Release build: `cargo build --release --locked -p midas-agent-windows`.

### Steps

1. **Baseline:** in an elevated PowerShell, `Get-Service MidasAgent -ErrorAction SilentlyContinue` must return nothing.
2. **Copy:** create `C:\Program Files\MIDAS\agent\` and copy `target\release\midas-agent.exe` into it.
3. **Install:** `& "C:\Program Files\MIDAS\agent\midas-agent.exe" install --server <server> --port 8444 --enrollment-port 443 --server-ca <server-ca.pem>`. Check that:
   - `Get-Service MidasAgent` shows Automatic start;
   - `sc.exe qfailure MidasAgent` shows the three restart actions;
   - `icacls C:\ProgramData\MIDAS\agent` lists only SYSTEM and Administrators.
4. **Enroll:** pass the token on standard input, never as an argument, so it stays out of the command history: `Read-Host "Token" | & "...\midas-agent.exe" enroll`. Then approve the device (`POST /api/tenants/{tenant}/devices/{device}/approve`). The service collects the certificate within a minute and connects.
5. **Status from the console:** in the console session run `& "...\midas-agent.exe" status`. Expected: `{"state":"connected","protocol":1}`.
6. **Network loss:** disconnect the network (cable or the virtual machine's adapter) for at least two minutes, then restore it. Meanwhile `status` shows `waiting`, and within two minutes after restoring it shows `connected` again. The server log shows a new "agent session started" for the same device.
7. **Restart:** restart the machine without signing in. The server log shows the machine reconnecting without any sign-in. After signing in, `status` shows `connected` again.
8. **Another session:** the second user runs `& "C:\Program Files\MIDAS\agent\midas-agent.exe" status` in their own session. Expected: no status, the call returns an error or nothing; the service log shows "status client refused" with `OtherSession`.
9. **Foreign process:** in the console session, copy the program to a user folder (for example `Copy-Item "...\midas-agent.exe" $env:TEMP`) and run `status` from there. Also try from PowerShell: `$p=New-Object System.IO.Pipes.NamedPipeClientStream('.','midas-agent','In'); $p.Connect(3000); (New-Object System.IO.StreamReader($p)).ReadToEnd()`. Expected: neither receives status, and the log shows `ForeignProcess`.
10. **Stop and remove:** after `Stop-Service MidasAgent` the server log shows the session closing. Finally run `& "...\midas-agent.exe" uninstall`, and delete `C:\Program Files\MIDAS\agent\` and `C:\ProgramData\MIDAS\agent\` when resetting the machine.

### Record

| Step | Expected | Result | Notes |
| --- | --- | --- | --- |
| 3. Install | Automatic, 3 restart actions, narrow ACL | | |
| 4. Enroll | `connected` after approval | | |
| 5. Console status | `connected` | | |
| 6. Network loss | `waiting`, then `connected` | | |
| 7. Restart | Connects without sign-in | | |
| 8. Another session | No status, `OtherSession` | | |
| 9. Foreign process | No status, `ForeignProcess` | | |
| 10. Stop | Session closes, removable | | |

### Checked on the development machine

- Windows integration tests (`agent/agent-windows/service/tests/ipc.rs`) show the following. The allowed program in its own session reads the status. A foreign program (PowerShell) in the same session receives nothing. Under another session rule even the allowed program gets no answer. A second server cannot take over the pipe name.
- Unit tests cover the session and program checks (case, `\\?\` prefix, other folder), configuration validation, atomic state writes and secret-free status output.
- Service installation, restart and a second session are verified only by the trial above.

### Known limits

- The status pipe answers only the active console session. On a machine used only over RDP even the signed-in user does not see the status; the user status view and tray icon (MIDAS-034) may refine this rule.
- `logs\agent.log` is not rotated yet; size limits and retention belong to later operations work.
- The service runs as LocalSystem because later remote-support and maintenance features need it; the server and the related tasks govern which network and local operations are allowed.
