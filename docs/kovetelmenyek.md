# MIDAS működési követelmények / Operating requirements

## Magyar

Frissítve: 2026. november 2. Az alábbi működést a prototípuson és a fázisok elfogadási próbáin kell igazolni.

Ez a dokumentum pontosítja a [rendszertervet](rendszerterv.md). Az erőforrásigény, az eszközkezelés, a két távsegítségi mód, az UAC, a szerverek és a tartományi műveletek tekintetében ez az irányadó követelményleírás. A normál munkamenetben is kötelező az UAC-prompt távoli megjelenítése és kezelése.

### 1. Erőforrás-takarékos működés

- **PERF-01:** a háttérben futó agent eseményvezérelten működjön, egy tartós, kimenő, hitelesített kapcsolattal. Ne legyen folyamatos teljes leltározás, képrögzítés, videokódolás vagy gyors állapotlekérdezés.
- **PERF-02:** az alapbeállítás 30 másodperces heartbeat és percenkénti metrikaküldés. Három kimaradt heartbeat után az eszköz offline. Az első teljes leltár regisztráció után készül. Az agent megőrzi az utolsó sikeres leltár idejét, újraindítás után onnan folytatja a napi egyeztetést, és közben csak a változásokat küldi. Az ütemezés gépenként kis véletlen eltérést kap.
- **PERF-03:** az agent takarékosan végezze a háttérmunkát. Az üzemeltető az egyes gyűjtések, feltöltések, frissítések és szkriptek terhelését feladatonként szabályozhatja. Aktív távsegítéskor a halasztható háttérfeladatok várjanak, a valós idejű képátvitel és vezérlés kapjon elsőbbséget.
- **PERF-04:** a videós folyamat és a GPU-kódoló csak engedélyezett távsegítés alatt fusson. A késleltetés fontosabb a távsegítés erőforrásigényénél, de a sorok és pufferek ekkor is legyenek korlátosak. A jóváhagyás, a chat és a tálcaikon ne igényeljen folyamatosan nyitott teljes WebView-felületet; a könnyű állandó felület és az igény szerint betöltött Tauri-nézet megoldását prototípus dönti el.
- **PERF-05:** hálózatkieséskor korlátos helyi tároló, fokozatos visszaküldés és exponenciális újracsatlakozás kell. Metrikáknál lehet összesítés és dokumentált adateldobás; auditot és feladateredményt nem szabad észrevétlenül eldobni. Ha ezeknek nincs helye, az új kockázatos művelet álljon meg, miközben az állapotjelzés és a helyi leállítás működik.
- **PERF-06:** a dashboard lapozott vagy virtualizált eszközlistát és az éppen látható, engedélyezett adatokra szűrt élő eseményeket használjon. A szerver kötegeljen, és ügyfelenként korlátozza a munkát; ne küldje újra a teljes flottát minden változáskor.
- **PERF-07:** a távsegítés alapértelmezésben automatikusan alkalmazkodjon a becsült rendelkezésre álló sávszélességhez, az RTT-hez, a csomagvesztéshez és a késleltetés ingadozásához. Romló hálózaton csökkentse, tartós javuláskor fokozatosan növelje a bitrátát és szükség szerint a felbontást, illetve a képkockaszámot. A vezérlés gyors reakciója és a szöveg olvashatósága elsődleges; a szöveg- és mozgásprofil eltérő sorrendben igazíthatja a kép paramétereit. A rövid kilengések ne okozzanak folyamatos minőségváltást, a képkockák ne halmozzanak késést. Hiányzó hálózati mérés nem jelent korlátlan sávszélességet. A hardver és a dekódoló képességeit figyelembe kell venni, önkényes globális erőforrásplafon nélkül. A felület mutassa az automatikus módot, a tényleges felbontást, bitrátát és képkockaszámot. Közvetlen és TURN-kapcsolaton is kell hálózatromlási és helyreállási próba mindkét távvezérlési módban, UAC alatt is; az alkalmazkodás nem módosíthat jogosultságot vagy jóváhagyást.

A háttéragent és az aktív távsegítés az adott feladathoz szükséges erőforrást használhatja, előre rögzített CPU-, GPU-, memória- vagy adatforgalmi plafon nélkül. A prototípuson külön mérjük a háttérben futó agentet, a megnyitott helyi felületet és az élő távsegítést. Windows 11-en a MIDAS felületéhez tartozó WebView2-folyamatokat is beleszámítjuk. Ezekkel a mérésekkel a felesleges terhelést keressük.

| Kapacitás és válaszidő | Vizsgálati cél | Mérési körülmények |
| --- | --- | --- |
| Terhelési próba | 1000 szimulált agent | metrikák, újracsatlakozási hullám, ügyfélszigetelés és a dashboard válaszidejének mérése |
| Aktív távsegítés | LAN-on p95 < 60 ms; interneten p95 < 120 ms | bevitel és látható válasz között; hálózati RTT, veszteség, GPU, kodek, felbontás és közvetlen/TURN út dokumentálva |

A késleltetési cél nem hálózattól független garancia. Rossz hálózaton legyen látható a mért késleltetés, és a rendszer a képrátát, bitrátát és szükség esetén a felbontást igazítsa, korlátlan pufferelés helyett. Lassú gépen is maradjon olvasható szövegre optimalizált mód.

