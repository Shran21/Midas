# Windows-képrögzítés és több monitor / Windows capture and multiple monitors

[English](#english)

## Magyar

2026. november 19. · MIDAS-042 · REMOTE-03, REMOTE-06

A segédfolyamat a Windows asztalát DXGI asztalduplikációval rögzíti. A technikus egy kiválasztott monitort vagy az összes monitort összevont nézetben láthat. Asztalváltáskor – például UAC-jóváhagyás vagy zárolás esetén – a régi asztalra szánt, még végre nem hajtott bevitel eldobódik.

### Monitorok és nézet

- A `win::capture::monitors` minden grafikus adapter asztalhoz csatolt kimenetét felsorolja. Minden monitornak van Windows-eszközneve (például `\\.\DISPLAY1`), helye a virtuális asztalon, elsődleges jelzése és forgatása.
- A `capture::Selection` egy monitort választ eszköznév szerint, vagy az összevont nézetet (`Combined`). Az összevont nézet a csatolt monitorok befoglaló téglalapja; a monitorok közötti, egyik monitor által sem fedett terület fekete marad.
- A nézet legfeljebb 16 384 × 16 384 képpont lehet, így hibásan jelentett elrendezés sem foglalhat korlátlan memóriát. Ismeretlen monitornév elutasított.
- A DXGI a kép kiolvasási tájolását adja. Az összerakás a 90, 180 és 270 fokos forgatást visszaforgatja, így álló helyzetű monitor is helyesen jelenik meg. A képletek a Microsoft asztalduplikációs mintájának textúra-leképezésével egyeznek.
- A `capture::to_desktop` a nézet egy képpontját visszaszámolja asztali koordinátára, de csak akkor, ha az valamelyik nézett monitorra esik. A monitorok közötti résre vagy a nézeten kívülre irányuló bevitelnek nincs célja.

### Asztalváltás és bevitel

- Ha a duplikáció elvész (`DXGI_ERROR_ACCESS_LOST`), vagy nem hozható létre (`E_ACCESSDENIED`, például a biztonsági asztalon), a rögzítő nem ad képet, és az asztalgeneráció megváltozik. A generáció a duplikáció visszatérésekor ismét változik.
- A `capture::InputQueue` minden bevitelt az akkor érvényes generációval jelöl, és követi a rögzítő generációját. Változáskor minden várakozó bevitelt eldob, a régi generációra érkező késői bevitelt pedig elutasítja. Így egy UAC-ablak megjelenése előtt beírt szöveg nem kerülhet a biztonsági asztalra, és visszatéréskor sem hajtódik végre. A sor korlátos.
- Az engedély (`Lease`) asztalidőszakra kötött bevitelellenőrzése ettől függetlenül érvényes marad. A sor a segéden belüli első védelem, nem helyettesíti a műveletenkénti engedélyellenőrzést.

### Folyamatmodell és erőforrás

- A normál asztalt a felhasználó munkamenetében futó korlátozott segéd rögzíti. A biztonsági asztal az emelt segéd feladata, és csak UAC-joggal érhető el.
- A `d3d11.dll` és a `dxgi.dll` késleltetve töltődik be. A szolgáltatás és a nem rögzítő résztvevő folyamat így nem tölti be a grafikus könyvtárakat.
- Új képkocka csak a kép változásakor készül; változatlan asztalnál a rögzítő az előző nézetet adja vissza, másolás nélkül. Erőforrásplafon nincs.

### Ellenőrzés

- Egységtesztek: monitorválasztás és összevont nézet negatív koordinátájú másodlagos monitorral; elhelyezés és lefedetlen terület; a három forgatás; képpont-visszaszámolás csak nézett monitorra; asztalváltáskor a várakozó és a késői bevitel eldobása, a sor korlátja.
- A valódi rögzítés tesztje ott fut le, ahol a Windows asztalduplikációt kínál. A fejlesztői gép RDP-munkamenetében a DXGI egy csatolt kimenetet sem ad, ezért ott a teszt csak ezt rögzíti.
- Valódi, több monitoros Windows 11-gépen a monitorválasztás, az összevont nézet, a forgatott monitor és az UAC-váltás kézi próbája a [végső ellenőrzés](vegso-ellenorzes.md) része.

### Ismert korlátok

- A rögzítés még nincs bekötve a képátviteli útba; ez a MIDAS-046, -049 és -053 része.
- Az egérmutató nincs a képen, mert az asztalduplikáció külön adja; a mutató megjelenítése a képátvitellel együtt készül.
- Csak 8 bites BGRA-formátum támogatott, HDR-kimenet nem. A kép a processzoron át másolódik; a hardveres kódolók közvetlen útja a MIDAS-043–045 része.

## English

November 19, 2026 · MIDAS-042 · REMOTE-03, REMOTE-06

The worker process captures the Windows desktop with DXGI desktop duplication. The technician can view a selected monitor or all monitors in a combined view. On a desktop switch, for example a UAC prompt or a lock, input queued for the old desktop and not yet executed is discarded.

### Monitors and view

- `win::capture::monitors` lists the desktop-attached outputs of every graphics adapter. Each monitor has its Windows device name (for example `\\.\DISPLAY1`), its position on the virtual desktop, a primary flag and its rotation.
- `capture::Selection` selects one monitor by device name or the combined view (`Combined`). The combined view is the bounding rectangle of the attached monitors; desktop area between monitors that no monitor covers stays black.
- A view is at most 16,384 × 16,384 pixels, so a misreported layout cannot allocate without bound. An unknown monitor name is refused.
- DXGI returns the image in scan-out orientation. Composition undoes 90, 180 and 270 degree rotation, so a portrait monitor appears upright. The formulas match the texture mapping of Microsoft's desktop duplication sample.
- `capture::to_desktop` maps a view pixel back to a desktop coordinate, but only when it falls on a viewed monitor. Input aimed at the gap between monitors or outside the view has no target.

### Desktop switches and input

- When duplication is lost (`DXGI_ERROR_ACCESS_LOST`) or cannot be created (`E_ACCESSDENIED`, for example on the secure desktop), the capturer returns no image and the desktop generation changes. The generation changes again when duplication returns.
- `capture::InputQueue` stamps every input with the generation current at the time and follows the capturer's generation. On a change it discards all pending input and refuses late input for the old generation. Text typed before a UAC prompt appears therefore cannot reach the secure desktop and is not executed on return either. The queue is bounded.
- The grant's (`Lease`) desktop-epoch input check still applies independently. The queue is the worker's first line of defense and does not replace per-operation authorization.

### Process model and resources

- The restricted worker in the user's session captures the ordinary desktop. The secure desktop belongs to the privileged worker and requires UAC rights.
- `d3d11.dll` and `dxgi.dll` are delay-loaded, so the service and participant processes that do not capture never load the graphics libraries.
- A new frame is produced only when the image changes; on an unchanged desktop the capturer returns the previous view without copying. No resource ceiling is set.

### Verification

- Unit tests: monitor selection and the combined view with a secondary monitor at negative coordinates; placement and uncovered area; the three rotations; mapping pixels back only onto viewed monitors; discarding pending and late input on a desktop switch, and the queue bound.
- The real-capture test runs wherever Windows offers desktop duplication. In the development machine's RDP session DXGI reports no attached output, so there the test only records that.
- A manual trial on a real multi-monitor Windows 11 machine of monitor selection, the combined view, a rotated monitor and a UAC switch belongs to the [final verification](vegso-ellenorzes.md#english).

### Known limits

- Capture is not yet connected to the media path; that belongs to MIDAS-046, -049 and -053.
- The mouse pointer is not in the image, since desktop duplication delivers it separately; drawing it comes with media delivery.
- Only 8-bit BGRA is supported, no HDR output. The image is copied through the CPU; a direct path for hardware encoders belongs to MIDAS-043–045.
