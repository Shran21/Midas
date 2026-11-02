# Fejlesztési ellenőrzések / Development checks

[English](#english)

## Magyar

A `Foundations` workflow friss Windows- és Linux-környezetben futtatja a formázást, lintet, teszteket és kiadási buildet. A `Security` workflow a Git-előzményt, valamint a Rust- és frontend-függőségeket vizsgálja. Mindkettő fut pull requestnél és a `main`, illetve `implementation/**` ágakra feltöltéskor. A biztonsági ellenőrzés hetente és kézzel is indítható, hogy az új hibajelentések kódváltozás nélkül is kiderüljenek.

### Vizsgálatok

- A Gitleaks az aktuális commit teljes előzményét ellenőrzi. A találatok titkos értékeit kitakarja. Külön próba igazolja, hogy az ártalmatlan bemenetet elfogadja, egy mesterséges hitelesítő adatot pedig elutasítja.
- A `cargo audit` a RustSec adatbázisával ellenőrzi a `Cargo.lock` csomagjait; a figyelmeztetések is hibát okoznak. A `cargo deny` az ismert hibák mellett a licenceket, a csomagforrásokat és a függőségi szabályokat vizsgálja a `deny.toml` alapján.
- A `pnpm audit --audit-level low` a fejlesztői csomagokat is ellenőrzi. Nincs kihagyott biztonsági bejelentés vagy figyelmen kívül hagyott hálózati hiba. A `concurrently` közvetett `shell-quote` csomagja javított, 1.12.0 verzióra rögzített.

Az eszközök verziója és archívumának SHA-256 összege a telepítőszkriptben rögzített. A workflow-k csak olvasási jogosultságot kapnak; a checkout nem tárol hitelesítő adatot, és a külső műveletek commitra rögzítettek. Függőségfrissítéshez külön felülvizsgált változtatás kell.

### Helyi futtatás

Windows/Linux x86_64 rendszeren a projekt gyökeréből:

```text
python .github/scripts/install-security-tools.py --destination .local/tools/security
```

A kiírt mappát add az aktuális terminál `PATH` változójához, majd futtasd a `just security` parancsot. Rust, Git, Python, Node.js és pnpm is szükséges; a [fejlesztői útmutató](fejlesztes.md) rögzíti a verziókat. Az ellenőrzés hálózatot igényel. A helyi Git-vizsgálat a commitolt előzményt nézi; a még nem commitolt munkát nem. Az új commitot a távoli CI ismét ellenőrzi.

### A main ág védelme

A main ágra csak olyan commit tölthető, amelynél sikeres a `Verify (ubuntu-24.04)`, `Verify (windows-2022)` és `Security` ellenőrzés. A kötelező ellenőrzések a GitHub Actions alkalmazáshoz kötöttek. Az ágnak naprakésznek kell lennie; a védelem az adminisztrátorra is érvényes. A force push és az ágtörlés tiltott. Pull request nem kötelező: az ellenőrzött commit közvetlenül feltölthető.

Ezek fejlesztési ellenőrzések. Nem helyettesítik a jogosultsági és ügyfélszigetelési teszteket, a valódi UAC-próbát vagy az éles pilot előtti független biztonsági felülvizsgálatot.

## English

The `Foundations` workflow runs formatting, lint, tests and release builds on fresh Windows and Linux runners. `Security` checks Git history and Rust/frontend dependencies. Both run on pull requests and pushes to `main` or `implementation/**`. Security checks also run weekly and on demand to detect new advisories without requiring source changes.

### Checks

- Gitleaks scans the current commit's full history and redacts secret values. A separate check verifies that harmless input passes and a synthetic credential is rejected.
- `cargo audit` checks `Cargo.lock` against RustSec; warnings also fail the check. `cargo deny` checks advisories, licenses, sources and dependency rules using `deny.toml`.
- `pnpm audit --audit-level low` includes development dependencies. No advisories or registry failures are ignored. The `concurrently` transitive `shell-quote` dependency is pinned to the patched 1.12.0 release.

Tool versions and archive SHA-256 hashes are pinned in the installer. Workflows have read-only permissions, checkout does not retain credentials, and external actions are pinned to commits. Dependency updates require a separately reviewed change.

### Local execution

On Windows/Linux x86_64, from the repository root:

```text
python .github/scripts/install-security-tools.py --destination .local/tools/security
```

Add the printed directory to the current terminal's `PATH`, then run `just security`. Rust, Git, Python, Node.js and pnpm are also required; see the versions in the [development guide](fejlesztes.md#english). Checks require network access. Local Git scanning covers committed history, not uncommitted work. Remote CI scans new commits again.

### Main branch protection

Only commits with successful `Verify (ubuntu-24.04)`, `Verify (windows-2022)` and `Security` checks may be pushed to main. Required checks are bound to the GitHub Actions application. The branch must be up to date, and protection also applies to administrators. Force pushes and branch deletion are prohibited. Pull requests are optional: a verified commit may be pushed directly.

These are development checks. They do not replace authorization and tenant-isolation tests, real UAC trials or independent security review before production pilot.
