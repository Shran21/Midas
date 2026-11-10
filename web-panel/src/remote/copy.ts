const hu = {
  title: "Munkamenetek",
  intro:
    "Kérd le a gép aktuális munkameneteit, majd jelöld ki a kívánt célt. A lekérés és a kijelölés nem indít távvezérlést.",
  fetch: "Munkamenetek lekérése",
  refresh: "Munkamenetek frissítése",
  loading: "Munkamenetek lekérése…",
  idle: "A munkamenetek kérésre jelennek meg.",
  choose: "Célmunkamenet kiválasztása",
  occupied: "Bejelentkezett",
  vacant: "Üres",
  unknown: "Bizonytalan",
  console: "Helyi konzol",
  session: "Munkamenet",
  locked: "Zárolt",
  unlocked: "Feloldott",
  lockUnknown: "Zárolási állapot nem ismert",
  noAccount: "Nincs bejelentkezett felhasználó",
  accountUnknown: "A felhasználó neve nem ismert",
  noSessions: "Nincs kijelölhető munkamenet a gépen.",
  noSessionsHint:
    "Ez nem engedélyez felügyelet nélküli hozzáférést. Később újra lekérheted a listát.",
  unknownHint:
    "A jelenlét nem állapítható meg biztosan. Kérj friss listát; ez a cél most nem jelölhető ki.",
  selected: "Kijelölt cél",
  selectHint: "Válassz egy munkamenetet a listából.",
  consent:
    "Ehhez a bejelentkezett felhasználó helyi jóváhagyása szükséges, zárolt vagy leválasztott munkamenet esetén is.",
  policy:
    "Üres munkamenethez is szükséges az ügyfél és a szerver engedélye, a megfelelő jogosultság, karbantartási időablak és friss hitelesítés.",
  workstation:
    "Az üres munkamenet nem jogosít fel felügyelet nélküli munkaállomás-vezérlésre.",
  connectionPending: "A távoli kapcsolat indítása még nem érhető el.",
  expired: "A lista elévült. Új kijelöléshez kérd le ismét a munkameneteket.",
  refreshed: "Lekérve",
  validity:
    "A kijelölés legfeljebb 30 másodpercig marad meg. Új lekérés, lapváltás vagy háttérbe helyezés törli; kapcsolatindítás előtt új ellenőrzés szükséges.",
  failed:
    "A munkamenetek most nem kérdezhetők le. Ellenőrizd a kapcsolatot, majd próbáld újra.",
  denied: "Nincs jogosultságod a gép munkameneteinek megtekintéséhez.",
  revoked: "A gép regisztrációját visszavonták.",
  unverified:
    "A gép aktuális állapota nem ellenőrizhető. Frissítsd az eszköz adatait, majd próbáld újra.",
  offline:
    "A gép jelenleg nem kapcsolódik. A munkamenetek akkor kérdezhetők le, ha újra elérhető.",
  unsupported:
    "Ez az agent még nem jelzi a munkamenet-lekérdezés támogatását. A funkcióhoz ezt támogató Windows-szolgáltatás szükséges.",
};

const en: Record<keyof typeof hu, string> = {
  title: "Sessions",
  intro:
    "Fetch the device’s current sessions, then choose a target. Fetching and selecting a session do not start remote control.",
  fetch: "Fetch sessions",
  refresh: "Refresh sessions",
  loading: "Fetching sessions…",
  idle: "Sessions appear when requested.",
  choose: "Choose a target session",
  occupied: "Signed in",
  vacant: "Vacant",
  unknown: "Uncertain",
  console: "Local console",
  session: "Session",
  locked: "Locked",
  unlocked: "Unlocked",
  lockUnknown: "Lock state unknown",
  noAccount: "No signed-in user",
  accountUnknown: "Account name unavailable",
  noSessions: "There are no selectable sessions on this device.",
  noSessionsHint:
    "This does not authorize unattended access. You can fetch the list again later.",
  unknownHint:
    "Presence could not be established. Fetch a fresh list; this target cannot be selected now.",
  selected: "Selected target",
  selectHint: "Choose a session from the list.",
  consent:
    "The signed-in user must approve locally, including when their session is locked or disconnected.",
  policy:
    "A vacant session still requires customer and server opt-in, the appropriate permissions, a maintenance window and fresh authentication.",
  workstation:
    "A vacant session does not authorize unattended workstation control.",
  connectionPending: "Starting a remote connection is not available yet.",
  expired:
    "The list has expired. Fetch sessions again before choosing a target.",
  refreshed: "Fetched",
  validity:
    "Selection lasts up to 30 seconds. Refreshing, leaving the tab or hiding the page clears it; starting a connection requires a fresh check.",
  failed: "Sessions could not be fetched. Check the connection and try again.",
  denied: "You do not have permission to view this device’s sessions.",
  revoked: "This device’s enrollment has been revoked.",
  unverified:
    "The device’s current state could not be verified. Refresh device information and try again.",
  offline:
    "This device is disconnected. Sessions can be fetched when it reconnects.",
  unsupported:
    "This agent does not report session-query support yet. A Windows service that supports this feature is required.",
};

export const sessionCopy = { hu, en };
