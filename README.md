# MIDAS

Nyílt forráskódú távfelügyeleti (RMM) platform ügyfelek IT-környezetéhez: folyamatos monitorozás, automatizált karbantartás és gyors, jó képminőségű távsegítség, egy helyről, több ügyfélre.

> **English:** MIDAS is an open-source RMM platform for monitoring, automated maintenance and remote support. The project documentation is in Hungarian; the dashboard will be available in Hungarian and English.

## Állapot

A rendszerterv elkészült, a fejlesztés a **0. fázissal** (Alapok) indul. Futtatható kód még nincs.

- Rendszerterv: [docs/rendszerterv.md](docs/rendszerterv.md)
- Fázisok és feladatok: [mérföldkövek](https://github.com/Shran21/MIDAS/milestones) és [issue-k](https://github.com/Shran21/MIDAS/issues)

## Fő funkciók

- **Távsegítés WebRTC-n**: hardveres AV1, H.265 vagy H.264 kódolás, ha a videokártya tudja, és szoftveres H.264, ha nem; így bármilyen gépen, GPU nélkül is működik. Jóváhagyással (attended) vagy ügyfélszintű szabályzattal jóváhagyás nélkül (unattended).
- **Monitorozás és riasztás**: heartbeat, metrikák, leltár, ellenőrzések, öröklődő szabályzatok, értesítés e-mailben, webhookon és Web Push-sal.
- **Automatizált karbantartás**: aláírt szkriptkönyvtár, ütemezett feladatok, Windows Update, winget, apt és dnf, karbantartási ablakok.
- **Ügyfelek és jogosultságok**: ügyfél, telephely, eszközcsoport; szerepek és felhasználónként állítható engedélyek; belépés passkey-jel, TOTP-vel, Active Directoryval (LDAPS) és Entra ID-val.
- **Böngészős dashboard**: magyar és angol nyelv, világos és sötét téma, saját mobilnézet, PWA-ként telepíthető.
- **Szerver Linuxra és Windowsra**: aláírt telepítő (.deb, .rpm, illetve setup.exe) és webes beállító varázsló, beégetett alapjelszó nélkül.

## Technológia röviden

| Rész | Választás |
| --- | --- |
| Agent és szerver | Rust (axum, tokio, rustls, sqlx) |
| Adatbázis | PostgreSQL 18, sorszintű biztonsággal (RLS) |
| Agent–szerver kapcsolat | protobuf WebSocketen, mTLS kliens-tanúsítvánnyal |
| Távsegítés | WebRTC, TURN-relé: eturnal |
| Dashboard | React, TypeScript, Vite, Tailwind CSS, shadcn/ui |
| Asztali app | Tauri 2 |
| Támogatott rendszerek | Agent: Windows 10/11, Windows Server 2016-tól, Debian, Ubuntu, Rocky Linux. Szerver: Windows Server 2022-től, Debian 12/13, Ubuntu 24.04 LTS-től, Rocky Linux 9/10 |

A döntések indoklása a [rendszertervben](docs/rendszerterv.md#technológiai-döntések) olvasható.

## Ütemterv

| Fázis | Tartalom |
| --- | --- |
| 0. Alapok | repó, CI, beállító varázsló, belépés, jogosultságok, ügyfélmodell |
| 1. Agent és monitorozás | Windows- és Linux-agent, mTLS, metrikák, leltár, riasztás, MSI, asztali app |
| 2. Távsegítés | prototípus és mérés, hardveres és szoftveres kódolás, bevitel, fájlátvitel, TURN |
| 3. Automatizált karbantartás | szkriptek, ütemezés, Windows Update, winget, apt és dnf |
| 4. Megerősítés és pilot | telepítők, kódaláírás, önfrissítés, mentés, biztonsági audit, első éles ügyfél |
| 5. Bővítések | Linux-távsegítés, gyorssegítség, hang, felvétel, Entra ID, riportok, ügyfélportál |

## Biztonság

Ha sebezhetőséget találsz, kérjük, ne nyiss róla nyilvános issue-t, hanem jelentsd be privát módon a [Security fülön](https://github.com/Shran21/MIDAS/security/advisories/new). A részletes folyamat a SECURITY.md-be kerül a 0. fázisban.

## Licenc

[GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0).
