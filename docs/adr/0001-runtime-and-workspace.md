# 0001 — Futtatókörnyezet és monorepó / Runtime and monorepo

2026-10-31 · Elfogadva / Accepted

## Magyar

A szerver és az agent Rustban készül, közös Cargo-workspace-ben. A dashboard React és TypeScript, pnpm-workspace-ben. Az asztali felülethez Tauri 2-t használunk.

A Rust típus- és tulajdonosi modellje segíti a hosszú ideig futó szolgáltatások és a privilegizált komponensek fejlesztését. A Windows API-khoz szükséges nem biztonságos műveletek később kis, külön felülvizsgálható modulokba kerülnek. Az új crate-ekben alapértelmezésben tiltott az unsafe kód.

A Tauri felülete a felhasználói munkamenetben fut. A rendszerjogú szolgáltatás és az UAC-komponens külön folyamat; a megjelenített ablak nem válik rendszergazdai felületté. A tényleges memória- és processzorigényt Windows 11-en a WebView2 folyamatokkal együtt mérjük.

A fordító és a csomagkezelő verziója rögzített. A Cargo.lock és a pnpm-lock.yaml a repó része. A Windows MSVC és a Linux natív build ugyanazokat a közös parancsokat használja. A platformfüggő képességek elfogadása a célplatformon történik.

A licenc AGPL-3.0-only. A későbbi kodekek és telepítőcsomagok terjesztési feltételeit külön ellenőrizzük.

## English

The server and agent use Rust in a shared Cargo workspace. The dashboard uses React and TypeScript in a pnpm workspace. The desktop interface uses Tauri 2.

Rust's type and ownership model supports development of long-running services and privileged components. Required unsafe Windows API calls will live in small, separately reviewed modules. New crates deny unsafe code by default.

The Tauri interface runs in the user's session. The privileged service and UAC component are separate processes; displaying a window does not elevate it. Measure actual memory and CPU use on Windows 11, including WebView2 processes.

Pin the compiler and package manager. Commit Cargo.lock and pnpm-lock.yaml. Native Windows MSVC and Linux builds share the same development commands. Verify platform-specific capabilities on their target platforms.

The license is AGPL-3.0-only. Assess distribution terms for future codecs and installer components separately.
