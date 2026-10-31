# Agent

A Windows- és Linux-agent külön platformmappát kap. Az agent megvalósítása a következő feladatok része; jelenleg nincs telepíthető vagy futtatható agent.

- [agent-windows](agent-windows/README.md): Windows-szolgáltatás, felhasználói munkamenet és külön UAC-komponens.
- [agent-linux](agent-linux/README.md): Linux-szolgáltatás és platformintegráció.

A közös protokoll és agentlogika megosztott modulba kerül. A platformmappák az operációs rendszerhez kötött megvalósítást tartalmazzák.

## English

Windows and Linux agents have separate platform directories. Agent implementation is subsequent work; no installable or runnable agent exists yet.

- [agent-windows](agent-windows/README.md): Windows service, user session and separate UAC component.
- [agent-linux](agent-linux/README.md): Linux service and platform integration.

Protocol and common agent logic belong in shared modules. Platform directories contain operating-system-specific implementations.
