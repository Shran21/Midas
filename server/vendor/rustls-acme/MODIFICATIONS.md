# Helyi módosítás / Local modification

## Magyar

A crates.io `rustls-acme` 0.15.4 kiadásának könyvtári forrása, az eredeti MIT/Apache-2.0 licencekkel. Az `upstream.json` rögzíti az eredeti csomag ellenőrzőösszegét és fájljainak hashét. A példaprogramok és a hozzájuk tartozó fejlesztői függőségek nem részei ennek a másolatnak.

2026. november 17.: a `RUSTSEC-2026-0543` karbantartási figyelmeztetés miatt a `chrono` függőséget `time` váltja fel. Az X.509-érvényesség már eleve `time::OffsetDateTime` értéket ad; a felesleges átalakítás megszűnik. A megújítás továbbra is az élettartam kétharmadánál esedékes, elmúlt határidőnél azonnal. A TLS-ellenőrzés, az ACME-kérések, a kulcskezelés és az alapértelmezett API nem változik. Két teszt ellenőrzi az időzítési határokat és egy helyben generált tanúsítvány feldolgozását.

A `Cargo.toml` publikálásra tiltott helyi csomagot határoz meg, eltávolítja a példacélokat/fejlesztői függőségeket, és a dátumfüggőséget cseréli. A `src/state.rs` tartalmazza az időkezelési javítást és a teszteket. Az eredeti forrás többi része változatlan. Az upstream javított kiadásakor a helyi másolat és a gyökérben lévő patch eltávolítandó, azonos regressziós ellenőrzéssel. A sérülékenység-ellenőrzéshez nem készült kivétel.

## English

Library source from the crates.io `rustls-acme` 0.15.4 release, retaining its MIT/Apache-2.0 licenses. `upstream.json` records the original archive checksum and file hashes. Example applications and their development dependencies are omitted.

November 17, 2026: replace `chrono` with `time` following the `RUSTSEC-2026-0543` maintenance advisory. X.509 validity already exposes `time::OffsetDateTime`, so the redundant conversion is removed. Renewal remains scheduled at two thirds of certificate lifetime, immediately when overdue. TLS verification, ACME requests, key handling and the public API are unchanged. Two tests cover scheduling boundaries and parsing a locally generated certificate.

`Cargo.toml` disables publication, removes example targets/development dependencies and replaces the date dependency. `src/state.rs` contains the time change and tests. Other upstream source files remain unchanged. Remove this copy and the root patch when an upstream fixed release is available, retaining equivalent regression checks. No vulnerability-check exception was introduced.

Upstream: [rustls-acme](https://github.com/FlorianUekermann/rustls-acme) · [Release](https://crates.io/crates/rustls-acme/0.15.4) · [Advisory](https://rustsec.org/advisories/RUSTSEC-2026-0543.html).
