# Távsegítési képátvitel / Remote-support video

[English](#english)

## Magyar

A MIDAS-046 a jóváhagyott munkamenet képútját valósítja meg Windowson: a közös adaptív vezérlőt, a kodekegyeztetést, a színtér-átalakítást, a Windows beépített szoftveres H.264-kódolóját, a WebRTC-felet és a munkamenet-logikát, a jelzéscserét a szerveren át, a folyamatok bekötését és a böngészős nézőt. A valódi gépen, virtuális kijelzővel végzett próba a végső kézi ellenőrzés része. UAC-jog esetén ugyanebben a kapcsolatban a biztonságos asztal és a rendszergazdai ablakok is kezelhetők (MIDAS-049). Az engedély tízmásodpercenként megújul ([felhatalmazás](tavsegitesi-felhatalmazas.md)); megújítás nélkül a kép és a vezérlés legfeljebb 30 másodperc múlva leáll.

### Kodekegyeztetés

A kép H.264, 1-es csomagolási móddal (`packetization-mode=1`), így a csomagnál nagyobb képkockák darabolva mennek át. A választás a böngésző ajánlatából és a gépen talált kódolókból indul: előbb a hardveres (MIDAS-043–045), utána a szoftveres kódoló, a profilok közül pedig a constrained baseline. Ezt minden böngésző dekódolja, és nincs benne képkocka-átrendezésből adódó késés. Közös formátum nélkül nincs kép.

### Szoftveres H.264 és terjesztési feltételek

A szoftveres út a Windows része, a Media Foundation H.264-kódolója (`CLSID_MSH264EncoderMFT`). Az agent nem terjeszt kodekbinárist; a kódoló használatát a Windows licence fedi. Windows N-kiadáson a Media Feature Pack, Windows Serveren a Media Foundation szolgáltatás (`Server-Media-Foundation`) szükséges. Ennek hiányában a kódoló „nem elérhető” hibát ad, a szolgáltatás indulását viszont nem akadályozza: a program a `mfplat.dll`-t késleltetve tölti be, előtte pedig kifejezetten a System32-ből tölti be.

Az OpenH264-et nem csomagoljuk. Forrásból fordítva a H.264 szabadalmi licencét a Cisco nem fedezi, a Cisco által kiadott binárist pedig a felhasználónak közvetlenül a Cisco-tól kell letöltenie ([feltételek](https://www.openh264.org/BINARY_LICENSE.txt)). Ha később mégis szükség lesz rá, az külön döntés.

A kódoló beállításai: baseline profil, alacsony késleltetésű mód, állandó bitráta, kérésre kulcskocka, tízmásodpercenként rendszeres kulcskocka, valamint a BT.709 korlátozott tartomány jelzése a dekódolónak. A bitráta futás közben változtatható, felbontás- vagy képkockaszám-váltáshoz viszont új kódoló kell. Alacsony késleltetésű módban minden képkocka azonnal, pufferelés nélkül kódolódik. Ehhez a kódoló tulajdonságait a médiatípusok előtt kell megadni.

### Színtér-átalakítás

A rögzített BGRA képet a kódoló NV12 formátumban kapja. Kicsinyítéskor a program területátlagolást használ, hogy a szöveg olvasható maradjon. A színek BT.709 szerint, korlátozott tartományban képződnek, és a kimenet mérete páros. Az átalakítás figyelembe veszi a sorhosszt (stride), a sorvégi kitöltést nem olvassa, a rövid puffert és a hibás méretet pedig elutasítja.

### Adaptív vezérlő

A vezérlő a sávszélesség-becslésből, a körbejárási időből (RTT), a veszteségből, a jitterből és a várakozó képkockák számából bitrátát, méretarányt és képkockaszámot számol:

- Kezdéskor és mérés nélkül 1 Mbit/s a felső határ. Ha 3 másodpercig nincs becslés, a vezérlő legfeljebb erre csökkent, és sosem emel.
- A tartós veszteség (5% felett, simítva), a körbejárási idő növekedése (a legkisebb mért érték plusz 150 ms felett) és a jitter (60 ms felett) torlódásnak számít. Ilyenkor a vezérlő azonnal csökkent, tartós torlódásnál legfeljebb másodpercenként tovább. A becslés esése is azonnali csökkentést okoz.
- Emelés csak 3 másodperc nyugalom után jöhet, lépésenként legfeljebb 20%-kal, és legfeljebb az utolsó 5 másodperc legkisebb becsléséig. Ingadozó kapcsolaton így nem alakul ki fűrészfog.
- Ha a kódoló vagy a küldés lemarad, előbb a képkockaszám csökken, és később fokozatosan áll vissza.
- A szövegprofil a felbontást tartja meg, a mozgásprofil a képkockaszámot. Lépcsőváltás felfelé 15% tartalékot kíván, így a küszöb körül nem ingadozik.
- Újracsatlakozáskor a vezérlő újrakezd. A hardveres és a szoftveres kódoló korlátai (képpont, képkockaszám, bitráta) mindig érvényesek.

Az alkalmazkodás jogosultságot és jóváhagyást nem érint.

### WebRTC-fél

Az agent oldali fél a str0m könyvtárra épül, a kép DTLS-SRTP-védelemmel halad. A böngésző ajánlatára válaszol, és csak videósávot tartalmazó ajánlatot fogad el. A kodek a kapcsolat létrejöttekor választódik ki a fenti szabály szerint; közös formátum nélkül a fél nem küld képet, és a munkamenet leáll. A fél a megadott helyi címeken közvetlen jelöltet hirdet, beállított relénél relézett és szerver felől látott jelöltet is (lásd: TURN-relé). A kapcsolatkezelés a hívó szálán fut, saját UDP-csatlakozóval. A str0m sávszélesség-becslése, veszteség-, RTT- és jittermérése az adaptív vezérlő bemenete. Bontáskor, kapcsolatvesztéskor vagy hibás állapotnál a fél végleg lezárul. A WebRTC-rész csak a távsegítést kiszolgáló agentbe kerül: a Linux-agent nem tartalmazza.

### Segéd és transzport kapcsolata

A rendszerterv szerint a WebRTC-kapcsolatot külön, védett transzportfolyamat kezeli, a felhasználói segéd csak rögzít, kódol és bevitelt hajt végre. A kettő között a helyi vezérlőcsatorna viszi az üzeneteket. Ennek egy üzenete legfeljebb 64 KiB, ezért a kódolt képkocka legfeljebb 60 KiB-os darabokban halad, sorszámmal és fejléccel. A transzport a darabokat sorrendben rakja össze. Képkockánként legfeljebb 8 MiB-ot fogad el, új képkocka érkezésekor eldobja a befejezetlent, és bármilyen hibás vagy ellentmondó üzenetnél lezárja a munkamenetet, mert a segéd nem megbízható. A transzport a segédnek a kódolási beállításokat (bitráta, méretarány, képkockaszám), a kulcskockakérést és a leállítást küldi, és a segéd ezeket ellenőrzött tartományban fogadja el. Két üzenet tartozik az UAC-kezeléshez: a felengedés, amely a segéd által még lenyomva tartott billentyűket és gombokat engedi fel, és az asztalállapot (felhasználói, emelt ablak, biztonságos), amelyet csak az emelt jogú segédtől fogad el a közvetítő.

### A segéd képfolyama

A Windows-segéd a kijelölt monitorokat rögzíti, a képet a transzport beállítása szerinti méretre alakítja és kódolja, majd üzenetekre bontja. A kódoló csak új méretnél vagy képkockaszámnál épül újra, és ilyenkor kulcskockával indul; a bitráta futás közben változik. Asztalváltáskor és kérésre kulcskocka készül, kérésre az utolsó rögzített képből is. Változatlan asztalnál nem küld semmit.

### Bevitel a segédben

A segéd a transzporttól kapott bevitelt a saját asztalára injektálja (`SendInput`). A billentyűk fizikai scankódként mennek, így a karaktert a célgép saját kiosztása adja; ez a magyar ISO-billentyűzet `IntlBackslash` (í) billentyűjére is igaz. Ismeretlen billentyűnév nem jut át. A mutató a rögzített nézet koordinátáiba kerül, a gombok állapotváltozásként, a görgő Windows-egységekben. A segéd nyilvántartja a lenyomott billentyűket és gombokat: el nem engedett billentyűt nem enged fel, munkamenet végén pedig mindent felenged. Felhasználói tokennel futva a Windows (UIPI) nem engedi emelt ablakba vagy a biztonsági asztalra injektálni; ezeket csak az emelt komponens szolgálhatja ki, külön UAC-joggal.

### A képút folyamatai Windowson

A felhasználó munkamenetében futó normál segéd rögzít, kódol és hajtja végre a bevitelt. A WebRTC-transzport a párjaként induló SYSTEM-közvetítőben fut, ez a rendszerterv külön transzportidentitása. A szolgáltatás a közvetítő indítócsövét az indulás után is nyitva tartja, és ezen adja át a böngésző ajánlatát a helyi címekkel együtt; a választ ugyanott kapja vissza. A cső bezárása, a lease vége vagy bármelyik folyamat kilépése az egész csoportot leállítja. A közvetítő és a segéd közötti vezérlőcsatorna két irányba bontva, egyszerre viszi a képkockákat és a parancsokat. Minden üzenet korlátos marad, és a túloldal ellenőrzése mindkét irányban megmarad. Mivel minden olvasás legfeljebb öt másodpercig várhat, mindkét fél másodpercenként életjelet küld. A közvetítő a fel nem kapcsolt, a loopback és a link-local címek nélkül, a működő hálózati csatolók IPv4-címeit kínálja közvetlen jelöltként, és ha a szerver relét adott, a relézett címet is. A segéd a rögzítést csak az első beállítás megérkezése után kezdi el, a munkamenet végén minden lenyomott billentyűt és gombot felenged. Ha az asztal átmenetileg nem rögzíthető (például biztonságos asztal vagy kijelzőváltás miatt), a munkamenet megmarad, és a segéd fél másodpercenként újrapróbálja a rögzítést. Az indulási ellenőrzések nem változtak: a közvetlen indítás `0x22` vagy `0x23` kóddal áll le, a grafikus és médiakönyvtárak pedig csak első használatkor töltődnek be.

### Böngészős néző

Aktív kérésnél a panel megnyitja a képet. Csak fogadó videósávot és egy „control” adatcsatornát kér, a jelölteket megvárja, majd a teljes ajánlatot küldi el. A kép kijelölése után az egér, a görgő és a billentyűzet a távoli gépre megy: a mutató a kép valódi, levágott sávok nélküli területén belül számít, a billentyűk fizikai kódként mennek. A panel a profilt (szöveg vagy mozgás) választhatóvá teszi. A tényleges felbontást, a küldött bitrátát és képkockaszámot és az automatikus módot az agent másodperces jelentéséből mutatja. Ha három másodpercig nincs friss jelentés, a felület ezt kifejezetten jelzi, és nem mutat egészséges kapcsolatot.

### Munkamenet a transzportban

A transzport munkamenete összeköti a WebRTC-felet, az adaptív vezérlőt és a segéd képkockáit. Félmásodpercenként frissíti a beállításokat, és csak változáskor küldi el őket a segédnek. Másodpercenként valós adatot küld a böngészőnek: tényleges felbontást, célbitrátát és célképkockaszámot, küldött bitrátát és képkockaszámot, az automatikus módot és a profilt. A böngészőtől csak szigorúan értelmezett üzeneteket fogad el: kulcskockakérést, profilváltást (szöveg vagy mozgás), valamint mutató-, görgő- és billentyűeseményt a kép koordinátáiban (0–1). Az ismeretlen mezőt, a tartományon kívüli értéket és a 4 KiB-nál nagyobb üzenetet eldobja, a bevitelhez pedig korlátos sort használ. A bevitel a segédhez kerül; végrehajtás előtt minden eseménynél a lease ellenőrzése dönt.

### Jelzéscsere

A böngésző a teljes, jelöltekkel együtt összegyűjtött ajánlatát (legfeljebb 16 KiB) a `POST /api/tenants/{id}/devices/{device}/remote-requests/{request}/media` címre küldi, és ugyanott kapja vissza az agent válaszát. Ajánlatot csak a kérelmező küldhet, a saját, már aktív (a helyi felhasználó által engedélyezett és az agent által visszaigazolt) kéréséhez, kérésenként egyszer, és csak `remote.control` jog mellett. A szerver az ajánlatot a hitelesített agentkapcsolaton továbbítja (`MediaOffer`), a választ (`MediaAnswer`) pedig csak ugyanerre a kérésre fogadja el. Az agent az ajánlatot csak annak a kérésnek a natív hátteréhez engedi, amelynek engedélyét legutóbb elfogadta, és amely nincs visszavonva; minden más esetben üres választ ad, a kapcsolat ettől nem szakad meg. A sikeres és az elutasított jelzéscsere is auditált (`remote.media`), az SDP tartalma nem.

### UAC ugyanabban a kapcsolatban (MIDAS-049)

Ha a jóváhagyott kérés UAC-kezelést is tartalmaz, a szolgáltatás a felhasználó munkamenetében a segédpár mellett az emelt jogú segédet is elindítja, külön SYSTEM-tokenmásolattal. Ez a segéd nem a szolgáltatáshoz, hanem a munkamenet közvetítőjéhez kapcsolódik. A szolgáltatás a közvetítőnek átadja a segéd folyamatára mutató, csak lekérdezésre jogosító kezelőt és egy végpontot. A közvetítő ellenőrzi, hogy a saját munkamenetében futó SYSTEM-folyamatról van szó, és munkamenetenként csak egy ilyen segédet fogad. A felhasználói alkalmazások továbbra is a felhasználó saját jogával futnak, az általános képet és bevitelt a korlátozott segéd kezeli.

Az emelt jogú segéd tizedmásodpercenként megnézi, melyik asztal kapja a bevitelt. Ha a biztonságos asztal látszik (például UAC-kérdés vagy hitelesítő ablak), átáll rá, rögzíti és kódolja a képet, és a bevitelt is ott hajtja végre. A felhasználó asztalán nem rögzít. Ha viszont elöl vagy a mutató alatt közepesnél magasabb integritású, azaz rendszergazdai ablak van, amelyet a korlátozott segéd bevitele a Windows UIPI-védelme miatt nem érne el, a bevitelt ő kézbesíti. Minden állapotváltást az új állapot első képkockája előtt jelez a közvetítőnek.

A közvetítő ez alapján dönt. Képkockát csak a biztonságos asztalon fogad az emelt jogú segédtől, máskor csak a korlátozottól; a bevitel a biztonságos asztalon és emelt ablaknál az emelt jogú segédhez megy. Forrásváltáskor eldobja a régi forrás félbemaradt képkockáját, kulcskockát kér, és a régi úton lenyomva maradt billentyűket és gombokat felengedteti. A böngészős néző a mérés mellett jelzi, ha a távoli gépen biztonságos asztal vagy emelt ablak látszik.

A UAC, a biztonságos asztal és a rendszer házirendjei bekapcsolva maradnak, és a segéd semmit nem hagy jóvá magától. A kérdést a jogosult technikus kezeli; ahol a Windows hitelesítő adatot kér, megfelelő Windows-fiók kell. UAC-jog nélkül nem indul emelt jogú segéd: a biztonságos asztal ekkor nem látszik, és bevitel sem jut el oda. A CI elszigetelt próbája a kiadási programmal ellenőrzi, hogy az emelt jogú segéd csatlakozik a közvetítőhöz, második segéd nem csatlakozhat, és a munkamenettel együtt leáll. A valódi UAC-kérdéssel végzett próba a [végső ellenőrzés](vegso-ellenorzes.md) része.

### TURN-relé (MIDAS-052)

Ha nincs közvetlen út, a kép és a bevitel TURN-relén (eturnal) halad. A média ekkor is DTLS-SRTP-védelemmel megy végponttól végpontig, ezért a relé csak titkosított csomagokat lát. A szerver a relé közös titkából rövid életű hitelesítő adatot ad ki (felhasználónév: lejárat és a kérés azonosítója, jelszó: ennek HMAC-SHA1-e Base64-ben). Ez öt percig nyithat új foglalást; a futó foglalás frissítéséhez nem kell újra. A munkamenet végét nem a relé jelszava dönti el, hanem az agent, amely az engedélye lejártakor vagy visszavonásakor leállít, és a foglalást is visszaadja.

A böngésző a panel `GET …/remote-requests/{request}/ice` végpontjáról kapja a relé címeit és hitelesítő adatát. Ugyanaz az ellenőrzés vonatkozik rá, mint a kép indítására: csak a kérelmező, csak aktív kérésnél, a mód összes jogával kaphatja meg, és a válasz nem gyorsítótárazható. Az agent a böngésző ajánlatával együtt kap saját hitelesítő adatot. Az első UDP-s `turn:` címen foglal relézett címet a közvetítő saját csatlakozóján, és a közvetlen jelöltek mellé a relézett és a szerver felől látott címét is felkínálja. A csomagokat a relé felé TURN-üzenetekbe csomagolja, a jogosultságokat és a foglalást időben megújítja. A relé tiltott címet visszautasít, ilyen címre az agent nem is küld többet. Az agent egyelőre UDP-n éri el a relét; a TCP- és TLS-kapcsolat (443-as port) a böngésző oldalán már használható, az agent oldalán későbbi bővítés.

A relé mintabeállítása a `server/turn/eturnal.yml`. A belső, loopback, link-local, megosztott, dokumentációs, multicast és fenntartott címeket IPv4-en és IPv6-on is tiltja, az IPv4-re leképzett és a NAT64-es formákat is. A CI Linux-próbája a rögzített eturnal-képpel ezt a mintát futtatja véletlen titokkal, és csak a futtató gép saját címét engedi kivételként. A próba ellenőrzi, hogy lejárt vagy más titokkal készült hitelesítő adattal nem nyílik foglalás, és hogy a tiltott IPv4- és IPv6-címekre szóló jogosultságkérést a relé elutasítja. Igazolja azt is, hogy egy csomag oda-vissza átjut a relén, és hogy egy teljes WebRTC-kapcsolat, amelyben az agentnek csak relézett jelöltje van, képet visz át.

### Ellenőrzés és hátralévő munka

Az egységtesztek a referenciaszíneket, a területátlagolást, a kitöltött sorokat, a hibás méreteket és a kodekválasztást ellenőrzik. Ugyanígy ellenőrzik a vezérlő viselkedését jó, veszteséges, késleltetett, ingadozó és mérés nélküli kapcsolaton, kódolóhátraléknál, profilváltásnál és újracsatlakozáskor. A segéd kódolási egységének tesztje szintetikus asztalképpel ellenőrzi a méretezést, a kérésre és átméretezéskor készülő kulcskockát és a sorszámokat; a valódi asztalduplikálás tesztje kihagyja magát, ahol a munkamenet nem duplikálható. A szoftveres kódoló tesztje valódi Windows-kódolóval fut: első képkockától kódolt kimenet, baseline profil, SPS/PPS/IDR, kérésre kulcskocka, bitrátaváltás. Ahol a kódoló hiányzik, a teszt ezt jelzi és kihagyja magát. A WebRTC-fél és a munkamenet tesztje helyi UDP-n egy böngészőt helyettesítő str0m-féllel kapcsolódik: a darabolt képkocka a böngészőoldalon összeáll, az adatcsatorna mindkét irányban működik, a bevitel csak ellenőrzés után jut tovább, a statisztika valós értékeket hoz, kodek nélkül pedig nincs küldés. Az üzenetkódolás tesztjei a darabolást, az összerakást, az elavult darabokat és a hibás üzeneteket vizsgálják. A jelzéscserét az agent oldali egységteszt (csak az elfogadott kérésre, visszavonás után nem, túl nagy ajánlat elutasítva) és az adatbázisos integrációs teszt ellenőrzi a panel API-ján át: másik kezelő nem küldhet ajánlatot, üres ajánlat hibás, aktív kérésnél az agent válasza visszaér, második ajánlat ütközés, engedélyezés előtt nincs jelzéscsere. A teljes hálózati mátrixot a MIDAS-058 igazolja.

A Windows-oldali bekötést a kétirányú csatorna, a vezérlőcsatorna-üzenetek, a hálózati címek és a bevitelleképezés tesztje ellenőrzi, a CI elszigetelt próbája pedig azt, hogy a kiadási segédpár továbbra is kapcsolódik és tisztán leáll. A böngészős néző koordináta- és statisztikamodelljének tesztje a levágott sávokat, a billentyűnév-szűrést és a hibás jelentéseket vizsgálja. Hátravan a valódi gépen, virtuális kijelzővel, hardveres kódoló nélkül végzett kézi próba ([végső ellenőrzés](vegso-ellenorzes.md)).

## English

MIDAS-046 implements the video path of a consented session on Windows: the shared adaptive controller, codec negotiation, colour conversion, the software H.264 encoder built into Windows, the WebRTC side and session logic, signalling through the server, the process wiring and the browser viewer. A trial on a real machine with a virtual display is part of the final manual verification. With UAC rights the secure desktop and administrator windows are handled in the same connection (MIDAS-049). The grant renews every ten seconds ([authorization](tavsegitesi-felhatalmazas.md#english)); without renewal video and control stop within 30 seconds.

### Codec negotiation

Video is H.264 with packetization mode 1 (`packetization-mode=1`), so frames larger than a packet are fragmented. The choice starts from the browser's offer and the encoders found on the machine: hardware first (MIDAS-043–045), then software, and among profiles constrained baseline. Every browser decodes it, and it has no frame-reordering delay. Without a common format there is no video.

### Software H.264 and distribution terms

The software path is part of Windows: the Media Foundation H.264 encoder (`CLSID_MSH264EncoderMFT`). The agent distributes no codec binary; using the encoder is covered by the Windows licence. Windows N editions need the Media Feature Pack, and Windows Server needs the Media Foundation feature (`Server-Media-Foundation`). Without it the encoder reports itself unavailable, but it does not prevent the service from starting: the program delay-loads `mfplat.dll` and first loads it explicitly from System32.

OpenH264 is not bundled. Built from source, it is not covered by Cisco's H.264 patent licence, and the binary Cisco publishes must be downloaded by the user directly from Cisco ([terms](https://www.openh264.org/BINARY_LICENSE.txt)). Using it later would be a separate decision.

Encoder settings: baseline profile, low-latency mode, constant bitrate, keyframes on request, a regular keyframe every ten seconds, and BT.709 limited range signalled to the decoder. The bitrate can change while running; a resolution or frame-rate change needs a new encoder. In low-latency mode every frame is encoded at once, without buffering. This requires setting the encoder properties before the media types.

### Colour conversion

The encoder receives the captured BGRA image as NV12. Downscaling uses area averaging so text stays readable. Colours follow BT.709 in limited range, and the output size is even. The conversion respects the row stride, never reads row padding, and refuses short buffers and invalid sizes.

### Adaptive controller

From the bandwidth estimate, round-trip time (RTT), loss, jitter and the number of waiting frames, the controller derives a bitrate, a scale and a frame rate:

- At start and without measurements, 1 Mbit/s is the upper bound. With no estimate for 3 seconds the controller lowers to at most that level and never raises.
- Sustained loss (above 5%, smoothed), a rising round-trip time (above the lowest measured value plus 150 ms) and jitter (above 60 ms) count as congestion. The controller then lowers at once, and under sustained congestion at most once per second further. A falling estimate also lowers at once.
- Increases come only after 3 calm seconds, by at most 20% per step, and never above the lowest estimate of the last 5 seconds. A fluctuating link therefore produces no sawtooth.
- When the encoder or the sender falls behind, the frame rate drops first and recovers gradually later.
- The text profile keeps resolution, the motion profile keeps the frame rate. Moving up a step needs a 15% margin, so quality does not flap around a threshold.
- Reconnecting starts over. The hardware and software encoder limits (pixels, frame rate, bitrate) always apply.

Adaptation never touches permissions or consent.

### WebRTC side

The agent side builds on the str0m library, and video is protected by DTLS-SRTP. It answers the browser's offer and accepts only offers with a video line. The codec is chosen by the rule above once the connection is up; without a common format the side sends no video and the session ends. It advertises direct candidates on the given local addresses, and with a configured relay also relayed and server-reflexive ones (see TURN relay). Connection handling runs on the caller's thread with its own UDP socket. str0m's bandwidth estimate and its loss, RTT and jitter measurements feed the adaptive controller. On close, connection loss or an error state the side shuts for good. The WebRTC part goes only into agents that serve remote support: the Linux agent does not contain it.

### Worker and transport link

Following the system design, a separate protected transport process handles the WebRTC connection, while the user worker only captures, encodes and performs input. Messages between them travel over the local control channel. One of its messages is at most 64 KiB, so an encoded frame travels in chunks of at most 60 KiB with a sequence number and a header. The transport reassembles the chunks in order. It accepts at most 8 MiB per frame, drops an unfinished frame when a new one starts, and closes the session on any malformed or inconsistent message, because the worker is not trusted. The transport sends the worker the encoder settings (bitrate, scale, frame rate), keyframe requests and the stop, and the worker accepts them only within checked ranges. Two messages serve UAC handling: release, which lets go of the keys and buttons the worker still holds, and the desktop state (user, elevated window, secure), which the companion accepts only from the privileged worker.

### Worker video stream

The Windows worker captures the selected monitors, scales the image to the size the transport's settings give, encodes it and splits it into messages. The encoder is rebuilt only for a new size or frame rate, and then starts with a keyframe; the bitrate changes while running. A desktop change or a request produces a keyframe, on request also from the last captured image. An unchanged desktop sends nothing.

### Input in the worker

The worker injects the input it gets from the transport into its own desktop (`SendInput`). Keys travel as physical scan codes, so the target's own layout decides the character; this also holds for the `IntlBackslash` (í) key of the Hungarian ISO keyboard. Unknown key names do not pass. The pointer lands in the captured view's coordinates, buttons as state changes, the wheel in Windows units. The worker tracks pressed keys and buttons: it never releases a key that is not held, and at the end of the session it releases everything. Running with a user token, Windows (UIPI) does not let it inject into elevated windows or the secure desktop; only the privileged component may serve those, with the separate UAC permission.

### Video path processes on Windows

The ordinary worker in the user's session captures, encodes and performs input. The WebRTC transport runs in the SYSTEM companion that starts as its pair, the separate transport identity of the system design. The service keeps the companion's startup pipe open after startup and hands it the browser's offer with the local addresses there; the answer comes back the same way. Closing the pipe, the end of the lease or any process exiting stops the whole group. The control channel between the companion and the worker is split in two directions and carries frames and commands at the same time. Every message stays bounded, and the other side is still checked in both directions. Because every read waits at most five seconds, both sides send a keepalive every second. The companion offers the IPv4 addresses of working network interfaces as direct candidates, without disconnected, loopback and link-local addresses, and the relayed address when the server named a relay. The worker starts capturing only once the first settings arrive, and at the end of the session it releases every pressed key and button. If the desktop cannot be captured for a while (for example because of the secure desktop or a display change), the session stays and the worker retries capture every half second. Startup checks are unchanged: direct invocation stops with `0x22` or `0x23`, and graphics and media libraries load only on first use.

### Browser viewer

For an active request the panel opens the picture. It asks only for a receive-only video track and a "control" data channel, waits for its candidates, then sends the complete offer. Once the picture is selected, mouse, wheel and keyboard go to the remote computer: the pointer counts only inside the actual picture without letterbox bands, and keys travel as physical codes. The panel lets the operator choose the profile (text or motion). It shows the actual resolution, the sent bitrate and frame rate and the automatic mode from the agent's report every second. Without a fresh report for three seconds, the interface says so explicitly and never shows a healthy connection.

### Session in the transport

The transport session joins the WebRTC side, the adaptive controller and the worker's frames. It updates the settings every half second and sends them to the worker only when they change. Every second it sends the browser real data: actual resolution, target bitrate and frame rate, sent bitrate and frame rate, the automatic mode and the profile. From the browser it accepts only strictly parsed messages: keyframe requests, profile changes (text or motion), and pointer, wheel and key events in video coordinates (0–1). It drops unknown fields, out-of-range values and messages above 4 KiB, and queues input in a bounded queue. Input goes on to the worker; before execution the lease check decides for every event.

### Signalling

The browser sends its complete offer, with candidates already gathered (at most 16 KiB), to `POST /api/tenants/{id}/devices/{device}/remote-requests/{request}/media` and gets the agent's answer back from the same call. Only the requester may send an offer, for their own request once it is active (allowed by the local user and acknowledged by the agent), once per request, and only with `remote.control`. The server passes the offer over the authenticated agent connection (`MediaOffer`) and accepts the answer (`MediaAnswer`) only for the same request. The agent passes the offer only to the native backend of the request whose grant it accepted last and that is not revoked; otherwise it answers empty, without dropping the connection. Successful and refused signalling are both audited (`remote.media`); the SDP content is not.

### UAC in the same connection (MIDAS-049)

When the consented request includes UAC handling, the service also starts the privileged worker in the user's session next to the worker pair, with a separate SYSTEM token copy. This worker connects to the session's companion, not to the service. The service hands the companion a query-only handle to the worker's process and an endpoint. The companion checks that it is a SYSTEM process in its own session, and it accepts only one such worker per session. User applications keep running with the user's own rights, and the restricted worker handles the ordinary picture and input.

The privileged worker checks every tenth of a second which desktop receives input. When the secure desktop is shown (a UAC prompt or a credential window, for example), it moves onto it, captures and encodes the picture, and performs input there. It does not capture the user's desktop. But when a window above medium integrity, that is an administrator window, is in front or under the pointer, which the restricted worker's input could not reach because of Windows UIPI, it delivers the input. It reports every change of state to the companion before the first frame of the new state.

The companion decides on that basis. It takes frames from the privileged worker only on the secure desktop and otherwise only from the restricted worker; input goes to the privileged worker on the secure desktop and for elevated windows. On a change of source it drops the unfinished frame of the old source, requests a keyframe and has the keys and buttons still held on the old route released. Next to the measurements, the browser viewer says when the remote computer shows the secure desktop or an elevated window.

UAC, the secure desktop and system policies stay enabled, and the worker approves nothing by itself. The authorized technician handles the prompt; where Windows asks for credentials, an appropriate Windows account is needed. Without UAC rights no privileged worker starts: the secure desktop is then not visible, and no input reaches it. The CI isolated trial checks with the release program that the privileged worker joins the companion, that a second one cannot, and that it stops with the session. A trial with a real UAC prompt is part of the [final verification](vegso-ellenorzes.md#english).

### TURN relay (MIDAS-052)

When there is no direct path, picture and input travel over a TURN relay (eturnal). The media is still DTLS-SRTP protected end to end, so the relay sees only encrypted packets. From the relay's shared secret the server issues a short-lived credential (user name: the expiry and the request ID, password: its HMAC-SHA1 in Base64). It can open a new allocation for five minutes; refreshing a running allocation needs no new one. The relay password never decides when the session ends: the agent does, stopping when its grant expires or is revoked, and it also gives the allocation back.

The browser gets the relay addresses and its credential from the panel's `GET …/remote-requests/{request}/ice` endpoint. It is checked like starting the picture: only the requester, only for an active request, with every permission of the mode, and the response must not be cached. The agent gets its own credential with the browser's offer. It allocates a relayed address on the first UDP `turn:` URL, on the companion's own socket, and offers the relayed and its server-reflexive address next to the direct candidates. It wraps packets for the relay in TURN messages and renews permissions and the allocation in time. The relay refuses forbidden addresses, and the agent sends nothing more to them. For now the agent reaches the relay over UDP; TCP and TLS (port 443) already work on the browser side, and the agent side is a later extension.

The relay's sample configuration is `server/turn/eturnal.yml`. It forbids internal, loopback, link-local, shared, documentation, multicast and reserved addresses on IPv4 and IPv6, including IPv4-mapped and NAT64 forms. The Linux CI trial runs this sample with the pinned eturnal image and a random secret, with only the runner's own address as an exception. It checks that an expired credential, or one made with another secret, opens no allocation, and that the relay refuses permissions for forbidden IPv4 and IPv6 addresses. It also shows that a packet crosses the relay both ways, and that a whole WebRTC connection in which the agent has only a relayed candidate carries a picture.

### Verification and remaining work

Unit tests check reference colours, area averaging, padded rows, invalid sizes and the codec choice. They also check the controller on good, lossy, delayed, fluctuating and unmeasured links, with encoder backlog, profile changes and reconnection. The worker encoding test uses a synthetic desktop image to check scaling, keyframes on request and after a size change, and sequence numbers; the real desktop duplication test skips itself where the session cannot be duplicated. The software encoder test runs the real Windows encoder: encoded output from the first frame, baseline profile, SPS/PPS/IDR, keyframes on request and a bitrate change. Where the encoder is missing, the test says so and skips itself. The WebRTC side and session tests connect over loopback UDP to a str0m peer standing in for the browser: the chunked frame is reassembled on the browser side, the data channel works both ways, input passes only after validation, statistics carry real values, and nothing is sent without a codec. The message encoding tests cover chunking, reassembly, stale chunks and malformed messages. Signalling is checked by an agent unit test (only for the accepted request, not after revocation, oversized offers refused) and by the database integration test through the panel API: another operator cannot send an offer, an empty offer is invalid, the agent's answer comes back for an active request, a second offer conflicts, and there is no signalling before the user allowed. MIDAS-058 verifies the full network matrix.

Tests of the split channel, the control messages, network addresses and input mapping check the Windows wiring, and the CI isolated trial checks that the release worker pair still connects and stops cleanly. The browser viewer's coordinate and statistics model tests cover letterbox bands, key name filtering and malformed reports. Remaining: the manual trial on a real machine with a virtual display and no hardware encoder ([final verification](vegso-ellenorzes.md#english)).
