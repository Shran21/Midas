# Csomagolás / Packaging

[English](#english)

## Magyar

A Debian- és Ubuntu-csomag a [cargo-deb](https://github.com/kornelski/cargo-deb), a Rocky Linux-csomag a [cargo-generate-rpm](https://github.com/cat-in-136/cargo-generate-rpm) eszközzel készül. A beállítások az `agent/agent-linux/Cargo.toml` `package.metadata` szakaszában vannak. A program `/usr/bin/midas-agent` néven, a systemd-unit `/usr/lib/systemd/system/midas-agent.service` néven települ.

Linuxon, a repó gyökeréből:

```bash
cargo install --locked cargo-deb cargo-generate-rpm
cargo build --release --locked -p midas-agent-linux
cargo deb --no-build -p midas-agent-linux
cargo generate-rpm -p agent/agent-linux
```

A csomagok a `target/debian/` és a `target/generate-rpm/` mappába kerülnek. A telepítés utáni szkript létrehozza a `midas-agent` rendszerfelhasználót, az `/etc/midas-agent` (root:midas-agent, 0750) és a `/var/lib/midas-agent` (midas-agent, 0700) mappát, majd engedélyezi a szolgáltatást. A szolgáltatás csak akkor indul, ha a `config.json` már ott van.

## English

The Debian and Ubuntu package is built with [cargo-deb](https://github.com/kornelski/cargo-deb), the Rocky Linux package with [cargo-generate-rpm](https://github.com/cat-in-136/cargo-generate-rpm). Settings are in the `package.metadata` sections of `agent/agent-linux/Cargo.toml`. The program installs as `/usr/bin/midas-agent` and the systemd unit as `/usr/lib/systemd/system/midas-agent.service`.

On Linux, from the repository root:

```bash
cargo install --locked cargo-deb cargo-generate-rpm
cargo build --release --locked -p midas-agent-linux
cargo deb --no-build -p midas-agent-linux
cargo generate-rpm -p agent/agent-linux
```

Packages land in `target/debian/` and `target/generate-rpm/`. The post-install script creates the `midas-agent` system user, `/etc/midas-agent` (root:midas-agent, 0750) and `/var/lib/midas-agent` (midas-agent, 0700), and enables the service. The service starts only once `config.json` is in place.
