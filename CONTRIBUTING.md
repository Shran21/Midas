# Hozzájárulás / Contributing

[English](#english)

## Magyar

A MIDAS fejlesztése elindult. A környezet és az ellenőrző parancsok a [fejlesztési útmutatóban](docs/fejlesztes.md) találhatók. A fejlesztés kiindulópontja a [rendszerterv](docs/rendszerterv.md), a [működési követelmények](docs/kovetelmenyek.md) és a [feladatlista](docs/feladatlista.md).

Új feladatnál írd le a megoldandó problémát, a várt eredményt és az ellenőrizhető elfogadási feltételeket. Jelöld a függőségeket és a kapcsolódó mérföldkövet. A magyar leírást az angol változat kövesse.

A karbantartói fejlesztés közvetlenül a `main` ágon történik, külön fejlesztési ág és pull request nélkül. A commit írja le a változás célját, és hivatkozzon az issue-ra. Feltöltés előtt végezd el a változáshoz illő helyi ellenőrzéseket; dokumentumnál tartalmi ellenőrzés kell. Feltöltés után várd meg a Windows-, Linux- és biztonsági CI eredményét. Az érintett tervet és követelményeket is frissítsd. A részleteket a [CI-útmutató](docs/ci.md) tartalmazza.

Hét címkét használunk: security, server, agent, dashboard, remote-support, automation és documentation. Általában egy vagy két címke elég. A telepítők az agent vagy server, a fáziszáró ellenőrzések a documentation kategóriába tartoznak.

A projekt licence [AGPL-3.0](LICENSE). Sebezhetőséget a [privát bejelentőn](https://github.com/Shran21/Midas/security/advisories/new) jelents be.

## English

MIDAS development has started. Find environment setup and verification commands in the [development guide](docs/fejlesztes.md#english). Start with the [system plan](docs/rendszerterv.md#english), [operating requirements](docs/kovetelmenyek.md#english) and [task list](docs/feladatlista.md#english).

For new work, describe the problem, expected outcome and verifiable acceptance criteria. Identify dependencies and the relevant milestone. Put Hungarian descriptions before English.

Maintainer development happens directly on `main`, without separate development branches or pull requests. Explain the change in its commit and reference the issue. Run applicable local checks before pushing; documentation needs content review. After pushing, wait for Windows, Linux and security CI results. Update affected plans and requirements. See the [CI guide](docs/ci.md#english) for details.

We use seven labels: security, server, agent, dashboard, remote-support, automation and documentation. One or two labels are usually sufficient. Installers belong under agent or server; phase closure reviews belong under documentation.

MIDAS uses [AGPL-3.0](LICENSE). Report vulnerabilities through the [private reporting form](https://github.com/Shran21/Midas/security/advisories/new).
