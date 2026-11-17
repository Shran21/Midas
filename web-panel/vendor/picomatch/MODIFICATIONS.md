# Helyi módosítás / Local modification

## Magyar

A `picomatch` 4.0.7 npm-csomag helyi másolata, az eredeti MIT-licenccel és szerzői adatokkal. Az `upstream.json` rögzíti az eredeti archívum integritását és a fájlok hashét. 2026. november 17-én a `GHSA-fc5q-39xc-vjpf` hibához még nincs javított npm-kiadás.

A `lib/parse.js` a `+(...)` és `*(...)` ismétlődő extglob mintákat a reguláris kifejezés létrehozása előtt elutasítja. A tiltás a rekurzióellenőrzés kikapcsolásával sem kerülhető meg. Ezeket a mintákat a MIDAS buildje nem használja; ha későbbi konfiguráció mégis bevezetné, egyértelmű hibát kap. A normál globok és a nem ismétlődő extglobok működnek tovább. A `package.json` publikálásra tiltott, a csomag saját fejlesztői parancsai és függőségei kimaradtak. A többi upstream fájl változatlan.

A pnpm override a Vite, tinyglobby és fdir összes érintett függőségét erre a másolatra irányítja. A Security CI a szokásos audit mellett ellenőrzi az összes tényleges feloldási útvonalat, a telepített és helyi forrás egyezését, a változatlan fájlok hashét, a hibás minták elutasítását és a buildhez használt mintákat. Helyi fájlfüggőséget az npm-advisory adatbázis nem minősít javított kiadásnak: a helyi javítás ellenőrzése ezeknek a teszteknek a feladata. Kitalált javított verzió és globális auditkizárás nincs. A külön upstream-audit az eredeti npm-verzióhoz tartozó minden figyelmeztetést lekéri: kizárólag a fenti, helyi forrás- és regressziós tesztekkel igazolt javítást fogadja el, minden új jelzés vagy hibás válasz leállítja az ellenőrzést. Upstream javított kiadásra áttéréskor a másolat és override eltávolítandó, azonos regressziós ellenőrzéssel.

## English

Local copy of npm `picomatch` 4.0.7, retaining its MIT license and author metadata. `upstream.json` records original archive integrity and file hashes. No patched npm release exists for `GHSA-fc5q-39xc-vjpf` as of November 17, 2026.

`lib/parse.js` rejects repeated `+(...)` and `*(...)` extglobs before regular-expression construction, including when recursion checking is disabled. MIDAS build patterns do not use this syntax; introducing it later produces an explicit error. Ordinary globs and nonrepeating extglobs remain supported. `package.json` disables publication and omits upstream development scripts/dependencies. Other upstream files are unchanged.

The pnpm override redirects every affected Vite, tinyglobby and fdir dependency to this copy. Alongside the normal audit, Security CI checks all actual resolution paths, installed/local source equality, unchanged-file hashes, rejection of affected patterns and supported build patterns. The npm advisory database does not classify local file dependencies as patched releases: these regression checks validate the local modification. There is no fictitious fixed version or blanket audit exclusion. A separate upstream audit fetches every advisory for the original npm version: only the fix above is accepted after local source/regression checks; any new advisory or malformed response fails the check. Replace the copy and override when a fixed upstream release is available, retaining equivalent regression coverage.

[Upstream](https://github.com/micromatch/picomatch) · [Advisory](https://github.com/advisories/GHSA-fc5q-39xc-vjpf).
