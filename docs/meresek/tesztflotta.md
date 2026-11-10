# Egyhetes tesztflotta / One-week test fleet

[English](#english)

## Magyar

A projektgazda 2026. november 10-i döntése szerint ez a próba a [MIDAS-086](https://github.com/Shran21/Midas/issues/87) pilotindításának kötelező előfeltétele. Még nincs elvégezve. A szimulált flotta és a 2,9 órás agentprofil nem helyettesíti.

A pilot indítása előtt tíz valós, vegyes tesztgép egyhetes jelentését kell elfogadni. A jegyzőkönyv tartalmazza:

- a gépek azonosítóit, Windows/Linux platformját, munkaállomás/szerver szerepét, rendszer- és agentverzióját;
- az időszak elejét és végét időzónával, a kiszolgáló verzióját és a jelentési szabályzatot;
- gépenként a heartbeat, metrikák, leltár és állapotellenőrzések folytonosságát, a hiányzó adatokat és a tervezett leállásokat;
- a hálózatkiesés és újraindítás utáni visszatérést, a pufferelt adatok pótlását és a riasztások viselkedését;
- a felmerült hibákat, javításukat és újraellenőrzésüket, valamint az elfogadási döntést és a bizonyítékok helyét.

A hiányzó mérést vagy kimaradó gépet láthatóan kell jelezni. Sikertelen próba esetén a pilot nem indulhat; az érintett funkciókat újra kell vizsgálni. A sikeres jegyzőkönyv a MIDAS-086 eredményéhez és a MIDAS-087 fáziszárásához tartozik. Az ügyfél egyhónapos pilotja külön, későbbi elfogadási feltétel.

## English

By the owner's November 10, 2026 decision, this trial is a mandatory prerequisite for starting the [MIDAS-086](https://github.com/Shran21/Midas/issues/87) pilot. It has not been performed. Neither the simulated fleet nor the 2.9-hour agent profile replaces it.

Before the pilot starts, accept one week of reporting from ten real, mixed test devices. Record:

- device identifiers, Windows/Linux platforms, workstation/server roles, OS and agent versions;
- start and end timestamps with timezone, server version and reporting policy;
- per-device heartbeat, metrics, inventory and health-report continuity, missing data and planned downtime;
- reconnection after network loss and reboot, buffered-data delivery and alert behavior;
- defects, fixes and retests, the acceptance decision and evidence locations.

Make missing measurements and absent devices explicit. A failed trial blocks the pilot and requires re-checking affected features. Attach the accepted record to MIDAS-086 and the MIDAS-087 closure. The customer's month-long pilot remains a separate, subsequent acceptance condition.
