# Távsegítési kérés használhatósági próbája / Remote support request usability trial

[English](#english)

## Magyar

MIDAS-019 · Próbaprotokoll és megfigyelőlap. A résztvevős próba eredménye ebben a dokumentumban rögzítendő; amíg ez nem történt meg, a feladat felülvizsgálat alatt marad.

### A prototípus

A webpanel `#usability` címe alatt kattintható prototípus fut, mintaadatokkal és valódi távkapcsolat nélkül. A két nézetet két ablakban kell megnyitni, ugyanabból a böngészőből:

- `…/#usability/operator`: a kezelő adminpanele (Norda Kft., WS-BUD-014, két bejelentkezett Windows-felhasználó).
- `…/#usability/consent`: a felhasználó gépén megjelenő helyi jóváhagyó ablak.

Indítás fejlesztői gépen: `just dev`, majd a http://127.0.0.1:5173/#usability/operator cím. A „Másik nézet megnyitása új ablakban” gomb megnyitja a párját. A megfigyelői napló időbélyeggel rögzíti az eseményeket, és letölthető. Minden résztvevő előtt nyomd meg a megfigyelői panel **Alaphelyzet (mindkét nézet)** gombját.

A két nézet csak ugyanazon böngészőn belül kommunikál. A mobilos körben ezért mindkét nézetet a telefonon, két lapon nyisd meg: a résztvevő a kezelői lapot használja, a moderátor a válaszokat a másik lapon adja. Ehhez a fejlesztői szervert a helyi hálózaton is elérhetővé kell tenni (`pnpm --filter @midas/web-panel exec vite --host 0.0.0.0`); ezt csak megbízható hálózaton, a próba idejére tedd meg.

A prototípus a követelmények szerint működik:

- A kérés csak munkamenetválasztás után küldhető.
- A két kérésmód külön gombról indul.
- A várakozás, az elutasítás, a lejárat (60 s), a kapcsolódás és a bontás külön, élő régióban felolvasott állapot. A visszaszámlálás és a kapcsolat időtartama külön, nem élő sorban látszik, így a képernyőolvasó nem olvassa fel másodpercenként.
- A helyi ablakban a kezdő fókusz az Elutasítás. Az Engedélyezés a kérés megjelenése után 1 másodpercig nem aktiválható, így egy véletlen billentyű nem hagyhat jóvá.
- Késői vagy lejárt válasz nem hoz létre kapcsolatot. Kapcsolódás után a fókusz a Befejezés gombra kerül. Ha a fókuszált gomb eltűnik vagy inaktív lesz, a fókusz az állapotüzenetre kerül.

### Résztvevők és szerepek

- Moderátor: vezeti a feladatokat, nem segít a megoldásban.
- Kezelő: technikus vagy a moderátor a kezelői nézetben.
- Új felhasználó: legalább egy olyan résztvevő, aki nem ismeri a MIDAS-t, és nem vett részt a fejlesztésben. Magyar és angol nyelvű kört is futtatni kell, lehet két külön résztvevővel.

### Feladatok és sikerfeltételek

| # | Feladat | Siker |
| --- | --- | --- |
| 1 | A kezelő normál távvezérlést kér Kiss Anna asztalához. | A kezelő önállóan kiválasztja a munkamenetet és elküldi a kérést. |
| 2 | Az új felhasználó elolvassa a kérést, és saját szavaival elmondja, mit tehetne a technikus. | A nevet, a szervezetet és legalább a képernyő látását és a vezérlést helyesen visszaadja. |
| 3 | Az új felhasználó elutasítja a kérést. | Elutasít, és a kezelő látja, hogy nem jött létre kapcsolat. |
| 4 | A kezelő rendszergazdai módban új kérést küld; a felhasználó elmondja, miben más. | A felhasználó megérti, hogy UAC-kezelés és rendszergazdai műveletek is szerepelnek benne. |
| 5 | A felhasználó engedélyez, majd befejezi a kapcsolatot. | Megtalálja a Befejezés gombot, és mindkét oldal a lezárást mutatja. |
| 6 | Lejárat: a felhasználó nem válaszol 60 másodpercig. | Mindkét oldal azt mutatja, hogy a kérés lejárt és nincs kapcsolat. |
| 7 | Billentyűzetes kör: az 1–5. feladat egér nélkül. | A fókusz mindig látható, a párbeszédablak bezárásakor visszakerül a gombra, nincs elakadás. |
| 8 | Mobil adminpanel képernyőolvasóval (Android TalkBack vagy iOS VoiceOver, keskeny nézet): az 1., 3. és 5. feladat. | Az állapotváltozásokat a képernyőolvasó felolvassa, minden gomb megnevezett. |
| 9 | Helyi ablak képernyőolvasóval (Windowson NVDA vagy Narrátor): a 2–5. feladat. | A kérés címe, leírása és a lejárati idő felolvasásra kerül; a gombok egyértelműek. |

A 7–9. feladatot magyarul és angolul is el kell végezni.

### Megfigyelőlap

| Dátum | Résztvevő (kód) | Nyelv | Eszköz / képernyőolvasó | Feladat | Siker (igen/részben/nem) | Idő | Megjegyzés, idézet |
| --- | --- | --- | --- | --- | --- | --- | --- |
| | | | | | | | |

Személyes adatot ne rögzíts; a résztvevőket kóddal jelöld.

### A fejlesztő előzetes ellenőrzése (2026-11-06)

Az alábbiak nem helyettesítik a résztvevős próbát.

- **Kezelői nézet:** a munkamenetválasztás nélkül a Kérés küldése gomb inaktív, a fókusz a párbeszédablakba kerül, majd visszatér a kérés gombjára.
- **Állapotok:** a várakozás, az elutasítás, az engedélyezés, a felhasználói befejezés és a lejárat mindkét ablakban helyes állapotot mutat. A késői és idegen válaszokat a modell automatikus tesztje kizárja.
- **Helyi ablak:** a kezdő fókusz az Elutasítás. Az Engedélyezés 150 ms-nál inaktív, 1,85 s-nál aktív. Rendszergazdai módban a két többletképesség (UAC, rendszergazdai műveletek) megjelenik. Engedélyezés után a fókusz a Befejezés gombon van.
- **Nyelv és megjelenés:** a nézet magyarul és angolul, világos és sötét témában, keskeny szélességen is olvasható.
- **Billentyűzetes kör (7. feladat, magyarul és angolul):** az 1–5. feladat egér nélkül, valódi billentyűleütésekkel végigvihető mindkét ablakban. Az első körben két hiba derült ki: a kérés elküldése után és a helyi Befejezés után a fókusz a lap elejére esett. Javítás után a fókusz ilyenkor az állapotüzenetre kerül; Escape vagy Mégse után a kérés gombjára tér vissza. Az ismételt körben (küldés, Escape, elutasítás, engedélyezés, befejezés mindkét oldalról) a fókusz mindig látható volt, elakadás nem volt.
- **Képernyőolvasós előellenőrzés (8–9. feladat, 375 px, magyarul és angolul):** a látható vezérlők szerepét, nevét és az élő régiókat szkripttel olvastam ki a böngészőből; ez nem valódi képernyőolvasó. Minden gomb, rádiógomb és hivatkozás megnevezett. A munkamenet-csoport, a párbeszédablak és a jóváhagyó ablak (`alertdialog`) címe helyes. A leírás tartalmazza a szervezetet, a módot, a képességeket és a lejárati időt. A `lang` attribútum a nyelvvel együtt vált, vízszintes görgetés nincs. Hiba: a kezelői élő régió másodpercenként változott (visszaszámlálás, időtartam), így a képernyőolvasó folyamatosan újraolvasta volna. Javítás után az idő külön sorban van, az élő régió 3 másodperc alatt nem változott.
- **Lejárat angolul:** 60 másodperc után mindkét ablak a lejáratot mutatja, és nincs kapcsolat.
- **Nincs még elvégezve:** valódi képernyőolvasós kör (TalkBack vagy VoiceOver, NVDA vagy Narrátor), mobileszközös kör, valamint a 2–5. feladat megértési része új felhasználóval.

## English

MIDAS-019 · Trial protocol and observation form. Record the participant trial results in this document; until then the task stays in review.

### The prototype

The web panel's `#usability` route runs a clickable prototype with sample data and no real remote session. Open the two views in two windows of the same browser:

- `…/#usability/operator`: the operator's admin panel (Norda Kft., WS-BUD-014, two signed-in Windows users).
- `…/#usability/consent`: the local consent window shown on the user's computer.

To start on a development machine: `just dev`, then open http://127.0.0.1:5173/#usability/operator. The "Open the other view in a new window" button opens its counterpart. The facilitator log records timestamped events and can be downloaded. Press **Reset (both views)** in the facilitator panel before every participant.

The two views only communicate within the same browser. For the mobile round, open both views on the phone in two tabs: the participant uses the operator tab and the moderator answers in the other tab. This needs the development server reachable on the local network (`pnpm --filter @midas/web-panel exec vite --host 0.0.0.0`); do this only on a trusted network and only for the trial.

The prototype follows the requirements:

- A request can only be sent after choosing a session.
- The two request modes start from separate buttons.
- Waiting, declined, expired (60 s), connected and ended are separate states, announced through a live region. The countdown and the connection duration appear on a separate line outside the live region, so a screen reader does not announce them every second.
- In the local window the initial focus is Decline. Allow cannot be activated for 1 second after the request appears, so a stray key press cannot approve.
- A late or expired answer never creates a connection. After connecting, focus moves to the End button. If the focused button disappears or becomes inactive, focus moves to the status message.

### Participants and roles

- Moderator: guides the tasks and does not help solve them.
- Operator: a technician, or the moderator, in the operator view.
- New user: at least one participant who does not know MIDAS and was not involved in development. Run both a Hungarian and an English round, possibly with two different participants.

### Tasks and success criteria

| # | Task | Success |
| --- | --- | --- |
| 1 | The operator requests normal remote control of Kiss Anna's desktop. | The operator chooses the session and sends the request unaided. |
| 2 | The new user reads the request and explains in their own words what the technician could do. | They correctly report the name, the organisation, and at least screen viewing and control. |
| 3 | The new user declines the request. | They decline, and the operator sees that no connection was made. |
| 4 | The operator sends a new request in administrative mode; the user explains the difference. | The user understands that UAC handling and administrative actions are included. |
| 5 | The user allows, then ends the connection. | They find the End button, and both sides show the connection as closed. |
| 6 | Expiry: the user does not answer for 60 seconds. | Both sides show that the request expired and there is no connection. |
| 7 | Keyboard round: tasks 1–5 without a mouse. | Focus is always visible, returns to the button when the dialog closes, and nothing gets stuck. |
| 8 | Mobile admin panel with a screen reader (Android TalkBack or iOS VoiceOver, narrow view): tasks 1, 3 and 5. | The screen reader announces state changes, and every button has a name. |
| 9 | Local window with a screen reader (NVDA or Narrator on Windows): tasks 2–5. | The request title, description and expiry time are read out, and the buttons are unambiguous. |

Run tasks 7–9 in both Hungarian and English.

### Observation form

| Date | Participant (code) | Language | Device / screen reader | Task | Success (yes/partly/no) | Time | Notes, quotes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| | | | | | | | |

Do not record personal data; identify participants by code.

### Implementer pre-check (2026-11-06)

These checks do not replace the participant trial.

- **Operator view:** without a session choice the Send request button is inactive; focus moves into the dialog and returns to the request button.
- **States:** waiting, declined, allowed, user-ended and expired show the correct state in both windows. The model's automated tests rule out late and foreign answers.
- **Local window:** initial focus is Decline. Allow is inactive at 150 ms and active at 1.85 s. In administrative mode the two extra capabilities (UAC, administrative actions) appear. After allowing, focus is on End.
- **Language and appearance:** the view reads well in Hungarian and English, light and dark, at narrow width.
- **Keyboard round (task 7, Hungarian and English):** tasks 1–5 can be completed in both windows with real key presses and no mouse. The first round found two defects: after sending a request and after the local End, focus fell to the top of the page. After the fix, focus moves to the status message in these cases; after Escape or Cancel it returns to the request button. In the repeated round (send, Escape, decline, allow, end from both sides) focus was always visible and nothing got stuck.
- **Screen-reader pre-check (tasks 8–9, 375 px, Hungarian and English):** I read the roles, names and live regions of the visible controls from the browser with a script; this is not a real screen reader. Every button, radio button and link has a name. The session group, the dialog and the consent window (`alertdialog`) have correct titles. The description includes the organisation, the mode, the capabilities and the expiry time. The `lang` attribute follows the language, and there is no horizontal scrolling. Defect: the operator live region changed every second (countdown, duration), so a screen reader would have re-read it continuously. After the fix the time is on a separate line, and the live region did not change during 3 seconds.
- **Expiry in English:** after 60 seconds both windows show the expiry, and there is no connection.
- **Not yet done:** a real screen-reader round (TalkBack or VoiceOver, NVDA or Narrator), a round on a mobile device, and the comprehension part of tasks 2–5 with a new user.
