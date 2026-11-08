# Állapotellenőrzések és platformképességek / Health checks and platform capabilities

[English](#english)

## Magyar

2026. november 8. · MIDAS-027 · SERVER-01

### Ellenőrzések

Az agent indulás után legfeljebb egy perc véletlen késleltetéssel, majd ötpercenként futtatja az ellenőrzéseket. Minden ellenőrzés egy állapotot ad: `ok`, `warning`, `critical` vagy `unavailable`. Mellé stabil okkód (a panel ebből fordít), angol nyelvű tartalékszöveg és legfeljebb 16 érintett elem (szolgáltatás, kötet, lemez vagy napok száma) tartozik. Több hiba esetén a legsúlyosabb adja az állapotot és az okot, a szöveg mindegyiket felsorolja.

| Ellenőrzés | Windows | Linux |
| --- | --- | --- |
| `services` | Automatikus indítású, triggerrel nem indított szolgáltatás, amely hibakóddal állt le: `critical` / `stopped` | `systemctl` szerint hibás (`failed`) szolgáltatásegység: `critical` / `stopped` |
| `updates` | Függő újraindítás (`reboot_pending`); az utolsó sikeres telepítés 35 napnál régebbi (`outdated`, `warning`), 60 napnál régebbi (`critical`); a keresés 7 napja nem sikerült (`search_failed`). Forrás: Windows Update agent és a függő újraindítás registrykulcsai | `/run/reboot-required` (`reboot_pending`); függő biztonsági frissítés (`security_pending`, `critical`) vagy egyéb frissítés (`pending`, `warning`) az `apt-get -s upgrade`, illetve a `dnf --cacheonly` alapján; 7 napnál régebbi csomaglista (`search_failed`) |
| `antivirus` | Microsoft Defender: kikapcsolt védelem (`disabled`), kikapcsolt valós idejű védelem (`realtime_off`), 3 napnál régebbi (`warning`) vagy 7 napnál régebbi (`critical`) definíciók (`signatures_old`). Passzív mód más vírusvédelem mellett: `ok` / `passive`. Ha a Defender ki van kapcsolva, de a Windows Biztonsági központ (csak kliens kiadásokon) más, egészséges vírusvédelmet jelez: `ok` / `other_product` | Nincs ellenőrzés: `unavailable` / `not_supported` |
| `encryption` | BitLocker a fix köteteken: nem védett (`not_encrypted`), felfüggesztett (`suspended`), titkosítás vagy visszafejtés alatt (`in_progress`); mind `warning` | A gyökér-fájlrendszer dm-crypt (LUKS) eszközön van-e, LVM alatt is: `ok` / `encrypted` vagy `warning` / `not_encrypted` |
| `storage` | Szabad hely: 10% alatt `warning`, 5% alatt `critical` (`low_space`), kivéve ha legalább 100 GiB szabad; fizikai lemez figyelmeztetése (`disk_warning`) vagy hibája (`disk_unhealthy`, `critical`) | Ugyanaz a szabadhely-szabály; hibát rögzítő ext4-fájlrendszer (`fs_errors`, `warning`). A mindig teli képfájlrendszerek (például snap) kimaradnak |

Ha egy ellenőrzés nem futtatható, nem tűnik el, hanem `unavailable` állapotot kap. Az okkód lehet `not_supported` (a rendszer vagy kiadás nem ismeri), `access_denied` (a futtató jog kevés; a Windows-szolgáltatás LocalSystemként fut, ezért a BitLocker-lekérdezés ott elérhető) vagy `error` (a lekérdezés sikertelen vagy 60 másodpercen túl futott).

### Platformképességek

Az agent a `Hello` üzenetben felsorolja, mit tud az adott platformon, például `metrics`, `inventory`, `health.services`. A Linux-agent nem jelent `health.antivirus` képességet, de az ellenőrzést `unavailable` állapottal megmutatja. A szerver minden kapcsolódáskor felülírja az eszköz tárolt képességlistáját. Képesség legfeljebb 32 lehet, egyenként legfeljebb 48 bájt (`a-z`, `0-9`, `.`, `_`); ennek megsértése érvénytelen `Hello`.

### Átvitel és tárolás

- Az agent a legutóbbi eredményt `HealthReport` üzenetben küldi: kapcsolódáskor, változáskor, és változatlan eredménynél óránként, hogy a szerver lássa, friss. Egyszerre egy jelentés van úton; a következő a `HealthAck` után megy. A jelentés legfeljebb 32 KiB; ha hosszú, többbájtos nevek miatt nagyobb lenne, az agent a leghosszabb listák végéről elhagy érintett elemeket, a szöveg és az állapot megmarad.
- A szerver minden ellenőrzést külön sorban tárol (`device_health`, ügyfélenkénti RLS). A változás ideje csak az állapot vagy az ok változásakor mozdul. Az érvénytelen ellenőrzést kihagyja, a jelentésből kimaradt ellenőrzést törli, és rögzíti a jelentés idejét.
- `GET /api/tenants/{ügyfél}/devices/{eszköz}/health` (`device.read`): képességek, a legutóbbi jelentés ideje és az ellenőrzések. Riasztás a MIDAS-029, megjelenítés a MIDAS-031 része.

### Ellenőrzés

- Az agent-core egységtesztjei a súlyossági sorrendet, a szöveg- és méretkorlátokat (16 ellenőrzés hosszú, többbájtos szöveggel is az üzenethatár alatt marad), a szabadhely-küszöböket és az óránkénti újraküldést vizsgálják.
- A Windows-egységtesztek szimulált hibákkal igazolják a következőket: kikapcsolt Defender, kikapcsolt valós idejű védelem, régi definíciók, nem védett, felfüggesztett és titkosítás alatti BitLocker-kötetek, régi vagy hiányzó frissítés, függő újraindítás, hibás és figyelmeztető lemez, valamint a WMI-hibák `unavailable` okkódjai.
- A valódi fejlesztői gépen mind az öt ellenőrzés választ ad:
  - egy valóban hibakóddal leállt automatikus szolgáltatás `critical`;
  - a frissítés és a Defender `ok`;
  - a BitLocker felhasználói joggal `unavailable` / `access_denied`;
  - a 8%-os szabad hely `warning`.
- A Linux-tesztek a `systemctl`-, `apt`- és `dnf`-kimenetet, a LUKS-ot LVM alatt, az ext4-hibaszámlálókat és a csomaglisták korát ellenőrzik.
- Az adatbázisos tesztcsomag valódi PostgreSQL-lel és mTLS-sel igazolja a következőket:
  - mind az öt eredmény eltárolódik, a nem elérhető ellenőrzés is;
  - a képességek az API-ban látszanak, más ügyfél 404-et kap;
  - állapotváltáskor a változás ideje frissül, csak az érintett elem változásakor nem;
  - a jelentésből kimaradt ellenőrzés törlődik.

### Ismert korlátok

- Valódi szolgáltatás-, Defender- és BitLocker-hibát a fejlesztői gépen nem idéztem elő, mert az a gép biztonsági beállításait módosítaná; ezeket a szimulált tesztek fedik. A tulajdonosi Windows- és Linux-próba ([Windows](meresek/windows-szolgaltatas.md), [Linux](meresek/linux-agent.md)) a szolgáltatásjoggal futó lekérdezést is megmutatja.
- Linuxon a `dnf --cacheonly` a megerősített unit alatt cache nélkül sikertelen lehet; ekkor a frissítési ellenőrzés `unavailable` / `error`.
- A szabadhely-, frissítési és definíciós küszöbök ügyfél-, telephely- és csoportszabályzattal módosíthatók ([szabályzatok](szabalyzatok.md)); az itt leírt értékek az alapértékek.

## English

November 8, 2026 · MIDAS-027 · SERVER-01

### Checks

The agent runs the checks after a random delay of up to one minute, then every five minutes. Each check gives one state: `ok`, `warning`, `critical` or `unavailable`. It also carries a stable reason code (the panel translates it), an English fallback text and up to 16 affected subjects (services, volumes, disks or a day count). With several problems, the most severe one sets the state and reason, and the text lists all of them.

| Check | Windows | Linux |
| --- | --- | --- |
| `services` | An automatic, non-trigger-start service that stopped with an error code: `critical` / `stopped` | A service unit that `systemctl` reports as `failed`: `critical` / `stopped` |
| `updates` | Pending restart (`reboot_pending`); last successful installation older than 35 days (`outdated`, `warning`) or 60 days (`critical`); no successful search for 7 days (`search_failed`). Source: the Windows Update agent and the pending-restart registry keys | `/run/reboot-required` (`reboot_pending`); pending security updates (`security_pending`, `critical`) or other updates (`pending`, `warning`) from `apt-get -s upgrade` or `dnf --cacheonly`; package lists older than 7 days (`search_failed`) |
| `antivirus` | Microsoft Defender: protection off (`disabled`), real-time protection off (`realtime_off`), security intelligence older than 3 days (`warning`) or 7 days (`critical`) (`signatures_old`). Passive mode next to another antivirus: `ok` / `passive`. If Defender is off but Windows Security Center (client editions only) reports another healthy antivirus: `ok` / `other_product` | No check: `unavailable` / `not_supported` |
| `encryption` | BitLocker on fixed volumes: unprotected (`not_encrypted`), suspended (`suspended`), encrypting or decrypting (`in_progress`); all `warning` | Whether the root file system is on a dm-crypt (LUKS) device, also below LVM: `ok` / `encrypted` or `warning` / `not_encrypted` |
| `storage` | Free space: below 10% `warning`, below 5% `critical` (`low_space`), unless at least 100 GiB are free; physical disk warning (`disk_warning`) or failure (`disk_unhealthy`, `critical`) | The same free-space rule; ext4 file systems that recorded errors (`fs_errors`, `warning`). Always-full image file systems (for example snaps) are skipped |

A check that cannot run does not disappear; it becomes `unavailable`. Its reason is `not_supported` (the system or edition lacks it), `access_denied` (the running account lacks rights; the Windows service runs as LocalSystem, so the BitLocker query works there) or `error` (the query failed or ran longer than 60 seconds).

### Platform capabilities

The agent lists in its `Hello` what it can do on its platform, such as `metrics`, `inventory` or `health.services`. The Linux agent does not announce `health.antivirus`, but it still shows that check as `unavailable`. The server replaces the device's stored capability list on every connection. There are at most 32 capabilities of at most 48 bytes each (`a-z`, `0-9`, `.`, `_`); anything else makes the `Hello` invalid.

### Transfer and storage

- The agent sends its latest result as a `HealthReport`: on connecting, on every change, and hourly when unchanged so the server knows it is current. One report is in flight at a time; the next follows the `HealthAck`. A report is at most 32 KiB; if long multi-byte names would make it larger, the agent drops subjects from the end of the longest lists and keeps every text and state.
- The server stores each check as its own row (`device_health`, per-tenant RLS). The change time moves only when the state or the reason changes. Invalid checks are skipped, checks missing from the report are removed, and the report time is recorded.
- `GET /api/tenants/{tenant}/devices/{device}/health` (`device.read`): capabilities, the latest report time and the checks. Alerting belongs to MIDAS-029 and display to MIDAS-031.

### Verification

- agent-core unit tests cover severity ordering, text and size limits (16 checks with long multi-byte texts stay below the message limit), free-space thresholds and hourly republishing.
- Windows unit tests verify the following with simulated faults:
  - Defender turned off, real-time protection off and old security intelligence;
  - unprotected, suspended and encrypting BitLocker volumes;
  - old or missing updates and a pending restart;
  - failing and warning disks;
  - the `unavailable` reasons of WMI errors.
- On the real development machine all five checks answer:
  - an automatic service that really stopped with an error code is `critical`;
  - updates and Defender are `ok`;
  - BitLocker with user rights is `unavailable` / `access_denied`;
  - 8% free space is a `warning`.
- Linux tests check `systemctl`, `apt` and `dnf` output, LUKS below LVM, ext4 error counters and package list age.
- The database suite, with real PostgreSQL and mTLS, verifies the following:
  - all five results are stored, including the unavailable one;
  - capabilities appear in the API, and another customer gets 404;
  - a state change moves the change time, a changed subject alone does not;
  - a check missing from the report is removed.

### Known limits

- Real service, Defender and BitLocker failures were not provoked on the development machine, because that would change its security settings; the simulated tests cover them. The owner's Windows and Linux trials ([Windows](meresek/windows-szolgaltatas.md#english), [Linux](meresek/linux-agent.md#english)) also show the queries running with service rights.
- On Linux `dnf --cacheonly` may fail under the hardened unit without a cache; the update check is then `unavailable` / `error`.
- Free-space, update and security-intelligence thresholds can be changed by customer, site and group policies ([policies](szabalyzatok.md#english)); the values above are the defaults.
