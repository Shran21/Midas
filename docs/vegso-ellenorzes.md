# Végső kézi ellenőrzés / Final manual verification

[English](#english)

## Magyar

A projektgazda 2026. november 10-i döntése alapján a kézi ellenőrzések a fejlesztés végén történnek. Az automatizált próbák közben folyamatosan futnak. Ez az összesítő nem igazolja a kézi próbák sikerét, és nem helyettesíti a feladatok saját elfogadási feltételeit a [feladatlistában](feladatlista.md).

Jelenleg nincs rögzített végső kézi elfogadás. Minden eredményhez kell az ellenőrzött commit, a környezet és verziói, a végrehajtott lépések, a tényleges eredmény és az esetleges hiba hivatkozása. Sikertelen próba után az érintett funkciót és a ráépülő működést javítás után újra kell ellenőrizni.

| Ellenőrzési kör | Még szükséges bizonyíték |
| --- | --- |
| MIDAS-018, -019, -020 | Windows 11-es UAC-próba; résztvevős és képernyőolvasós használhatósági próba; a WebRTC-döntés projektgazdai elfogadása. |
| MIDAS-023, -024 | Windows-szolgáltatás és Linux-csomag telepítési, újraindítási és eltávolítási próbája a vállalt rendszereken. |
| Windows távsegítés | Valódi szolgáltatás, konzol és több RDP-munkamenet; zárolás, leválasztás, kijelentkezés és felhasználóváltás; helyi jóváhagyás, elutasítás és megszakítás; normál/rendszergazdai mód és UAC secure desktop. A kész funkciókat kell együtt kipróbálni. |
| Hálózat és képátvitel | Valós kép és bevitel, adaptív felbontás/bitráta/képfrissítés, hálózati romlás, helyreállás és a támogatott kódolók a végső tesztmátrix szerint. |
| Tartomány | Elkülönített AD-tesztkörnyezetben felderítés, delegált beléptetés és hibakezelés. Teszttartomány jelenleg nem áll rendelkezésre. |
| Telepítés és üzemeltetés | A kész frissítési, visszaállítási, jogosultság-visszavonási és vészleállítási folyamatok, továbbá a vállalt független biztonsági felülvizsgálat eredménye. |
| MIDAS-086 és -087 | A [tíz vegyes gép egyhetes próbájának](meresek/tesztflotta.md) jegyzőkönyve a pilot indítása előtt, majd a pilot és a végső elfogadás saját bizonyítékai. |

Az automatizált tesztben használt agentválasz, böngészős API-fixture vagy szoftveres állapotszimuláció nem helyettesíti a valódi Windows-szolgáltatásos, UAC-, hardveres vagy résztvevős próbát. A fejlesztői gépen nem történik rendszerszintű telepítési próba.

## English

The owner's November 10, 2026 decision defers manual verification until the end of development. Automated checks continue throughout development. This record does not establish successful manual trials or replace each task's acceptance criteria in the [task list](feladatlista.md#english).

No final manual acceptance is recorded yet. Each result must identify the tested commit, environment and versions, steps performed, actual outcome and any linked defect. After a failed trial, re-check the affected function and downstream behavior following a fix.

| Verification area | Evidence still required |
| --- | --- |
| MIDAS-018, -019, -020 | Windows 11 UAC trial; participant and screen-reader usability trial; owner acceptance of the WebRTC decision. |
| MIDAS-023, -024 | Windows service and Linux package installation, restart and removal trials on the committed systems. |
| Windows remote support | Actual service, console and multiple RDP sessions; locking, disconnecting, signing out and switching users; local consent, refusal and termination; normal/administrative modes and UAC secure desktop. Test the completed functions together. |
| Network and media | Actual video and input, adaptive resolution/bitrate/frame rate, degraded networks, recovery and supported encoders according to the final test matrix. |
| Directory integration | Discovery, delegated domain join and failure handling in an isolated AD test environment. No test domain is currently available. |
| Deployment and operations | Completed update, restore, permission revocation and emergency-stop workflows, plus the committed independent security review. |
| MIDAS-086 and -087 | The [one-week trial with ten mixed devices](meresek/tesztflotta.md#english) before starting the pilot, followed by the pilot and final acceptance evidence. |

An automated agent response, browser API fixture or simulated state does not replace an actual Windows service, UAC, hardware or participant trial. System-level installation trials are not performed on the development machine.
