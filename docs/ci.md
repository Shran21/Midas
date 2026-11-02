# Fejlesztési ellenőrzések / Development checks

[English](#english)

## Magyar

A `Foundations` workflow friss Windows- és Linux-környezetben futtatja a formázást, lintet, teszteket és kiadási buildet. A `Security` workflow a Git-előzményt, valamint a Rust- és frontend-függőségeket vizsgálja. Mindkettő fut a `main` ágra feltöltéskor; a külső pull requestek ellenőrzése is megmarad. A biztonsági ellenőrzés hetente és kézzel is indítható, hogy az új hibajelentések kódváltozás nélkül is kiderüljenek.

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

A karbantartói munka a helyi `main` ágon, külön fejlesztési ág és pull request nélkül történik. Feltöltés előtt a változáshoz illő helyi ellenőrzéseket kell elvégezni. A szokásos push után a `Verify (ubuntu-24.04)`, `Verify (windows-2022)` és `Security` ellenőrzésnek ugyanazon a commiton sikeresen le kell futnia az elfogadáshoz.

A feltöltés előtti kötelező CI-feltétel ki van kapcsolva, ezért a GitHub egy még nem ellenőrzött commitot is befogad. A távoli CI hibája esetén a módosítás nem tekinthető elfogadottnak: javító vagy visszavonó commit kell, majd új ellenőrzés. Az előzmény átírása helyett új commit őrzi a változást. Az adminisztrátorra is érvényes ágvédelem, a lineáris előzmény, a force push és a `main` törlésének tiltása megmarad. A titokellenőrzés és a push protection ettől függetlenül működik.

Ezek fejlesztési ellenőrzések. Nem helyettesítik a jogosultsági és ügyfélszigetelési teszteket, a valódi UAC-próbát vagy az éles pilot előtti független biztonsági felülvizsgálatot.

## English

The `Foundations` workflow runs formatting, lint, tests and release builds on fresh Windows and Linux runners. `Security` checks Git history and Rust/frontend dependencies. Both run on pushes to `main`; checks for external pull requests remain available. Security checks also run weekly and on demand to detect new advisories without requiring source changes.

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

Maintainers work on local `main` without separate development branches or pull requests. Run applicable local checks before pushing. After a normal push, `Verify (ubuntu-24.04)`, `Verify (windows-2022)` and `Security` must pass for that exact commit before acceptance.

Pre-push required CI checks are disabled, so GitHub can receive a commit before remote verification. A failing remote check prevents acceptance: add a fix or revert commit and verify again. Preserve history rather than rewriting it. Administrator enforcement, linear history, and prohibitions on force pushes and main deletion remain enabled. Secret scanning and push protection operate independently.

These are development checks. They do not replace authorization and tenant-isolation tests, real UAC trials or independent security review before production pilot.
