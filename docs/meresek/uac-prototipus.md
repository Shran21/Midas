# UAC- és védett beviteli próba / UAC and protected input proof

[English](#english)

## Magyar

MIDAS-018 · Próbaleírás és jegyzőkönyv. Az elfogadási feltétel: standard felhasználónál a böngésző kezeli a valódi secure-desktop UAC-t, helyi leállítással és a védelmek kikapcsolása nélkül. Ez csak külön tesztgépen igazolható, mert rendszerszolgáltatást és standard felhasználói fiókot igényel. Amíg a VM-es próba eredménye nincs rögzítve, a feladat nem zárható le.

### A prototípus felépítése

Forrás: `agent/agent-windows/uac-prototype`. A WebRTC-átvitel és a kódolás a MIDAS-017 prototípusát használja.

- **Szolgáltatás (`service`)**: LocalSystemként fut a 0. munkamenetben. Az aktív konzolmunkamenetbe saját SYSTEM-tokenjével elindítja a segédet a `winsta0\default` asztalon, és munkamenetváltáskor újraindítja. Kilépett segédet nem indít újra: új távoli munkamenethez a szolgáltatás újraindítása és új kód kell.
- **Munkamenet-segéd (`session`)**: 50 ms-onként figyeli, melyik asztal kapja a bevitelt (normál vagy biztonságos asztal), és minden váltáskor új generációt számol. A képrögzítő szál GDI-vel mindig a bemeneti asztalt rögzíti, a kurzort is beleértve. A bevitel külön szálon, ugyanerre az asztalra kerül.
- **Bevitel**: a böngésző minden eseményhez megadja, melyik asztalgenerációt látta. A régebbi asztalra szánt eseményt a segéd eldobja. Váltáskor és bontáskor a lenyomva maradt billentyűket és gombokat felengedi. A billentyűk fizikai scan code-dal mennek, így a célgép saját kiosztása (például a magyar) érvényes. A Ctrl+Alt+Del nem küldhető.
- **Helyi ablak**: mindig felül, a jobb alsó sarokban jelenik meg. Mutatja a 8 jegyű kapcsolódási kódot és a kapcsolat állapotát. A **Befejezés** gomb és az ablak bezárása is azonnal leállítja a segédet, vele a képet és a bevitelt.
- **Hozzáférés**: kapcsolódni csak a helyi ablakban látható kóddal lehet. Öt hibás próbálkozás után a segéd minden további kérést elutasít. Egyszerre egy kapcsolat lehet.
- **Napló**: a szolgáltatás a bináris melletti `logs` mappába ír; ezt csak rendszergazda írhatja. A bevitelt, így a begépelt jelszót sem, a segéd nem naplózza. A kód nem kerül a naplóba.

### Ismert korlátok

- Csak laborpróba: sima HTTP-jelzéscsatorna, a médiát és a bevitelt a WebRTC DTLS-titkosítása védi. Nincs helyi jóváhagyó folyamat, szerverkapcsolat, audit és újrafelhatalmazás; ezek a 2. fázis feladatai (MIDAS-047–051).
- A próbában egyetlen LocalSystem-folyamat kezeli a normál és a biztonságos asztalt is. A korlátozott felhasználói segéd és az emelt komponens szétválasztása a MIDAS-047 feladata.
- Csak az elsődleges monitor. A GDI-rögzítés lassabb a DXGI-nél; ez a próbához elég.
- A Befejezés ablak a normál asztalon van. A biztonságos asztalon a helyi felhasználó a Windows saját gombjaival (Nem / Esc) zárhatja be a kérést, utána a Befejezés elérhető. A fizikai billentyűzet és egér mindig működik.
- Bontott (disconnected) munkamenetnek nincs bemeneti asztala, ilyenkor a böngésző „nem elérhető” állapotot és üres képet mutat.

### Fejlesztői előellenőrzés (2026-11-06)

Fejlesztői gépen, normál felhasználóként (SYSTEM nélkül), bontott RDP-munkamenetben. Ez nem helyettesíti a VM-es próbát.

- Fordítás, `clippy -D warnings`, egységtesztek: sikeres. A tesztek a bevitel értelmezését, a koordináta- és görgetéskorlátot, a scan code-okat, a régi asztalra szánt bevitel eldobását és a felengedést fedik le.
- Hibás kóddal a kapcsolat elutasított (403), helyes kóddal létrejön. Aktív kapcsolat mellett a második kérés 409. Öt hibás kód után a helyes kód is elutasított.
- A bontott munkamenetben a bemeneti asztal nem nyitható meg (a Windows 5-ös hibakódja). A segéd ezt „nem elérhető” állapotként jelzi, és helyettesítő képpel tovább fut.
- A helyi ablak mindig felül van, mutatja a kódot és kapcsolat közben a „Kapcsolat aktív” állapotot. A Befejezés gombra a segéd 61 ms alatt kilépett, a böngésző a kapcsolat végét jelezte.
- **Nem ellenőrizhető itt:** valódi kép és bevitel (a munkamenet bontott), a biztonságos asztal (LocalSystem kell), a szolgáltatásos indítás és a standard felhasználó.

### VM-es próba menete

Előfeltétel: Windows 11 VM elkülönített (belső vagy privát) hálózaton, a gazdagéppel elérhető IP-címmel. Hyper-V-ben alap munkamenetet használj, ne bővített munkamenetet. Kell egy helyi rendszergazdai fiók és egy standard felhasználó, például `midas-test`, aki nem tagja az Administrators csoportnak.

1. **Kiinduló beállítások rögzítése** (rendszergazdai PowerShell):
   `reg query HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v EnableLUA`, majd ugyanígy a `PromptOnSecureDesktop` és a `ConsentPromptBehaviorUser` érték. Elvárt: 1, 1 és 3 (alapértelmezés). A Microsoft Defender legyen bekapcsolva.
2. **Fordítás a gazdagépen:** `cargo build --release --locked -p midas-uac-prototype`. Másold a `target\release\midas-uac-prototype.exe` fájlt a VM `C:\Program Files\MIDAS\uac-prototype\` mappájába. Csak rendszergazda írhatja ezt a mappát; felhasználó által írható helyre ne tedd.
3. **Telepítés** (rendszergazdai PowerShell; a `<VM-IP>` a VM címe a tesztszámítógép-hálózaton):
   ```powershell
   $exe = "C:\Program Files\MIDAS\uac-prototype\midas-uac-prototype.exe"
   sc.exe create MidasUacPrototype binPath= "`"$exe`" service --address <VM-IP> --port 7701" start= demand
   New-NetFirewallRule -DisplayName "MIDAS UAC proof" -Direction Inbound -Program $exe -Action Allow -Profile Private
   ```
4. Jelentkezz be a standard felhasználóval. Emelt parancssorból indítsd el a szolgáltatást: `sc.exe start MidasUacPrototype`. Az emeléshez megjelenő UAC-ablak itt várt. A jobb alsó sarokban megjelenik a MIDAS-ablak a kóddal.
5. A gazdagép böngészőjében (Chrome vagy Edge) nyisd meg a `http://<VM-IP>:7701` címet, add meg a kódot, majd Kapcsolódás. Elvárt: a VM asztala látszik, „normál asztal” állapottal.
6. A böngészőből, távoli bevitellel indíts emelést, például: Start → „cmd” → Futtatás rendszergazdaként. Elvárt: az állapot „biztonságos asztal (UAC)”, és a kép a hitelesítő adatot kérő UAC-ablakot mutatja.
7. A böngészőből kattints a **Nem** gombra. Elvárt: az ablak bezárul, az állapot ismét „normál asztal”.
8. Ismételd meg az emelést, és a böngészőből add meg a rendszergazdai fiók adatait, majd **Igen**. Elvárt: elindul az emelt parancssor. Zárd be. Jelszót semmilyen jegyzetbe ne írj.
9. Kapcsolat közben a VM-ben a helyi felhasználó kattintson a **Befejezés** gombra. Elvárt: a böngésző néhány másodpercen belül a kapcsolat végét jelzi, további bevitel nem jut át. A `logs\session.log` fájlban a „session ended locally”, a `logs\service.log` fájlban az újraindítást kérő sor látható.
10. Ismételd meg az 1. lépést. Elvárt: a három érték változatlan.
11. **Takarítás:** `sc.exe stop MidasUacPrototype`, `sc.exe delete MidasUacPrototype`, `Remove-NetFirewallRule -DisplayName "MIDAS UAC proof"`, majd töröld a mappát.

Bizonyíték: a böngésző képernyőképei (normál és biztonságos asztal), a `logs` mappa naplói (az asztalváltások időpontjával), valamint a kitöltött táblázat.

| # | Lépés | Elvárt | Eredmény (igen/nem) | Megjegyzés |
| --- | --- | --- | --- | --- |
| 1 | Kiinduló beállítások | 1 / 1 / 3, Defender be | | |
| 5 | Kapcsolódás kóddal | normál asztal látszik | | |
| 6 | Emelés standard felhasználóként | UAC a biztonságos asztalon, a böngészőben látszik | | |
| 7 | Nem a böngészőből | a prompt bezárul | | |
| 8 | Hitelesítés és Igen a böngészőből | emelt program indul | | |
| 9 | Helyi Befejezés | azonnali leállás, a böngésző jelzi | | |
| 10 | Beállítások a próba után | változatlanok | | |

### Követelmények

- **REMOTE-03:** a biztonságos asztal képe és vezérlése ugyanabban a kapcsolatban, a normál asztalra visszatérve folytatódik. A korlátozott segéd szétválasztása: MIDAS-047.
- **REMOTE-05:** UAC, secure desktop és házirendek változatlanok (1. és 10. lépés). Az agent nem kattint automatikusan; a hitelesítést a technikus adja meg.
- **REMOTE-06:** a biztonságos asztal képe és bevitele csak a LocalSystem-segédben él. Asztalváltáskor a régi asztalra szánt bevitel eldobódik. A hitelesített helyi IPC: MIDAS-023 és MIDAS-047.
- **REMOTE-07:** a helyi Befejezés azonnal leállítja a képet és a bevitelt. A szerveroldali visszavonás és a 30 másodperces korlát: MIDAS-051.

## English

MIDAS-018 · Trial description and record. Acceptance: a browser controls real secure-desktop UAC for a standard user, with local termination and security policies enabled. This needs a separate test machine, because it requires a system service and a standard user account. The task cannot be closed until the VM trial result is recorded.

### How the prototype works

Source: `agent/agent-windows/uac-prototype`. WebRTC transport and encoding reuse the MIDAS-017 prototype.

- **Service (`service`)**: runs as LocalSystem in session 0. It starts the helper in the active console session with its own SYSTEM token, on the `winsta0\default` desktop, and restarts it when the console session changes. It does not restart a helper that has exited: a new remote session needs a service restart and a new code.
- **Session helper (`session`)**: checks every 50 ms which desktop receives input (normal or secure) and counts a new generation on each switch. The capture thread always captures the input desktop with GDI, including the pointer. Input is injected on a separate thread into the same desktop.
- **Input**: the browser tags every event with the desktop generation it saw. The helper drops events meant for an earlier desktop. On a switch and on disconnect it releases keys and buttons still held down. Keys travel as physical scan codes, so the target's own layout (for example Hungarian) applies. Ctrl+Alt+Del cannot be sent.
- **Local window**: always on top in the bottom-right corner. It shows the 8-digit connection code and the session state. The **End** button, and closing the window, stop the helper at once, together with capture and input.
- **Access**: connecting requires the code shown in the local window. After five wrong attempts the helper refuses every further request. Only one session at a time.
- **Logging**: the service writes to the `logs` folder next to the binary, which only administrators can write. Input is never logged, so typed passwords are not logged either. The code is not logged.

### Known limits

- Lab trial only: plain HTTP signaling; media and input are protected by WebRTC DTLS. There is no local consent flow, server connection, audit or reauthorization; these are phase 2 work (MIDAS-047–051).
- In the trial a single LocalSystem process handles both the normal and the secure desktop. Splitting off the restricted user worker is MIDAS-047.
- Primary monitor only. GDI capture is slower than DXGI, which is enough for the trial.
- The End window is on the normal desktop. On the secure desktop the local user can dismiss the prompt with Windows' own controls (No / Esc), after which End is available. The physical keyboard and mouse always work.
- A disconnected session has no input desktop; the browser then shows "not accessible" and a blank picture.

### Implementer pre-check (2026-11-06)

On the development machine, as a normal user (no SYSTEM), in a disconnected RDP session. This does not replace the VM trial.

- Build, `clippy -D warnings` and unit tests pass. The tests cover input parsing, coordinate and wheel bounds, scan codes, dropping input meant for an earlier desktop, and releasing held keys.
- A wrong code is refused (403) and the correct code connects. While a session is active, a second request gets 409. After five wrong codes the correct code is refused as well.
- In the disconnected session the input desktop cannot be opened (Windows error 5). The helper reports this as "not accessible" and keeps running with a placeholder picture.
- The local window stays on top, shows the code and, during a session, "Remote session active". On End the helper exited within 61 ms and the browser reported the end of the session.
- **Not verifiable here:** real capture and input (the session is disconnected), the secure desktop (needs LocalSystem), the service start and the standard user.

### VM trial procedure

Prerequisite: a Windows 11 VM on an isolated (internal or private) network, with an IP address the host can reach. In Hyper-V use a basic session, not an enhanced session. You need a local administrator account and a standard user, for example `midas-test`, who is not in the Administrators group.

1. **Record the baseline** (administrator PowerShell):
   `reg query HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v EnableLUA`, then the same for `PromptOnSecureDesktop` and `ConsentPromptBehaviorUser`. Expected: 1, 1 and 3 (the defaults). Microsoft Defender must be on.
2. **Build on the host:** `cargo build --release --locked -p midas-uac-prototype`. Copy `target\release\midas-uac-prototype.exe` to `C:\Program Files\MIDAS\uac-prototype\` on the VM. Only administrators can write this folder; never use a user-writable location.
3. **Install** (administrator PowerShell; `<VM-IP>` is the VM's address on the test network):
   ```powershell
   $exe = "C:\Program Files\MIDAS\uac-prototype\midas-uac-prototype.exe"
   sc.exe create MidasUacPrototype binPath= "`"$exe`" service --address <VM-IP> --port 7701" start= demand
   New-NetFirewallRule -DisplayName "MIDAS UAC proof" -Direction Inbound -Program $exe -Action Allow -Profile Private
   ```
4. Sign in as the standard user. From an elevated prompt, start the service: `sc.exe start MidasUacPrototype`. The UAC prompt for that elevation is expected. The MIDAS window with the code appears in the bottom-right corner.
5. In the host browser (Chrome or Edge) open `http://<VM-IP>:7701`, enter the code and select Connect. Expected: the VM desktop is visible, with the "normal desktop" state.
6. Using remote input from the browser, start an elevation, for example Start → "cmd" → Run as administrator. Expected: the state becomes "secure desktop (UAC)" and the picture shows the UAC credential prompt.
7. Select **No** from the browser. Expected: the prompt closes and the state is "normal desktop" again.
8. Repeat the elevation, enter the administrator credentials from the browser and select **Yes**. Expected: the elevated prompt starts. Close it. Do not write the password into any notes.
9. While connected, the local user on the VM selects **End**. Expected: within seconds the browser reports that the session ended, and no further input gets through. `logs\session.log` shows "session ended locally" and `logs\service.log` shows the line asking for a restart.
10. Repeat step 1. Expected: the three values are unchanged.
11. **Clean up:** `sc.exe stop MidasUacPrototype`, `sc.exe delete MidasUacPrototype`, `Remove-NetFirewallRule -DisplayName "MIDAS UAC proof"`, then delete the folder.

Evidence: browser screenshots (normal and secure desktop), the logs in the `logs` folder (with the desktop switch times) and the completed table.

| # | Step | Expected | Result (yes/no) | Notes |
| --- | --- | --- | --- | --- |
| 1 | Baseline settings | 1 / 1 / 3, Defender on | | |
| 5 | Connect with the code | normal desktop visible | | |
| 6 | Elevation as the standard user | UAC on the secure desktop, visible in the browser | | |
| 7 | No from the browser | prompt closes | | |
| 8 | Credentials and Yes from the browser | elevated program starts | | |
| 9 | Local End | immediate stop, reported in the browser | | |
| 10 | Settings after the trial | unchanged | | |

### Requirements

- **REMOTE-03:** the secure desktop is seen and controlled in the same session, which continues on return to the normal desktop. Splitting off the restricted worker: MIDAS-047.
- **REMOTE-05:** UAC, the secure desktop and policies stay unchanged (steps 1 and 10). The agent never clicks automatically; the technician enters the credentials.
- **REMOTE-06:** secure-desktop frames and input exist only in the LocalSystem helper. On a desktop switch, input meant for the old desktop is dropped. Authenticated local IPC: MIDAS-023 and MIDAS-047.
- **REMOTE-07:** the local End stops capture and input at once. Server-side revocation and the 30-second limit: MIDAS-051.