### 2. Az agent és a webes adminfelület használata

- **UI-01:** több ügyfél több munkaállomása és szervere vehető fel. Kártyás és listás nézet, keresés, ügyfél/telephely/csoport, OS, online állapot, riasztás és függő újraindítás szerinti szűrés kell. A regisztráció és az agent telepítése ügyfélhez és telephelyhez kötött.
- **UI-02:** az eszközre kattintva saját oldal nyíljon: Áttekintés; Monitorozás és riasztások; Hardver és szoftverleltár; Frissítések; Szkriptek és feladatok; Fájlkezelő; Terminál; Távsegítség; Tartomány; Előzmények és audit.
- **UI-03:** az eszközcsempe és listasor jobbklikkes menüje ugyanazokat az engedélyezett műveleteket kínálja. Billentyűzettel és mobilon külön menügombból is elérhető legyen minden. A menü és az eszközoldal ugyanazt az engedélyellenőrzést használja; a szerver minden kérést önállóan ellenőriz.
- **UI-04:** távvezérlést kizárólag az adminisztrátor vagy az adott gépre jogosult technikus kezdeményezhet a webes adminpanelen. A **Távoli vezérlés kérése** és a **Rendszergazdai távvezérlés kérése** gomb a megfelelő módot indítsa, a módhoz szükséges jog ellenőrzésével. Látszódjon a célgép, a kiválasztott Windows-munkamenet, a kérelmező és az aktív mód. Több bejelentkezett felhasználónál kötelező a munkamenetválasztás.
- **UI-05:** offline géphez képernyőkapcsolat nem indulhat. A sorba állítható feladatnál legyen lejárat, karbantartási ablak, újraellenőrzés és egyértelmű „várakozik” állapot. A szerverekhez legyen külön profil és újraindítási szabályzat.
- **UI-06:** az agent kezdőnézete mutassa a kapcsolat állapotát. Az adminpanelről érkező kérésnél a helyi ablak nevezze meg a kérelmezőt és köznyelven a kért műveleteket, például hogy kezelheti-e a Windows rendszergazdai engedélykéréseit. Az **Engedélyezés**, az **Elutasítás** és aktív kapcsolatnál a **Befejezés** legyen könnyen megtalálható. Kapcsolathibánál a felület mondja meg, mit tehet a felhasználó.
- **UI-07:** a webes adminfelületen a kiválasztott ügyfél, gép és munkamenet neve a művelet indításáig látszódjon. Az eszköz áttekintésében legyenek elöl a gyakori műveletek. A részletes beállítások igény szerint nyíljanak meg. A távvezérlési kérésnél a technikus külön lássa a várakozást, az elutasítást, a lejáratot és az aktív kapcsolatot. Csoportos művelet előtt lássa az érintett gépeket, utána a gépenkénti eredményt.
- **UI-08:** mindkét felület legyen használható magyarul és angolul, egérrel és billentyűzettel. A fókusz látszódjon, az állapotnak legyen szöveges neve, a vezérlők és visszajelzések legyenek érthetők képernyőolvasóval is. A jóváhagyás tudatos helyi műveletet igényeljen. A webes párbeszédablak bezárásakor a fókusz térjen vissza a kérés indítójához, ha az még elérhető. A próbákhoz a [W3C párbeszédablak-mintája](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) ad támpontot.

### 3. Két távsegítségi mód, mindkettőben UAC-kezeléssel

Az agentet rendszergazdai jóváhagyással telepítjük, a szükséges szolgáltatásokkal és helyi engedélyekkel. Windows alatt a privilegizált agent-szolgáltatás gépindítástól, bejelentkezett felhasználó nélkül is működik. A telepítési jogosultság nem jelent automatikus távoli hozzáférést minden technikusnak.

| Tulajdonság | Távoli vezérlés kérése | Rendszergazdai távvezérlés kérése |
| --- | --- | --- |
| Cél | közös munka a felhasználó meglévő asztalán | előre engedélyezett rendszergazdai munkavégzés |
| Helyi jóváhagyás attended kapcsolatnál | a felhasználó engedélyezi a kért vezérlést | a felhasználó a rendszergazdai módot és a kért képességeket engedélyezi |
| Felhasználói alkalmazások | a meglévő felhasználói jogosultságaikkal futnak | a meglévő alkalmazások jogosultsága nem változik automatikusan |
| UAC-prompt | látható és kezelhető, külön UAC-joggal | látható és kezelhető, külön UAC-joggal |
| Rendszergazdai eszközök | külön kért és ellenőrzött műveletként | eleve engedélyezett munkamenet-képességként, műveletenkénti ellenőrzéssel |
| Technikusi jog | normál távvezérlés; UAC-hoz külön engedély | normál + rendszergazdai távvezérlés; UAC-hoz külön engedély |
| Erős újrahitelesítés | UAC-képesség engedélyezéséhez friss passkey-megerősítés | munkamenet indításakor friss passkey-megerősítés |

