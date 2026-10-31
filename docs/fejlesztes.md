# Fejlesztés / Development

[English](#english)

## Magyar

A jelenlegi alap egy helyi Rust-szerverből és egy React indítóoldalból áll. A szerver HTTP-t használ, kizárólag loopback címen indul, és a GET /health/live végpontot szolgálja ki. Az állapotválasz a folyamat működését jelzi. A TLS, az adatbázis, a bejelentkezés és az agentkapcsolat további feladatok.

### Eszközök

- Rust 1.99.1, rustfmt és Clippy; a rust-toolchain.toml rögzíti őket.
- Node.js 24.22.2 és pnpm 12.16.1.
- just 1.59.0.
- Windowson a Visual Studio C++ buildeszközei és Windows SDK; Linuxon C/C++ linker és fordítóeszközök.

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
| just lint | Rust formázás, Clippy és frontend-ellenőrzés |
| just fmt | Rust- és frontend-formázás |
| just release | Optimalizált Rust-bináris és frontend build |

A szerver binárisa a target/debug vagy target/release könyvtárba, a frontend az apps/dashboard/dist könyvtárba kerül. Ezek fejlesztői buildek; az aláírt terjesztési csomagok későbbi feladatok.

A szerver külön a cargo run --locked -p midas-server paranccsal is indítható. A --bind vagy MIDAS_BIND csak loopback címet fogad el; alapértéke 127.0.0.1:7443. A RUST_LOG szabályozza a strukturált JSON-napló részletességét.

### Függőségek és ellenőrzés

A Cargo.lock és pnpm-lock.yaml verziókövetett. A közös parancsok és a CI rögzített feloldást használnak. A pnpm nem futtat függőségi telepítőszkripteket, és legalább egynapos csomagkiadásokat enged.

Új környezetben a just setup, just lint, just test és just release sorrend igazolja a teljes alapot. A CI Windows és Linux gépen, friss klónból futtatja ugyanezeket. A Windows UAC, GPU és későbbi agentfunkciók próbái külön célgépet igényelnek.

## English

The current foundation consists of a local Rust server and a React launch page. The server uses HTTP, binds only to loopback addresses and exposes GET /health/live. Its response indicates process liveness. TLS, database access, sign-in and agent connections are subsequent tasks.

### Tools

- Rust 1.99.1, rustfmt and Clippy, pinned in rust-toolchain.toml.
- Node.js 24.22.2 and pnpm 12.16.1.
- just 1.59.0.
- Windows: Visual Studio C++ build tools and Windows SDK. Linux: a C/C++ linker and build tools.

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
| just lint | Rust formatting, Clippy and frontend checks |
| just fmt | Format Rust and frontend sources |
| just release | Optimized Rust binary and frontend build |

Server binaries go to target/debug or target/release; frontend output goes to apps/dashboard/dist. These are development builds. Signed distribution packages are later work.

Run the server separately with cargo run --locked -p midas-server. The --bind option or MIDAS_BIND accepts only loopback addresses and defaults to 127.0.0.1:7443. RUST_LOG controls structured JSON logging.

### Dependencies and verification

Commit Cargo.lock and pnpm-lock.yaml. Shared commands and CI use locked dependency resolution. pnpm does not execute dependency installation scripts and requires package releases to be at least one day old.

On a fresh checkout, run just setup, just lint, just test and just release to verify the foundation. CI runs the same commands on fresh Windows and Linux checkouts. Windows UAC, GPU and subsequent agent features require separate target-machine testing.
