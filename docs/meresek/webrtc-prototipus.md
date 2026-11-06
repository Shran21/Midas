# WebRTC- és kódoló-prototípus mérése / WebRTC and encoder proof measurements

[English](#english)

## Magyar

2026. november 6. · MIDAS-017 · Mérési jegyzőkönyv

### Cél és környezet

A próba azt vizsgálta, hogy egy natív Rust-komponens valódi Windows-asztalt tud-e WebRTC-n böngészőbe küldeni. Ellenőrizte a hálózati mérések elérhetőségét, a bitráta, felbontás és képkockaszám futás közbeni állíthatóságát, valamint az adaptáció reakcióját romló és javuló hálózaton.

- Gép: Windows 11 Pro, egy monitor (1920×1080), a felhasználó **RDP-munkamenetében** futtatva.
- Prototípus: `agent/agent-windows/media-prototype`. Rögzítés DXGI Desktop Duplicationnel, Windows Graphics Capture tartalékkal. Legközelebbi szomszédos kicsinyítés, OpenH264 szoftveres kódolás forrásból fordítva (ScreenContentRealTime, alacsony bonyolultság, 4 szál), str0m 0.25 WebRTC tisztán Rust kriptográfiával, vezérlő adatcsatorna.
- Böngésző: a Claude asztali alkalmazás beépített Chromium-panelje, `RTCPeerConnection` és `getStats()`.
- Hálózat: loopback (127.0.0.1). A romlást a küldő oldali emulátor adja a médiairányban: sávszélesség-korlát, véletlen csomagvesztés, hozzáadott késleltetés és 250 ms-os, véges várakozási sor.
- Nyers napló: `.local/media-prototype/trial-2026-11-06.jsonl` (helyi). Fázisösszesítő: [webrtc-2026-11-06.json](webrtc-2026-11-06.json), amely az `agent/agent-windows/media-prototype/summarize.py` szkripttel újra előállítható.

### Eredmények

**Valódi asztal a böngészőben:** igen. A böngésző 1920×1080-as, 1440×810-es és 960×540-es valódi asztalképet jelenített meg. Az RDP-munkamenetben az első DXGI-indítás `AccessDenied` hibával elutasított, ekkor a Windows Graphics Capture tartalék lépett be. A második indítás már DXGI-vel működött.

**Mérések elérhetősége:** a szerver oldalon a TWCC-alapú sávszélesség-becslés, a vevői jelentés szerinti veszteség, az RTT, az emulátorsor hossza, az eldobások, a kódolási idő és a küldött bitráta/fps elérhető. A böngészőben a `getStats()` megadja a képkockaszámot, a felbontást, a bitrátát, a jittert, a jitterpuffert, a csomagvesztést, a fagyásokat és a kapcsolat RTT-jét. A mérések a vezérlő adatcsatornán félmásodpercenként cserélődnek.

**Futás közbeni állítás:** kézi beállításra (600 kbit/s, 0,5-es méret, 15 fps) a böngésző 6 másodpercen belül 960×540-es, 15 fps-es képet kapott. A paramétercsere új kódolómunkamenetet és kulcsképkockát indít.

**Bontás:** a Bontás gomb után a szerveroldali munkamenet 0,3 s-on belül lezárult, a rögzítés leállt. A böngésző bezárásakor a vezérlőcsatorna lezárása is bontásnak számít.

**Adaptáció (95 s, automatikus mód):**

| Fázis | Emulált hálózat | Első reakció | Célbitráta (kbit/s) eleje → vége | Kliens fps | Kliens kbit/s | RTT / jitter (ms) | Fagyás | Emulátor eldobás (veszteség/sor) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | korlátlan | 0,3 s (emelés) | 471 → 2115 | 17,3 | 330 | 1 / 12 | 1 | 0 / 0 |
| 1 | 1,5 Mbit/s, 1%, 60 ms | **0,5 s** (csökkentés) | 2115 → 150 (1,5 s alatt) | 7,3 | 282 | 62 / 53 | 6 | 15 / 0 |
| 2 | 500 kbit/s, 5%, 150 ms | azonnal | 174 → 348, min. 150 | 8,3 | 242 | 155 / 82 | 9 | 49 / 9 |
| 3 | 1,5 Mbit/s, 1%, 60 ms | azonnal | 311 → 208 | 10,2 | 187 | 81 / 35 | 5 | 8 / 0 |
| 4 | korlátlan | 3,5 s (emelés) | 208 → 1932 (~22 s) | 16,0 | 283 | 3 / 15 | 4 | 0 / 0 |

Kódolási idő: 1920×1080-on kb. 127 ms képkockánként (legfeljebb kb. 8 fps), 1440×810-en kb. 39 ms, 960×540-en 22–31 ms.

### Megállapítások

1. **A str0m-integráció működik.** Külső kódoló, adatcsatorna, kulcsképkocka-kérés, TWCC-becslés, vevői statisztika és bontás egyaránt elérhető. A sans-IO felépítés miatt a UDP-utat a MIDAS kezeli; ez tette lehetővé az emulátort, és később a TURN-, a QoS- és a sorhatárkezelést is ide lehet illeszteni.
2. **A szoftveres OpenH264 forrásból, NASM nélkül túl lassú 1080p-hez.** A szoftveres útnak SIMD-del fordított vagy a Cisco által terjesztett binárisra van szüksége; ennek terjesztési feltételeit a MIDAS-046 ellenőrzi. Nagy felbontáshoz és mozgáshoz a hardveres kódolók (MIDAS-043–045) szükségesek.
3. **Statikus képernyőnél a tényleges bitráta jóval a cél alatt marad** (200–330 kbit/s), és a str0m becslése ilyenkor a valós kapacitás alá csökken. A vezérlő a romló fázisban gyorsan, 0,5 s alatt reagált, de a becslés követése miatt emelés és csökkentés váltakozott. A közös adaptív vezérlőhöz (MIDAS-046) ezért szükséges: csökkentés utáni tartási idő, veszteség- és késleltetésalapú becslés kombinálása, próbálás alacsony forgalomnál, valamint emelés csak a ténylegesen átvitt bitrátához mérten.
4. **Az RDP-munkamenet rögzítése nem megbízható DXGI-vel.** Átmeneti `AccessDenied` lépett fel; a WGC-tartalék és az automatikus újrapróbálás szükséges. A konzol- és RDP-esetet a MIDAS-042 és a MIDAS-058 mátrixa külön méri.
5. **A visszaállás fokozatos és oszcillációmentes** korlátlan hálózaton (2 s-onként +25%, kb. 22 s alatt). Hiányzó mérés nem emel minőséget.

### Döntés

A natív komponens WebRTC-integrációja a **str0m** könyvtárra épül. Indoklás: tisztán Rust, kis függőségi felület, sans-IO felépítés a saját UDP-, TURN- és sorkezeléshez, külső kódoló és adatcsatorna közvetlen támogatása, valamint beépített TWCC-becslés.

A **webrtc-rs** és a **libwebrtc** nem került méréses próbára. A libwebrtc C++-os buildje (GN/Ninja) és FFI-je több órás, platformonkénti építést igényel. A webrtc-rs aszinkron, „mindent tartalmazó” felülete kevesebb kontrollt ad a hálózati út felett. A libwebrtc érett sávszélesség-becslése és kódolóintegrációja tartalék lehetőség marad. Ha a MIDAS-058 valódi hálózati mátrixában a str0m becslése a MIDAS-046 javításai után sem teljesíti a késleltetési és stabilitási feltételeket, a döntést ott kell újranyitni. A 0. fázis zárásakor (MIDAS-020) a projektgazda felülvizsgálja ezt a döntést.

### Korlátok

- Egy gép, loopback, egyirányú, küldő oldali emuláció; valódi hálózat, NAT és TURN nincs benne.
- Csak szoftveres OpenH264, NASM-optimalizálás nélkül; nincs hardveres kódoló.
- RDP-munkamenet, egy monitor, a beépített Chromium-panel. Más böngészők és a konzolmunkamenet a MIDAS-058 része.
- A prototípus csak loopbackon figyel, nincs benne hitelesítés, helyi beleegyezés, audit vagy UAC; ezek a későbbi feladatok részei (MIDAS-018, MIDAS-047–051).

### Újrafuttatás

```text
cargo run --release --locked -p midas-media-prototype
```

Ezután nyisd meg a http://127.0.0.1:7700 címet, és nyomd meg a Kapcsolódás, majd a Mérési próba gombot. A napló a `.local/media-prototype` mappába kerül (`MIDAS_MEASUREMENTS_DIR`). Összesítés: `python agent/agent-windows/media-prototype/summarize.py <napló>`.

## English

6 November 2026 · MIDAS-017 · Measurement record

### Goal and environment

The trial examined whether a native Rust component can send the real Windows desktop to a browser over WebRTC. It checked the availability of network measurements, runtime adjustment of bitrate, resolution and frame rate, and the adaptation reaction on degrading and recovering networks.

- Machine: Windows 11 Pro, one monitor (1920×1080), running in the user's **RDP session**.
- Prototype: `agent/agent-windows/media-prototype`. Capture via DXGI Desktop Duplication with a Windows Graphics Capture fallback. Nearest-neighbour downscale, OpenH264 software encoding built from source (ScreenContentRealTime, low complexity, 4 threads), str0m 0.25 WebRTC with pure-Rust cryptography, and a control data channel.
- Browser: the Chromium pane built into the Claude desktop app, using `RTCPeerConnection` and `getStats()`.
- Network: loopback (127.0.0.1). The impairment comes from a sender-side emulator in the media direction: bandwidth cap, random packet loss, added delay and a bounded 250 ms queue.
- Raw log: `.local/media-prototype/trial-2026-11-06.jsonl` (local only). Phase summary: [webrtc-2026-11-06.json](webrtc-2026-11-06.json), reproducible with `agent/agent-windows/media-prototype/summarize.py`.

### Results

**Real desktop in the browser:** yes. The browser displayed the real desktop at 1920×1080, 1440×810 and 960×540. In the RDP session the first DXGI start was refused with `AccessDenied`, so the Windows Graphics Capture fallback took over. The second start worked with DXGI.

**Measurement availability:** on the server side, the TWCC bandwidth estimate, receiver-report loss, RTT, emulator queue length, drops, encode time and sent bitrate/fps are available. In the browser, `getStats()` reports frame rate, resolution, bitrate, jitter, jitter buffer, packet loss, freezes and connection RTT. Both sides exchange measurements every half second over the control data channel.

**Runtime adjustment:** after a manual setting (600 kbit/s, scale 0.5, 15 fps) the browser received 960×540 at 15 fps within 6 seconds. A parameter change starts a new encoder session with a keyframe.

**Termination:** after the Stop button the server session closed within 0.3 s and capture stopped. When the browser closes, closing the control channel also ends the session.

**Adaptation (95 s, automatic mode):**

| Phase | Emulated network | First reaction | Target bitrate (kbit/s) start → end | Client fps | Client kbit/s | RTT / jitter (ms) | Freezes | Emulator drops (loss/queue) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | unlimited | 0.3 s (raise) | 471 → 2115 | 17.3 | 330 | 1 / 12 | 1 | 0 / 0 |
| 1 | 1.5 Mbit/s, 1%, 60 ms | **0.5 s** (lower) | 2115 → 150 (within 1.5 s) | 7.3 | 282 | 62 / 53 | 6 | 15 / 0 |
| 2 | 500 kbit/s, 5%, 150 ms | immediate | 174 → 348, min 150 | 8.3 | 242 | 155 / 82 | 9 | 49 / 9 |
| 3 | 1.5 Mbit/s, 1%, 60 ms | immediate | 311 → 208 | 10.2 | 187 | 81 / 35 | 5 | 8 / 0 |
| 4 | unlimited | 3.5 s (raise) | 208 → 1932 (~22 s) | 16.0 | 283 | 3 / 15 | 4 | 0 / 0 |

Encode time: about 127 ms per frame at 1920×1080 (at most about 8 fps), about 39 ms at 1440×810 and 22–31 ms at 960×540.

### Findings

1. **The str0m integration works.** External encoder, data channel, keyframe requests, TWCC estimate, receiver statistics and termination are all available. Because str0m is sans-IO, MIDAS owns the UDP path; that made the emulator possible, and TURN, QoS and queue limits can later be added there too.
2. **Software OpenH264 built from source without NASM is too slow for 1080p.** The software path needs a SIMD build or the Cisco-distributed binary; MIDAS-046 checks its distribution terms. High resolution and motion require the hardware encoders (MIDAS-043–045).
3. **For a mostly static screen the actual bitrate stays well below the target** (200–330 kbit/s), and the str0m estimate then drops below the real capacity. The controller reacted within 0.5 s in the degrading phase, but by following the estimate it alternated between raising and lowering. The shared adaptive controller (MIDAS-046) therefore needs a hold time after a decrease, combined loss- and delay-based estimation, probing at low traffic, and raises bounded by the actually delivered bitrate.
4. **DXGI capture is unreliable in an RDP session.** A transient `AccessDenied` occurred; the WGC fallback and automatic retry are required. MIDAS-042 and the MIDAS-058 matrix measure console and RDP separately.
5. **Recovery is gradual and free of oscillation** on an unlimited network (+25% every 2 s, about 22 s). A missing measurement never raises quality.

### Decision

The native component's WebRTC integration builds on **str0m**. Rationale: pure Rust, a small dependency surface, a sans-IO design for our own UDP, TURN and queue handling, direct support for an external encoder and data channels, and built-in TWCC estimation.

**webrtc-rs** and **libwebrtc** were not trialled with measurements. libwebrtc's C++ build (GN/Ninja) and FFI need a multi-hour, per-platform build. webrtc-rs's asynchronous, all-in-one surface gives less control over the network path. libwebrtc's mature bandwidth estimation and encoder integration remain the fallback. If str0m's estimate still misses the latency and stability conditions in the MIDAS-058 real network matrix after the MIDAS-046 improvements, the decision must be reopened there. The project owner reviews this decision at the phase 0 closure (MIDAS-020).

### Limitations

- Single machine, loopback, one-way sender-side emulation; no real network, NAT or TURN.
- Software OpenH264 only, without NASM optimisation; no hardware encoder.
- RDP session, one monitor, the built-in Chromium pane. Other browsers and the console session belong to MIDAS-058.
- The prototype listens only on loopback and has no authentication, local consent, audit or UAC; those belong to later tasks (MIDAS-018, MIDAS-047–051).

### Re-running

```text
cargo run --release --locked -p midas-media-prototype
```

Then open http://127.0.0.1:7700 and press Connect, then Run trial. The log goes to the `.local/media-prototype` folder (`MIDAS_MEASUREMENTS_DIR`). Summary: `python agent/agent-windows/media-prototype/summarize.py <log>`.
