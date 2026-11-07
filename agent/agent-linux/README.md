# Linux agent

A Linux-agent (MIDAS-024): systemd-szolgáltatás `midas-agent` rendszerfelhasználóval, jogosultságok nélkül, szigorú systemd-korlátozásokkal, Debian-, Ubuntu- és Rocky Linux-csomaggal ([csomagolás](packaging/README.md), [próbaleírás](../../docs/meresek/linux-agent.md)). A kapcsolatot, a verzióegyeztetést és az újracsatlakozást az [`agent-core`](../agent-core) adja ([agentprotokoll](../../docs/agent-protokoll.md)).

A támogatott disztribúciókon végzett telepítési próba a projektgazda saját gépén történik. Követelmények: [agent és platformok](../../docs/kovetelmenyek.md).

## English

The Linux agent (MIDAS-024): a systemd service running as the unprivileged `midas-agent` system user with strict systemd restrictions, packaged for Debian, Ubuntu and Rocky Linux ([packaging](packaging/README.md#english), [trial](../../docs/meresek/linux-agent.md#english)). Connection, version negotiation and reconnection come from [`agent-core`](../agent-core) ([agent protocol](../../docs/agent-protokoll.md#english)).

Installation trials on the supported distributions run on the project owner's own machines. See the [agent and platform requirements](../../docs/kovetelmenyek.md#english).
