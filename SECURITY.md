# Biztonság / Security

[English](#english)

## Magyar

A MIDAS fejlesztés alatt áll; még nincs éles üzemre támogatott kiadás. A tervezett távoli hozzáférés, rendszergazdai műveletek és ügyfélszigetelés elfogadási feltételeit a [működési követelmények](docs/kovetelmenyek.md) rögzítik.

### Bejelentés

Sebezhetőséget a [GitHub privát bejelentőjén](https://github.com/Shran21/Midas/security/advisories/new) küldj. Add meg az érintett verziót vagy commitot, az operációs rendszert, a reprodukció lépéseit és a várható hatást. Lehetőleg mesterséges adatokkal készült példát csatolj; jelszót, privát kulcsot és ügyféladatot hagyj ki.

A javítás és a nyilvános közzététel időzítését a privát bejelentésben egyeztetjük. A kiadott javítás ismert korlátait és az érintett verziókat dokumentáljuk.

### Fejlesztési alapelvek

- A hozzáférés alapból tiltott, az engedély ügyfélhez, eszközhöz és művelethez kötött.
- A helyi rendszergazdai telepítés önmagában nem engedélyez távoli hozzáférést.
- Hitelesítő adatok, billentyűleütések és vágólaptartalom nem kerülhetnek a naplóba.
- Biztonsági határ módosításához jogosultságot megtagadó próbák és az érintett követelmények felülvizsgálata kell.
- Éles pilot előtt külső biztonsági felülvizsgálat, visszaállítási próba és a kritikus, illetve magas kockázatú hibák javítása szükséges.

## English

MIDAS is under development and has no release supported for production. The [operating requirements](docs/kovetelmenyek.md#english) define acceptance for remote access, privileged operations and tenant isolation.

### Reporting

Submit vulnerabilities through [GitHub private reporting](https://github.com/Shran21/Midas/security/advisories/new). Include the affected version or commit, operating system, reproduction steps and expected impact. Prefer a minimal example with synthetic data; omit passwords, private keys and customer data.

Coordinate remediation and public disclosure in the private report. Document affected versions and known limitations of the fix.

### Development principles

- Deny access by default and bind authorization to the customer, device and operation.
- Administrative installation alone does not authorize remote access.
- Keep credentials, keystrokes and clipboard contents out of logs.
- Changes to security boundaries require negative authorization tests and review of the affected requirements.
- Production pilots require independent security review, recovery testing and resolution of critical and high-risk findings.