- **REMOTE-01:** a helyi ablak az ellenőrzött technikust, az ügyfelet, a kért módot és az engedélyeket mutatja, az UAC-kezelést is beleértve. Az elfogadás atomikusan felhasználja a kérést. Elutasításkor, megszakításkor vagy lejáratkor a kérés lezárul, a késve érkező válasza érvénytelen. A kép és a bevitel csak érvényes engedéllyel indul. A helyi válaszra alapértelmezésben 60 másodperc van.
- **REMOTE-02:** a jóváhagyást a felhasználói munkamenetben futó asztali komponens kéri, és a service ellenőrzi és köti a konkrét technikushoz, eszközhöz, Windows-munkamenethez, módhoz és egyszer használható munkamenet-azonosítóhoz. Az ablak nem jelenhet meg a service 0. munkamenetében.
- **REMOTE-03:** normál módban a munkamenet-segéd a felhasználó korlátozott jogával kezeli az általános képet és bevitelt. Engedélyezett UAC-képességnél a megfelelő rendszerjoggal futó, külön komponens ugyanabban a munkamenetben átveszi a biztonságos asztal és az emelt ablak kezelését. Az UAC-prompt legyen látható és vezérelhető a böngészőből. A normál asztalra visszatérve ismét a korlátozott segéd végzi az általános munkát.
- **REMOTE-04:** a `remote.uac.interact` engedély mindkét módban kérhető a kapcsolódáskor. A technikus friss passkey-megerősítése és a kért UAC-kezelésre kiterjedő helyi jóváhagyás vagy szerveres unattended szabályzat szükséges. Így egy későbbi UAC-prompt ugyanabban a normál kapcsolatban látható és kezelhető. A `remote.admin` külön engedély a rendszergazdai MIDAS-műveletekhez.
- **REMOTE-05:** a Windows UAC-ja, a secure desktop és a rendszer házirendjei bekapcsolva maradnak. Az agent nem kattinthat automatikusan az engedélyezésre. A promptot az arra jogosult technikus kezeli; ahol a Windows hitelesítő adatot kér, megfelelő Windows-fiók kell. A MIDAS-mód nem teszi a bejelentkezett felhasználót rendszergazdává, és nem ad Domain Admin-jogot.
- **REMOTE-06:** a privilegizált komponens csak az engedélyezett célmunkamenet képét és bevitelét kezeli. A hitelesített helyi IPC a védett transzport identitását, az asztalt és az aktív felhatalmazást ellenőrzi. A biztonságos asztal képe, a csatornakulcs és az emelt bevitel útvonala legyen elzárva a normál felhasználói folyamattól. Asztalváltáskor a régi célhoz várakozó bevitelt el kell dobni. A MIDAS terminál-, fájl- és szkript-API-jai külön jogosultságot kérnek. Egy Windows által ténylegesen megemelt grafikus programban a technikus a program Windows-jogaival cselekedhet. Ezért a `remote.uac.interact` kiemelt jog mindkét módban.
- **REMOTE-07:** a helyi Befejezés gomb az agenten azonnal állítsa le a kép- és beviteli csatornát, közvetlen és relézett kapcsolatnál is. A szerver visszavonását kézbesítéskor alkalmazni kell. Elveszett jelzéskapcsolatnál a felhatalmazás megújítás nélkül legfeljebb 30 másodpercig élhet. Másik felhasználóhoz vagy munkamenethez új engedély kell. A technikus távoli bevitellel saját kérését nem hagyhatja jóvá.
- **REMOTE-08:** az agent kihagyja a billentyűleütéseket, hitelesítő adatokat és vágólaptartalmat a naplókból. A későbbi felvételi funkció az azonosított UAC-, bejelentkezési és hitelesítési felületeken a rögzítés forrásánál szünetel, a pufferelt képre is kiterjedően. Ezt mindkét oldal jelzi. Egyéb alkalmazások érzékeny mezőinek felismerhetőségét külön kell igazolni. Ahol a szükséges kizárás nem biztosítható, ott a felvétel tiltott. A bevitelhez fizikai billentyűkód és szöveg-/IME-kezelés kell, eltérő és magyar kiosztással is tesztelve.

Az alkalmazott Windows API-k és a megfelelő token-, munkamenet-, window station- és desktop-kezelés működőképességét **kötelező korai prototípusnak igazolnia**. A LocalSystem-szolgáltatás telepítése önmagában nem bizonyítja az UAC működését. A cél a böngészős technikusi felület és a telepített natív agent együttműködése.

### 4. Szerverek és felhasználó nélküli hozzáférés

- **SERVER-01:** az agenttel telepített Windows- és Linux-szerverek ugyanabban az eszközmodellben jelenjenek meg. Monitorozás, frissítés, fájlkezelő, terminál és jogosultsághoz kötött szkriptfuttatás bejelentkezett felhasználó nélkül is működjön. A terminál és fájlkezelő a választott, naplózott végrehajtási környezetet használja; ez nem következik automatikusan a képernyővezérlés módjából.
- **SERVER-02:** felhasználó nélküli szerveres munkamenethez együttesen szükséges az ügyfél és az adott szerver előzetes engedélyezése, a külön unattended jog, friss passkey-megerősítés, indoklás és lejáró felhatalmazás. Alapállapotban a szabályzat tilt. A felügyelet nélküli kérés csak a megnevezett szerveres munkamenetre érvényes. Jelen lévő felhasználó asztalának átvételéhez az attended folyamat tartozik.
- **SERVER-03:** Windows Server Desktop Experience esetén a bejelentkező és zárolt képernyő kezelése is cél. Server Core és más grafikus asztal nélküli gép esetén a dashboard a terminált és háttéreszközöket kínálja. A Linuxos grafikus távsegítés továbbra is külön, későbbi feladat.
- **SERVER-04:** bejelentkezett felhasználót vagy RDP-munkamenetet nem szabad csendben leváltani; az operátor válasszon célt. A szerverprofil karbantartási ablakot, újraindítás előtti megerősítést és kapcsolat-helyreállítási ellenőrzést írjon elő.

