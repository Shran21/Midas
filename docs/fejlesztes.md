# Fejlesztés / Development

[English](#english)

## Magyar

A jelenlegi alap egy helyi Rust-szerverből és egy React üzemeltetői előnézetből áll. A fejlesztői szerver HTTP-t használ, kizárólag loopback címen indul, és a GET /health/live végpontot szolgálja ki. Az állapotválasz a folyamat működését jelzi. A HTTPS/mTLS és ACME indítás a [szerver útmutatójában](szerver.md), a PostgreSQL-séma, migráció és ügyfélszigetelés az [adatbázis útmutatójában](adatbazis.md) szerepel. A bejelentkezés és az agentkapcsolat további feladatok.

### Mappaszerkezet

A `server/` a Rust-szerver, a `web-panel/` a webes felület forrása. Az `agent/agent-windows/` és `agent/agent-linux/` jelenleg a platformagentek tervét tartalmazza; az agent még nem futtatható. A Cargo- és pnpm-workspace gyökere közös marad.

A webpanel keresése, szűrése, kártyás és listás nézete, eszközadatlapja, témaváltása és nyelvváltása helyben működik. A bemutatóadatok a `web-panel/src/data.ts` fájlban vannak; a kapcsolatellenőrzés kizárólag a helyi szerver állapotát kéri le. A témát és nyelvet a böngésző tárolja. Nincs automatikus hálózati lekérdezési ciklus.

### Eszközök

- Rust 1.99.1, rustfmt és Clippy; a rust-toolchain.toml rögzíti őket.
- Node.js 24.22.2 és pnpm 12.16.1.
- just 1.59.0.
- Windowson a Visual Studio C++ buildeszközei és Windows SDK; Linuxon C/C++ linker és fordítóeszközök.
- Natív Perl és make/nmake az OpenSSL forrásból fordításához. Windowson a `python .github/scripts/install-perl.py` ellenőrzőösszeghez kötött hordozható Perlt készít elő a projekt `.local/tools` mappájában. Ezután PowerShellben: `$env:OPENSSL_SRC_PERL = "$PWD\.local\tools\perl-5.42.3.1\perl\bin\perl.exe"`. Linuxon a rendszer Perl- és make-csomagja használható.

A Windows MSVC-környezethez a [Microsoft leírása](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup) ad telepítési lépéseket. A just telepítése a [hivatalos útmutatóban](https://github.com/casey/just#installation) szerepel.

### Indítás

```sh
git clone https://github.com/Shran21/Midas.git
cd Midas
npm install --global pnpm@12.16.1
cargo install just --version 1.59.0 --locked
just setup
just dev
```

A fenti telepítési parancsot PowerShellben `npm.cmd install --global pnpm@12.16.1` alakban használd. A just Windows alatt a parancsfájlokat közvetlenül indítja, így nem igényel PowerShell-végrehajtási szabályzatmódosítást.

A felület a http://127.0.0.1:5173 címen érhető el. A Vite a /health kéréseket a http://127.0.0.1:7443 szerverhez továbbítja. Ctrl+C mindkét fejlesztői folyamatot leállítja.

### Közös parancsok

| Parancs | Eredmény |
| --- | --- |
| just setup | Függőségek telepítése a pnpm-lock.yaml alapján |
| just dev | Helyi szerver és frontend együtt |
| just build | Rust debug build és frontend build |
| just test | Rust-tesztek és TypeScript-típusellenőrzés |
| just test-database | PostgreSQL ügyfélszigetelési és hitelesítési próbák külön tesztklaszteren |
| just lint | Rust formázás, Clippy és frontend-ellenőrzés |
| just fmt | Rust- és frontend-formázás |
| just release | Optimalizált Rust-bináris és frontend build |
| just security | Függőség- és Git-előzmény-ellenőrzés, külön telepített biztonsági eszközökkel |

A szerver binárisa a target/debug vagy target/release könyvtárba, a frontend a web-panel/dist könyvtárba kerül. Ezek fejlesztői buildek; az aláírt terjesztési csomagok későbbi feladatok.

A szerver külön a cargo run --locked -p midas-server paranccsal is indítható. A --bind vagy MIDAS_BIND csak loopback címet fogad el; alapértéke 127.0.0.1:7443. A RUST_LOG szabályozza a strukturált JSON-napló részletességét.

### Függőségek és ellenőrzés

A Cargo.lock és pnpm-lock.yaml verziókövetett. A közös parancsok és a CI rögzített feloldást használnak. A pnpm nem futtat függőségi telepítőszkripteket, és legalább egynapos csomagkiadásokat enged.

A passkeyréteg a rögzített `webauthn-rs 0.5.5` verziót használja, a Cargo.lockban rögzített OpenSSL-forrás statikus fordításával. A TLS továbbra is rustls-alapú. Az első Rust-build emiatt hosszabb; a futtatáshoz külön OpenSSL-telepítés nem kell. [Hitelesítési réteg és integrációs feltételek](hitelesites.md).

Új környezetben a just setup, just lint, just test és just release sorrend igazolja a teljes alapot. A CI Windows és Linux gépen, friss klónból futtatja ugyanezeket. A Windows UAC, GPU és későbbi agentfunkciók próbái külön célgépet igényelnek.

[CI és biztonsági ellenőrzések](ci.md)

## English

The current foundation consists of a local Rust server and a React operator preview. The development server uses HTTP, binds only to loopback addresses and exposes GET /health/live. Its response indicates process liveness. See the [server guide](szerver.md#english) for HTTPS/mTLS and ACME startup and the [database guide](adatbazis.md#english) for PostgreSQL schema, migrations and tenant isolation. Sign-in and agent connections are subsequent tasks.

### Directory layout

`server/` contains the Rust server and `web-panel/` the web interface. `agent/agent-windows/` and `agent/agent-linux/` currently document the planned platform agents; there is no runnable agent yet. Cargo and pnpm share the repository root.

Search, filters, cards/list views, device details, themes and languages work locally. Sample data lives in `web-panel/src/data.ts`; the connection check only queries the local server. The browser stores theme and language preferences. There is no automatic network polling loop.

### Tools

- Rust 1.99.1, rustfmt and Clippy, pinned in rust-toolchain.toml.
- Node.js 24.22.2 and pnpm 12.16.1.
- just 1.59.0.
- Windows: Visual Studio C++ build tools and Windows SDK. Linux: a C/C++ linker and build tools.
- Native Perl and make/nmake for building OpenSSL from source. On Windows, run `python .github/scripts/install-perl.py` and set `$env:OPENSSL_SRC_PERL = "$PWD\.local\tools\perl-5.42.3.1\perl\bin\perl.exe"` in PowerShell. The checksum-pinned portable tool stays inside `.local/tools`. Linux can use system Perl/make packages.

Follow [Microsoft's guide](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup) for the Windows MSVC prerequisites and the [official instructions](https://github.com/casey/just#installation) for just.

### Start

```sh
git clone https://github.com/Shran21/Midas.git
cd Midas
npm install --global pnpm@12.16.1
cargo install just --version 1.59.0 --locked
just setup
just dev
```

In PowerShell, use `npm.cmd install --global pnpm@12.16.1` for the package manager installation above. On Windows, just invokes command files directly without requiring a PowerShell execution-policy change.

Open http://127.0.0.1:5173. Vite forwards /health requests to http://127.0.0.1:7443. Ctrl+C stops both development processes.

### Shared commands

| Command | Result |
| --- | --- |
| just setup | Install dependencies from pnpm-lock.yaml |
| just dev | Run the local server and frontend together |
| just build | Rust debug and frontend builds |
| just test | Rust tests and TypeScript type checking |
| just test-database | PostgreSQL tenant isolation and authentication against a separate test cluster |
| just lint | Rust formatting, Clippy and frontend checks |
| just fmt | Format Rust and frontend sources |
| just release | Optimized Rust binary and frontend build |
| just security | Dependency and Git-history checks, with separately installed security tools |

Server binaries go to target/debug or target/release; frontend output goes to web-panel/dist. These are development builds. Signed distribution packages are later work.

Run the server separately with cargo run --locked -p midas-server. The --bind option or MIDAS_BIND accepts only loopback addresses and defaults to 127.0.0.1:7443. RUST_LOG controls structured JSON logging.

### Dependencies and verification

Commit Cargo.lock and pnpm-lock.yaml. Shared commands and CI use locked dependency resolution. pnpm does not execute dependency installation scripts and requires package releases to be at least one day old.

On a fresh checkout, run just setup, just lint, just test and just release to verify the foundation. CI runs the same commands on fresh Windows and Linux checkouts. Windows UAC, GPU and subsequent agent features require separate target-machine testing.

[CI and security checks](ci.md#english)

The passkey layer pins `webauthn-rs 0.5.5` and statically builds the OpenSSL source pinned in Cargo.lock. TLS still uses rustls. Initial Rust builds take longer; runtime needs no separate OpenSSL installation. See the [authentication integration requirements](hitelesites.md#english).
