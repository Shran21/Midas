# Agent állapotnézet és tálcaikon / Agent status view and tray icon

[English](#english)

## Magyar

2026. november 8. · MIDAS-034, MIDAS-039 · UI-06, UI-08, PERF-04

### Felépítés

- A `midas-tray.exe` a bejelentkezett felhasználó jogával futó, kis natív Windows-program: tálcaikon, menü és üzenetablak, WebView nélkül, így állandóan futva is csak minimális erőforrást használ.
- Az MSI a `midas-agent.exe` mellé, a csak rendszergazdák által írható `%ProgramFiles%\MIDAS\Agent` mappába telepíti. A gépre vonatkozó indítási bejegyzés (`HKLM\...\Run`, „MIDAS Agent”) miatt minden felhasználó bejelentkezésekor elindul. Felhasználói munkamenetenként egy példány fut.
- Az állapotot ötmásodpercenként a szolgáltatás védett állapotcsövéből olvassa. A cső csak a konzolmunkamenetből és csak a telepített `midas-agent.exe`-nek vagy a mellette lévő `midas-tray.exe`-nek válaszol. Más mappából indított, azonos nevű program nem kap választ. Az állapot kulcsot, tokent vagy más titkot nem tartalmaz.
- A tálcaprogram csak szolgáltatásként (0-s munkamenetben) futó szervertől fogad állapotot. Más szervert azonnal bont; a csövet ehhez előbb ellenőrzi, és csak utána adja át az aszinkron olvasásnak. Így az elutasított lekérdezések sem halmoznak fel leírót. A 24 órás mérés első próbája ötmásodpercenként egy elveszett leírót mutatott; a javítást integrációs teszt őrzi.
- A felület nyelve a Windows felhasználói nyelvét követi (magyar vagy angol).

### Amit a felhasználó lát

| Állapot | Ikon | Szöveg és teendő |
| --- | --- | --- |
| Kapcsolódva | információ | „Kapcsolódva a MIDAS szerverhez” |
| Kapcsolódás folyamatban | információ | „Kapcsolódás…” |
| Hálózati hiba | figyelmeztetés | „A szerver nem érhető el”: ellenőrizze a hálózatot; az agent magától újrapróbál |
| A szerver nem fogadta el a kapcsolatot | hiba | csak a rendszergazda rendezheti (tanúsítvány, verzió) |
| Jóváhagyásra vár | figyelmeztetés | a rendszergazdának jóvá kell hagynia a gépet; a felhasználónak nincs teendője |
| Elutasított regisztráció | hiba | szóljon a rendszergazdának, ha a gépnek kezeltnek kell lennie |
| Nincs beállítva | hiba | a telepítés nem fejeződött be |
| A szolgáltatás nem fut | hiba | indítsa újra a gépet; ha megmarad, szóljon a rendszergazdának |

- A buboréküzenet csak romláskor jelenik meg, a teendővel. A kapcsolat helyreállásakor egyszer jelzi, hogy újra kapcsolódik.
- Bal kattintás vagy a menü **Részletek…** pontja megmutatja az állapotot és a teendőt. Az **Ikon elrejtése** a mostani bejelentkezésre bezárja a programot; a következő bejelentkezéskor újra indul.
- A távsegítési kérés helyi jóváhagyó ablaka (Engedélyezés, Elutasítás, Befejezés) a MIDAS-048 része; a tálcaprogram lesz a helye.

### Ellenőrzés

- Egységteszt: minden állapot szövege rövid, a hibás állapotoknak van teendője, magyarul és angolul. Hibás vagy hiányzó válasz „A szolgáltatás nem fut” állapotot ad.
- Csőteszt: a tálcaprogram a telepített agent mellől olvashat, más mappából nem.
- A fejlesztői gépen a program elindult, és szolgáltatás nélkül a „nem fut” állapotot mutatta. A második példány azonnal kilépett.
- A CI a Windows-futón telepíti az MSI-t, és ellenőrzi a tálcaprogram fájlját és az indítási bejegyzést, valamint ezek eltávolítását.

### Saját gépes próba

A [Windows-szolgáltatás próbája](meresek/windows-szolgaltatas.md) után, MSI-vel telepített agenten:

1. Jelentkezz be: a tálcán megjelenik a MIDAS ikon. Bal kattintásra a „Kapcsolódva…” szöveg látszik.
2. Válaszd le a hálózatot: fél percen belül figyelmeztető ikon és „A szerver nem érhető el” buborék jelenik meg. Visszakapcsoláskor „Kapcsolódva…” buborék jön.
3. Állítsd le a szolgáltatást (`sc stop MidasAgent`): hibaikon és „A MIDAS szolgáltatás nem fut” jelenik meg. Indítsd újra.
4. Az **Ikon elrejtése** után jelentkezz ki és be: az ikon újra megjelenik.
5. Egy második, nem rendszergazdai felhasználóval is jelentkezz be: ott is megjelenik az ikon, a többi lépés ugyanúgy működik.

### Ismert korlátok

- Csak a konzolmunkamenet kap állapotot; RDP-munkamenetben az ikon „nem fut” állapotot mutat. Ez a cső szándékos szigorítása, és a dokumentáció jelzi.
- Linuxon még nincs tálcaikon; az állapot a szolgáltatásnaplóban látszik (`journalctl -u midas-agent`).

## English

November 8, 2026 · MIDAS-034, MIDAS-039 · UI-06, UI-08, PERF-04

### Design

- `midas-tray.exe` is a small native Windows program running with the signed-in user's rights: tray icon, menu and message box, no WebView, so staying resident costs very little.
- The MSI installs it next to `midas-agent.exe` in `%ProgramFiles%\MIDAS\Agent`, which only administrators can write. A machine-wide startup entry (`HKLM\...\Run`, "MIDAS Agent") starts it at every user sign-in; one instance runs per user session.
- Every five seconds it reads the status from the service's protected status pipe. The pipe answers only the console session and only the installed `midas-agent.exe` or the `midas-tray.exe` next to it; a program with the same name started from another folder gets no answer. The status carries no keys, tokens or other secrets.
- The tray accepts status only from a server running as a service (session 0) and drops any other server at once. It checks the pipe first and only then hands it to asynchronous reading, so refused queries do not pile up handles either. The first trial of the 24-hour measurement showed one lost handle every five seconds; an integration test guards the fix.
- The interface follows the user's Windows display language (Hungarian or English).

### What the user sees

| State | Icon | Text and next step |
| --- | --- | --- |
| Connected | information | "Connected to the MIDAS server" |
| Connecting | information | "Connecting…" |
| Network problem | warning | "The server cannot be reached": check the network; the agent retries on its own |
| The server did not accept the connection | error | only the administrator can fix it (certificate, version) |
| Waiting for approval | warning | the administrator has to approve the computer; nothing to do for the user |
| Enrollment refused | error | tell the administrator if the computer should be managed |
| Not set up | error | the installation is incomplete |
| Service not running | error | restart the computer; if it stays, tell the administrator |

- A balloon appears only when things get worse, with the next step, and once when the connection recovers.
- A left click or **Details…** in the menu shows the state and the next step. **Hide icon** closes the program for this sign-in; it starts again at the next sign-in.
- The local consent window for support requests (Allow, Decline, End) belongs to MIDAS-048 and will live in this program.

### Verification

- Unit test: every state has a short text, error states have a next step, in Hungarian and English; a malformed or missing answer means "service not running".
- Pipe test: the tray program next to the installed agent may read; one from another folder may not.
- On the development machine the program started, showed "not running" without the service, and a second instance exited at once.
- CI installs the MSI on the Windows runner and checks the tray program file, the startup entry and their removal.

### Trial on your own machine

After the [Windows service trial](meresek/windows-szolgaltatas.md#english), with the agent installed from the MSI:

1. Sign in: the MIDAS icon appears in the tray. A left click shows "Connected…".
2. Disconnect the network: within half a minute a warning icon and a "The server cannot be reached" balloon appear. Reconnecting brings a "Connected…" balloon.
3. Stop the service (`sc stop MidasAgent`): an error icon and "The MIDAS service is not running" appear. Start it again.
4. After **Hide icon**, sign out and in: the icon is back.
5. Sign in with a second, non-administrator user: the icon appears there too, and the other steps work the same.

### Known limits

- Only the console session receives status; in an RDP session the icon shows "not running". This is the pipe's deliberate restriction and is documented.
- Linux has no tray icon yet; the state shows in the service log (`journalctl -u midas-agent`).