### 5. Tartományi felderítés és beléptetés

- **AD-01:** az AD-val történő technikusi belépés, a tartományi eszközök felderítése és egy gép tartományba léptetése három külön képesség. Ezek külön jogosultságot és külön auditot kapnak.
- **AD-02:** ügyfelenként egy kijelölt helyi agent/connector olvassa a megadott AD-t titkosított, ellenőrzött kapcsolaton, lehetőleg integrált hitelesítéssel és csak olvasási joggal. A felderítés és az AD-írást végző beléptetés külön identitást használjon. A felderítés OU-szűréssel, lapozással és ütemezett szinkronnal a megadott tartományhatáron belül marad. DC-t nem nyitunk ki az internetre, és nincs korlátlan hálózati pásztázás.
- **AD-03:** a felfedezett AD-számítógép „Felderített, agent nincs” jelöltként jelenjen meg. Az AD-bejegyzés nem élő állapotjelzés és nem igazolja, hogy az eszköz megbízható. Csak regisztrált, jóváhagyott agenttel vezérelhető. AD-objectGUID, tartomány, ügyfél és ellenőrzött agent-hozzárendelés alapján legyen duplikációkezelés; IP-cím vagy gépnév önmagában nem identitás.
- **AD-04:** agent telepítésére GPO, Intune, csomag vagy kifejezetten jóváhagyott telepítési feladat használható. A felderítés önmagában nem telepít agentet, nem léptet tartományba, és nem mozgat AD-objektumot.
- **AD-05:** tartományba léptetéshez célzott gép, ügyfél, tartomány, OU és gépnév előnézete; külön `domain.join` engedély, friss megerősítés, az adott OU-ra delegált AD-jog és a célgépen helyi adminisztrátori agent kell. Ellenőrizni kell a Windows-kiadást, a DNS-t, az időszinkront, az elérhetőséget és az aktuális tartománytagságot. Másik tartományból átléptetés nem automatikus újrapróbálkozás.
- **AD-06:** a kijelölt helyi connector a célgép AD-fiókjához állítja elő a `djoin` csomagot az adott OU-ra delegált joggal. A MIDAS titkosítva tárolja, a jóváhagyott célagent kulcsához titkosítva kézbesíti, és igazolt használat vagy kézbesítési lejárat után törli a tárolt példányt. A már visszafejtett blob felhasználhatóságát a MIDAS határideje önmagában nem korlátozza. Kiszivárgásnál a gépfiókkal kapcsolatos AD-oldali helyreállítás is szükséges. A megoldáshoz tartós Domain Admin-jelszó tárolása nem kell.
- **AD-07:** az újraindítás tervezett és jóváhagyott legyen. Utána a domain trustot, az agent visszatérését és az eszköz ügyfélhez rendelését ellenőrizni kell. Számítógépfiók-újrafelhasználásnál a Windows aktuális domain join hardening szabályai érvényesek. Linux tartományi beléptetése későbbi, külön megtervezendő képesség.

### 6. Biztonsági követelmények

Biztonsági célok és ellenőrizhető védelmek szükségesek; teljes kockázatmentesség nem garantálható.

- **SEC-01:** alapból tiltott hozzáférés; ügyfélre, eszközre és műveletre szűkített RBAC, alkalmazásoldali ellenőrzés és adatbázis-RLS. A képernyő, UAC, adminisztráció, terminál, fájlműveletek, szkript, unattended és domain join külön engedély.
- **SEC-02:** az új agent helyben kulcspárt készít, és a szervert hitelesítő TLS-en egy elkülönített regisztrációs végponthoz fordul. Az egyszer használható token hatókörének, a privát kulcs birtoklásának és a jóváhagyásnak vagy előzetesen hitelesített telepítési identitásnak az ellenőrzése után kap kliens-tanúsítványt. A regisztrációs végpont csak beiratkozást kezel. A műveleti csatornához érvényes, vissza nem vont mTLS-azonosság kell.
- **SEC-03:** rövid életű, aláírt műveleti felhatalmazás: technikus, ügyfél, eszköz, munkamenet, képességek, egyszer használható azonosító és lejárat. Minden privilegizált műveletnél aktuális jogosultságellenőrzés, visszajátszás elleni védelem és visszavonás kell.
- **SEC-04:** az audit rögzíti a kérést, döntést, módot, UAC-aktiválást, a MIDAS által kiadott műveletek célját és eredményét, valamint a lezárást. A grafikus munkamenetben végzett műveletekből nem következtet billentyűnaplóval parancsokra. A hash-láncot külön jogosultság alatt álló, hozzáfűzhető vagy megváltoztathatatlan külső másolat vagy hiteles ellenőrzőpont védi. Az ellenőrzés észlelje a törlést, átírást és a napló végének csonkolását is.
- **SEC-05:** a DTLS-SRTP védi a médiát a hálózati megfigyelőtől és a TURN-relétől. A központi jelzéskiszolgáló és a dashboardot kiszolgáló szerver a bizalmi modell része; egy kompromittált központ elleni médiavédelmet a WSS önmagában nem biztosít. Ilyen további védelem külön végpont-azonossági tervet igényel.
- **SEC-06:** aláírt frissítések, visszaállás, korlátozott szolgáltatások, titoktároló, függőségellenőrzés, ügyfelek közti negatív tesztek és külső biztonsági felülvizsgálat kell az éles pilot előtt. Külön ügyfél- és globális vészleállítás függessze fel az új távoli műveleteket, és vonja vissza az aktív felhatalmazásokat.

