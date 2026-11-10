const hu = {
  title: "Felügyelet nélküli szerverelérés",
  tenantTitle: "Ügyfélszintű engedély",
  deviceTitle: "Szerver és karbantartási időablak",
  tenantHint:
    "Az ügyfél engedélye mellett minden szervert külön is engedélyezni kell. Telephely vagy csoport nem kapcsolhatja be az elérést.",
  deviceHint:
    "Csak ezen a szerveren, egyszeri időablakban érvényes. Bejelentkezett felhasználó asztalához továbbra is helyi jóváhagyás kell.",
  tenantToggle: "Engedélyezem ennél az ügyfélnél",
  deviceToggle: "Engedélyezem ezen a szerveren",
  offHint: "A kikapcsolás a szerver tárolt időablakát is törli.",
  customer: "Ügyfél",
  server: "Szerver",
  enabled: "Engedélyezve",
  disabled: "Kikapcsolva",
  tenantOff:
    "Az ügyfél szabályzata tiltja az elérést. A szerver beállítása ettől még előkészíthető.",
  start: "Időablak kezdete",
  end: "Időablak vége",
  timezone: "Helyi időzóna",
  utc: "UTC szerint",
  noWindow: "Nincs beállított időablak",
  windowOpen: "Az időablak nyitva volt az ellenőrzéskor",
  windowClosed: "Az időablak az ellenőrzéskor zárva volt",
  checked: "Utoljára ellenőrizve",
  savedWindow: "Tárolt időablak",
  conditions:
    "A szabályzat engedélye önmagában nem jogosít csatlakozásra. Az operátor joga, friss hitelesítése, indoklása és a célmunkamenet ellenőrzése is szükséges.",
  reboot:
    "Újraindítás előtti megerősítés és a kapcsolat helyreállításának ellenőrzése kötelező.",
  unavailable:
    "A távkapcsolat indítása még fejlesztés alatt áll. Itt az előzetes engedélyeket készítheted elő.",
  readOnly:
    "A beállítások megtekinthetők. Módosításhoz hozzáférés-kezelési jog szükséges.",
  authHint:
    "A mentéshez friss passkey szükséges. Bekapcsoláshoz saját felügyelet nélküli elérési jog is kell.",
  save: "Mentés",
  saving: "Megerősítés és mentés…",
  saved: "A mentett beállításokat visszaellenőriztük.",
  confirmRetry:
    "A szerver új passkey-megerősítést kér. A vázlat megmaradt; kattints újra a Mentés gombra.",
  readbackFailed:
    "A mentést elküldtük, de a visszaellenőrzés nem sikerült. Frissítsd a beállításokat az eredmény ellenőrzéséhez.",
  refresh: "Beállítások frissítése",
  discard: "Módosítások elvetése",
  conflict:
    "A tárolt szabályzat szerkesztés közben megváltozott. Vesd el a vázlatot, és ellenőrizd az új beállításokat.",
  invalidTime:
    "Adj meg létező, egyértelmű kezdő és záró időpontot. Az óraátállításkor ismétlődő időpont helyett válassz másikat.",
  invalidOrder: "A befejezésnek későbbinek kell lennie a kezdésnél.",
  expired: "Az időablak vége már elmúlt. Válassz jövőbeli befejezést.",
};

const en: Record<keyof typeof hu, string> = {
  title: "Unattended server access",
  tenantTitle: "Customer permission",
  deviceTitle: "Server and maintenance window",
  tenantHint:
    "In addition to customer permission, each server needs its own opt-in. Sites and groups cannot enable access.",
  deviceHint:
    "Applies only to this server during a one-off window. A signed-in user's desktop still requires local consent.",
  tenantToggle: "Allow for this customer",
  deviceToggle: "Allow on this server",
  offHint: "Turning this off also clears the server's stored window.",
  customer: "Customer",
  server: "Server",
  enabled: "Enabled",
  disabled: "Off",
  tenantOff:
    "The customer policy blocks access. You can still prepare this server's settings.",
  start: "Window starts",
  end: "Window ends",
  timezone: "Local time zone",
  utc: "In UTC",
  noWindow: "No window configured",
  windowOpen: "The window was open when checked",
  windowClosed: "The window was closed when checked",
  checked: "Last checked",
  savedWindow: "Stored window",
  conditions:
    "Policy opt-in alone does not authorize a connection. Operator permission, fresh authentication, a reason and target-session verification are also required.",
  reboot:
    "Confirmation before reboot and verification of restored connectivity are mandatory.",
  unavailable:
    "Starting remote connections is still in development. You can prepare permissions here.",
  readOnly:
    "Settings are viewable. Changes require access-management permission.",
  authHint:
    "Saving requires a fresh passkey. Enabling also requires your own unattended-access permission.",
  save: "Save",
  saving: "Confirming and saving…",
  saved: "Saved settings have been verified.",
  confirmRetry:
    "The server requires a new passkey confirmation. Your draft is preserved; select Save again.",
  readbackFailed:
    "The save was submitted, but verification failed. Refresh settings to check the result.",
  refresh: "Refresh settings",
  discard: "Discard changes",
  conflict:
    "The stored policy changed while you were editing. Discard the draft and review the new settings.",
  invalidTime:
    "Enter valid, unambiguous start and end times. Choose a different time if a clock change makes it occur twice.",
  invalidOrder: "The end must be later than the start.",
  expired: "The window has already ended. Choose an end time in the future.",
};

export const policyCopy = { hu, en };