### 7. Megvalósítás és elfogadás

A követelményekhez rendelt konkrét feladatokat és mérföldköveket a [feladatlista](feladatlista.md) rögzíti.

| Fázis | Kiegészítés és elfogadási feltétel |
| --- | --- |
| 0 | képességmodell, erőforrásmérő tesztkörnyezet, eszközoldal és **korai WebRTC/UAC-prototípus**; normál munkamenetből is látható és kezelhető valódi secure-desktop UAC, védelmi házirendek kikapcsolása nélkül |
| 1 | több gép, munkaállomás/szerver profil, takarékos agent, eszközlista, AD-felderítés és regisztrációs jóváhagyás; 24 órás és 1000 agentes mérés |
| 2 | a két teljes távvezérlési mód, normál módú UAC, helyi jóváhagyás, szabályozott unattended szerverelérés; zárolás, UAC, RDP, több munkamenet és NAT/TURN tesztelve |
| 3 | elkülönített normál/admin terminál és fájlműveletek, korlátozott szkriptek, frissítések, domain join delegált OU-joggal és ellenőrzött újraindítással |
| 4 | teljesítményregressziót ellenőrző kapuk, vészleállítás, visszavonási tesztek, külső audit és pilot |
| 5 | a kiválasztott bővítés önálló elfogadási próbája és a már kiadott funkciók regressziós ellenőrzése |

**Használhatósági próba.** Az 1. fázisban a két felület 0. fázisban elkészült prototípusán, a 2. fázis végén valódi kapcsolattal próbáljuk ki a folyamatot. Az adminpanel kezelője két hasonló nevű gép közül válassza ki a megfelelő ügyfélét, majd küldjön távvezérlési kérést. Az agentet először használó munkatárs azonosítsa a kérelmezőt, mondja el, mire kér engedélyt, és utasítsa el a kérést. Az adminpanelen jelenjen meg az elutasítás, kép és bevitel nélkül. A kezelő indítson új kérést, a felhasználó engedélyezze, majd helyben fejezze be a kapcsolatot. Mindkét résztvevő tudja követni az eredményt külső útmutatás nélkül. Ismételjük meg billentyűzettel és a dashboard mobilnézetében, az agent párbeszédét képernyőolvasóval is.

A Windows-próba fedje le a normál és rendszergazdai módot standard és adminfiókkal, az UAC jóváhagyási és jelszókérő változatát, a zárolást és feloldást, a konzolt és több RDP-munkamenetet, valamint a magyar és az eltérő kiosztást. Próbáljuk ki az elutasítást, a lejárt kérés késői elfogadását, a helyi leállítást a biztonságos asztal használata alatt is, a jog visszavonását és a jelzéskapcsolat elvesztését. A támogatás a sikeresen vizsgált kombinációkra állítható. A normál módú UAC hiánya a vállalt Windows-konfiguráción akadálya a fázis elfogadásának.

### 8. További javaslatok, még nem vállalt funkciók

- **Felhasználói segítségkérés és hibajegykezelés.** Későbbi lehetőség egy **Segítséget kérek** gomb, amellyel a felhasználó hibajegyet küldhet és követheti annak állapotát. A bevezetésről a saját hibajegykezelés vagy egy meglévő rendszerhez való csatlakozás megtervezésekor döntünk. Ha a jegy megoldásához távvezérlés kell, a jogosult kezelő külön kérést indít az adminpanelen.
- **Windows LAPS-integráció:** eszközönként eltérő, forgatott helyi admin-hitelesítés, külön hozzáféréssel és visszakereshető használattal.
- **Időszakos szerepkiosztás és kétfős jóváhagyás:** emelt technikusi szerep automatikus lejárattal, független második jóváhagyó a kijelölt kritikus vagy tömeges műveletekhez.
- **Ügyfelenkénti karantén:** gyanús agent vagy feltört technikusi fiók célzott elszigetelése; az audit és a biztonsági állapotjelzés megőrzésével, a kötelező vészleállítás kiegészítéseként.
- **Nézet mód vezérlés nélkül:** választható csak képernyőmegtekintés, majd külön kért vezérlés; a két elsődleges kapcsolódási gomb mellett később bevezethető.
- **Flottaszintű megfelelőségi nézet:** az agentfrissítések teszt-, pilot- és teljes körös állapota, a BitLocker, a Defender, a javítások és a függő újraindítások egy áttekintésben.

## English

Updated November 2, 2026. Verify the behavior below on the prototype and during phase acceptance tests.

This document refines the [system plan](rendszerterv.md#english) and takes precedence for resource use, device management, remote support modes, UAC, servers and domain operations. Remote viewing and interaction with UAC prompts are mandatory in normal user-level sessions as well.

### 1. Efficient resource use

- **PERF-01:** use an event-driven background agent with one persistent authenticated outbound connection. Do not continuously run full inventory, screen capture, video encoding or frequent polling.
- **PERF-02:** default to a 30-second heartbeat and metrics every minute. Mark a device offline after three missed heartbeats. Take the first full inventory after enrollment. Persist the last successful inventory time across restarts, reconcile daily and send deltas between runs. Stagger collection across devices.
- **PERF-03:** the agent should run background work efficiently. Operators can control the load of individual collection, upload, update and script jobs. During remote support, defer nonessential background work and prioritize real-time video and input.
- **PERF-04:** start video workers and GPU encoders only for authorized support sessions. Latency takes priority over support-session resource use, while queues and buffers remain bounded. Consent, chat and the tray icon must not require a permanently open full WebView. Prototype a lightweight resident interface with Tauri views loaded on demand.
- **PERF-05:** use bounded offline storage, gradual upload and exponential reconnection backoff. Metrics may be aggregated or dropped with explicit reporting; audit records and job results must not be silently discarded. Exhausted audit/result capacity blocks new risky operations while health reporting and local termination remain available.
- **PERF-06:** paginate or virtualize device lists and scope live events to visible, authorized data. Batch server writes and limit work per customer; do not retransmit the whole fleet on every change.
- **PERF-07:** remote support defaults to automatic adaptation based on estimated available bandwidth, RTT, packet loss and jitter. Decrease bitrate and, where needed, resolution or frame rate when conditions degrade; increase them gradually after sustained recovery. Responsive control and readable text take priority; text and motion profiles may adjust parameters in different orders. Avoid quality oscillation on brief fluctuations and accumulating stale frames. Missing network measurements never imply unlimited bandwidth. Respect encoder/decoder capabilities without arbitrary global resource ceilings. Show automatic mode and actual resolution, bitrate and frame rate. Test degradation and recovery over direct and TURN paths in both control modes, including UAC; adaptation must not change permissions or consent.

The background agent and active remote support may use the resources their workload needs, without a preset cap on CPU, GPU, memory or network traffic. On the prototype, measure the background agent, the open local interface and live remote support separately. On Windows 11, include the WebView2 processes belonging to the MIDAS interface. Use these measurements to identify unnecessary load.

| Capacity and responsiveness | Test target | Measurement conditions |
| --- | --- | --- |
| Load test | 1000 simulated agents | measure metrics, reconnection storms, tenant isolation and dashboard response time |
| Active remote support | LAN p95 < 60 ms; internet p95 < 120 ms | input to visible response; document RTT, packet loss, GPU, codec, resolution and direct/TURN path |

Latency targets are conditional on the network. Show measured latency on poor links and adapt frame rate, bitrate and, if necessary, resolution rather than buffering indefinitely. Preserve a text-readability mode on slower devices.

### 2. Using the agent and web admin panel

- **UI-01:** manage multiple workstations and servers across customers. Provide cards and lists, search and filters for customer/site/group, OS, online status, alerts and pending reboot. Bind installation and registration to a customer and site.
- **UI-02:** opening a device shows Overview; Monitoring and alerts; Hardware/software inventory; Updates; Scripts and jobs; File manager; Terminal; Remote support; Domain; History and audit.
- **UI-03:** device-card and row context menus expose the same authorized actions. Provide an explicit menu button for keyboard and mobile access. Reuse permission rules across menus and device pages, and independently authorize every server request.
- **UI-04:** only an administrator or a technician authorized for the device may initiate remote control from the web admin panel. **Request remote control** and **Request administrative remote control** select the mode, subject to its required permissions. Show the target device, selected Windows session, requester and active mode. Multiple signed-in users require explicit session selection.
- **UI-05:** do not start screen sessions on offline devices. Queued jobs need expiry, a maintenance window, revalidation and a visible waiting state. Servers have a distinct profile and reboot policy.
- **UI-06:** show connection status on the agent home view. When a request arrives from the admin panel, the local dialog names the requester and describes the requested actions in everyday language, including whether the requester can interact with Windows administrator prompts. Users can find **Allow**, **Decline** and the active session’s **End** control unaided. A connection error explains the next action.
- **UI-07:** keep the selected customer, device and session visible in the web admin panel through execution. Put common actions on the device overview and open detailed settings on demand. Distinguish waiting for consent, declined, expired and connected. Show affected devices before a bulk operation and individual outcomes afterwards.
- **UI-08:** support Hungarian and English, mouse and keyboard on both interfaces. Provide visible focus, text labels for status and screen-reader-accessible controls and feedback. Consent requires a deliberate local action. When a web dialog closes, return focus to its invoking control if it is still available. Use the [W3C dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/) when testing.

### 3. Two support modes, both supporting UAC

Install the agent with administrative approval, required services and local permissions. The privileged Windows service runs from boot without requiring a signed-in user. Installation privileges do not authorize all technicians to connect.

| Property | Request remote control | Request administrative remote control |
| --- | --- | --- |
| Purpose | collaborate on the existing user desktop | perform administrative work authorized at session start |
| Local consent for an attended connection | the user allows the requested control | the user allows administrative mode and the listed capabilities |
| Existing applications | retain their existing user privileges | retain existing privileges; no automatic upgrade of running applications |
| UAC prompts | visible and interactive with separate UAC permission | visible and interactive with separate UAC permission |
| Administrative tools | individually requested and authorized operations | pre-authorized session capability, still checked for each operation |
| Technician permissions | normal control; separate UAC permission | normal and administrative control; separate UAC permission |
| Fresh authentication | fresh passkey confirmation to activate the UAC capability | fresh passkey confirmation at session start |

- **REMOTE-01:** show the verified technician, customer, mode and requested capabilities, including UAC. Acceptance atomically consumes the request. Decline, cancellation or expiry closes it and invalidates any late reply. Start capture and input only after authorization. Allow 60 seconds for a local response by default.
- **REMOTE-02:** the user-session desktop component requests consent. The service validates it and binds it to the technician, device, Windows session, mode and single-use session ID. Do not display the popup in service Session 0.
- **REMOTE-03:** in normal mode the session worker handles ordinary capture and input with restricted user privileges. With UAC permission, a separate privileged component handles the secure desktop and elevated windows within that same session. The technician must see and interact with the UAC prompt in the browser. The restricted worker resumes ordinary work on return to the normal desktop.
- **REMOTE-04:** request `remote.uac.interact` at connection time in either mode. Require fresh technician passkey verification and consent covering UAC, or the applicable unattended server policy covering UAC. A later UAC prompt must then be visible and interactive in the same normal session. The separate `remote.admin` permission governs administrative MIDAS operations.
- **REMOTE-05:** keep Windows UAC, secure desktop and security policies enabled. The agent must not automatically approve prompts. An authorized technician interacts with the prompt, using appropriate Windows credentials when required. MIDAS mode does not make the signed-in user an administrator or grant Domain Admin privileges.
- **REMOTE-06:** the privileged component handles only capture and input for the authorized target session. Authenticated local IPC checks the protected transport identity, target desktop and active grant. Isolate secure frames, channel keys and the elevated input path from the normal user process. Discard pending input for the old surface on a desktop switch. MIDAS terminal, file and script APIs require separate permissions. In an application actually elevated by Windows, interactive actions carry that application's Windows privileges. Treat `remote.uac.interact` as a privileged permission in both modes.
- **REMOTE-07:** the local End control immediately stops capture and input at the agent on direct and relayed connections. Apply server revocation on delivery. If signaling is lost, an unrenewed grant may last at most 30 seconds. Changing target user or session requires fresh authorization. Remote input cannot approve the technician’s own request.
- **REMOTE-08:** exclude keystrokes, credentials and clipboard content from logs. The later recording feature pauses at the capture source on identified UAC, sign-in and credential surfaces, including buffered frames, and indicates this to both parties. Verify detection of sensitive fields in other applications separately. Disable recording wherever the required exclusions cannot be enforced. Test physical keys and text/IME entry with Hungarian and mismatched layouts.

A **mandatory early prototype** must prove the Windows APIs and correct token, session, window-station and desktop handling. Installing a LocalSystem service alone does not prove UAC support. The target is a browser technician interface backed by the installed native agent.

### 4. Servers and unattended access

- **SERVER-01:** agent-equipped Windows and Linux servers use the same device model. Monitoring, updates, file management, terminal and authorized scripts work without a signed-in user. Terminal and file operations use explicitly selected and audited execution identities, independent of screen-session mode.
- **SERVER-02:** a server session without a local participant requires both customer and individual server opt-in, dedicated unattended permission, fresh passkey confirmation, a stated reason and an expiring audited grant. Default to deny. The request applies only to the named server session. Taking over a present user's desktop follows the attended flow.
- **SERVER-03:** support Windows Server Desktop Experience sign-in and locked screens. On Server Core and other hosts without a graphical desktop, offer terminal and background tools. Linux graphical support remains a separate later feature.
- **SERVER-04:** never silently replace a signed-in user or RDP session; require target selection. Server policies require maintenance windows, reboot confirmation and post-reboot connectivity verification.

### 5. Domain discovery and joining

- **AD-01:** technician AD login, domain-device discovery and joining a device to a domain are distinct capabilities with separate permissions and audit.
- **AD-02:** a designated customer-local agent/connector queries AD over an encrypted, verified connection, preferably using integrated authentication and read-only access. Use a separate identity for discovery and write-capable provisioning. Scope discovery by OU, paginate and synchronize on a bounded schedule within explicit domain boundaries. Expose no DC to the internet and perform no unrestricted network scanning.
- **AD-03:** show discovered computer objects as "Discovered, no agent" candidates. AD records are neither live health checks nor proof of trust. Control requires an enrolled, approved agent. Deduplicate using AD objectGUID, domain, customer and verified agent association; hostname or IP address alone is not identity.
- **AD-04:** deploy agents through GPO, Intune, packages or explicitly approved deployment jobs. Discovery alone never installs agents, joins domains or moves AD objects.
- **AD-05:** preview device, customer, domain, OU and hostname before joining. Require `domain.join`, fresh confirmation, delegated rights for that OU and a local administrative agent. Verify Windows edition, DNS, time synchronization, connectivity and current domain membership. Moving from another domain is not an automatic retry.
- **AD-06:** the designated local connector provisions a `djoin` blob for the target AD computer account using OU-delegated rights. MIDAS encrypts the stored copy, encrypts delivery to the approved target agent's key and deletes its copy after verified use or delivery expiry. The delivery deadline cannot invalidate an already decrypted blob. Exposure also requires AD-side remediation of the computer account. The design requires no stored Domain Admin password.
- **AD-07:** schedule and approve reboot, then verify domain trust, agent return and customer association. Follow current Windows domain-join hardening rules for computer-account reuse. Linux domain joining requires a separate later design.

### 6. Security requirements

Use explicit security goals and verifiable controls; complete absence of risk cannot be guaranteed.

- **SEC-01:** deny access by default. Scope RBAC to customer, device and operation, enforce application authorization and database RLS. Separate screen, UAC, administrative, terminal, file, script, unattended and domain-join permissions.
- **SEC-02:** a new agent creates its key pair locally and contacts a dedicated enrollment endpoint over server-authenticated TLS. Issue the client certificate after verifying single-use token scope, proof of private-key possession and either explicit approval or a verified deployment identity. The enrollment endpoint only supports enrollment. The operational channel requires a valid, unrevoked mTLS identity.
- **SEC-03:** use short-lived signed grants binding technician, customer, device, session, capabilities, single-use ID and expiry. Check current authorization, replay protection and revocation for every privileged operation.
- **SEC-04:** audit requests, decisions, mode, UAC activation, targets and results of MIDAS-issued actions, and termination. Do not reconstruct commands from keystrokes in graphical sessions. Protect the hash chain with an externally held append-only or immutable copy or authenticated checkpoint under separate access control. Check for removal, rewriting and truncation of the log tail.
- **SEC-05:** DTLS-SRTP protects media from network observers and TURN relays. Signaling and dashboard servers remain trust anchors; WSS alone does not protect against a compromised central server. Such additional protection requires a separate endpoint-identity design.
- **SEC-06:** require signed updates, rollback, restricted services, secret storage, dependency checks, cross-tenant negative tests and external security review before production pilot. Customer and global emergency stops suspend new remote actions and revoke active grants.

### 7. Delivery and acceptance

The [task list](feladatlista.md#english) maps these requirements to tasks and milestones.

| Phase | Addition and acceptance criteria |
| --- | --- |
| 0 | capability model, resource test environment, device page and **early WebRTC/UAC prototype**; view and interact with real secure-desktop UAC from a normal session without disabling security policies |
| 1 | multiple devices, workstation/server profiles, efficient agent, device lists, AD discovery and enrollment approval; 24-hour and 1000-agent measurements |
| 2 | both complete control modes, normal-session UAC, local consent, governed unattended server access; lock/UAC/RDP/multi-session and NAT/TURN testing |
| 3 | separate normal/admin terminal and file operations, bounded scripts, updates and domain joining with delegated OU permissions and verified reboot |
| 4 | performance regression gates, emergency stop, revocation tests, external audit and pilot |
| 5 | feature-specific acceptance test for each selected extension plus regression checks for released features |

**Usability trial.** Test both interface prototypes, built in phase 0, during phase 1 and repeat with a real connection at the end of phase 2. The admin panel operator chooses the correct customer's device from two similarly named machines and sends a remote-control request. A first-time agent user identifies the requester, explains the requested permissions and declines. The admin panel shows the refusal and no media or input starts. The operator sends a new request, which the user accepts before ending the session locally. Both participants must be able to follow the outcome unaided. Repeat with a keyboard and the mobile dashboard, and test the agent dialog with a screen reader.

The Windows trial covers normal and administrative modes with standard and administrator accounts, both UAC consent and credential prompts, lock/unlock, console and multiple RDP sessions, plus Hungarian and mismatched layouts. Test decline, late acceptance after expiry, local stop including during secure-desktop interaction, revocation and loss of signaling. Claim support only for combinations that pass. Failure to control UAC in normal mode on a committed Windows configuration blocks phase acceptance.

### 8. Additional proposals, not committed features

- **User help requests and ticketing.** A future **Request help** action could let users submit a ticket and follow its status. Decide on this feature when designing ticket handling or integration with an existing ticketing system. If a ticket requires remote control, an authorized operator sends a separate request from the admin panel.
- **Windows LAPS integration:** per-device rotating local administrator credentials with separately authorized, auditable retrieval.
- **Temporary role assignment and two-person approval:** expire elevated technician roles automatically and require an independent second approver for designated critical or bulk operations.
- **Customer quarantine:** selectively isolate suspicious agents or compromised technicians while retaining audit and security health reporting, supplementing the mandatory emergency stop.
- **View-only access:** optionally start with screen viewing and request control separately; a later option alongside the two primary connection buttons.
- **Fleet compliance view:** combine the status of test, pilot and fleet-wide agent updates with BitLocker, Defender, patching and pending reboots.

## Források / Sources

- [Microsoft: service isolation and user-session IPC](https://learn.microsoft.com/en-us/windows/win32/services/interactive-services)
- [Microsoft: SendInput and integrity-level restrictions](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [Microsoft: Desktop Duplication access to secure desktop](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput)
- [Microsoft: domain join permissions and offline domain join](https://learn.microsoft.com/en-us/windows-server/identity/ad-ds/manage/active-directory-domain-join-permissions)
- [Microsoft: offline domain join with djoin](https://learn.microsoft.com/windows-server/remote/remote-access/directaccess/directaccess-offline-domain-join)
- [MDN: physical key codes and keyboard layouts](https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/code)
- [IETF: WebRTC security architecture](https://www.rfc-editor.org/rfc/rfc8827)
