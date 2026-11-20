import type { Transfer } from "./model";

const hu = {
  title: "Távsegítés kérése",
  intro:
    "A bejelentkezett felhasználó a saját gépén látja a nevedet, a szervezetet és a kért műveleteket, és ő dönt. Válasz nélkül a kérés 60 másodperc után lejár.",
  normal: "Távoli vezérlés kérése",
  administrative: "Rendszergazdai távvezérlés kérése",
  uac: "Windows rendszergazdai engedélykérések (UAC) kezelése",
  clipboard: "Vágólap (szöveg) mindkét irányban",
  upload: "Fájlküldés a gépre",
  download: "Fájlletöltés a gépről",
  transfer: (transfer: Transfer) =>
    [
      transfer.clipboard && "vágólap",
      transfer.upload && "fájlküldés",
      transfer.download && "fájlletöltés",
    ]
      .filter(Boolean)
      .join(", "),
  sending: "Kérés küldése…",
  unsupported:
    "Ezen a gépen nincs bekapcsolva a távsegítés. A kéréshez az agent távsegítési beállítását be kell kapcsolni.",
  target: (device: string, session: string) =>
    `Célgép: ${device} · ${session}. munkamenet`,
  people: (requester: string, account: string) =>
    `Kérelmező: ${requester}${account ? ` · Bejelentkezett felhasználó: ${account}` : ""}`,
  mode: (administrative: boolean, uac: boolean) =>
    `Mód: ${administrative ? "rendszergazdai távvezérlés" : "normál távvezérlés"}${uac ? ", UAC-kezeléssel" : ""}`,
  waiting: (who: string) => `Várakozás ${who} jóváhagyására.`,
  remaining: (seconds: number) => `A kérés ${seconds} másodperc múlva lejár.`,
  active:
    "A felhasználó engedélyezte a kérést, a felhatalmazás érvényes. A kép és a vezérlés megjelenítése még nem érhető el.",
  activeRemaining: (seconds: number) =>
    `A felhatalmazás még ${seconds} másodpercig érvényes.`,
  declined: (who: string) =>
    `${who} elutasította a kérést. Nem jött létre kapcsolat.`,
  expired: (who: string) =>
    `${who} nem válaszolt, a kérés lejárt. Nem jött létre kapcsolat.`,
  unavailable:
    "Most nem kérhető helyi jóváhagyás: nincs bejelentkezett felhasználó, vagy már fut egy távsegítés. Nem jött létre kapcsolat.",
  cancelled: "Visszavontad a kérést. Nem jött létre kapcsolat.",
  ended: "A kapcsolat véget ért.",
  endedBy: (reason: string | null) =>
    ({
      local: "A felhasználó a gépén befejezte a kapcsolatot.",
      desktop:
        "A kapcsolat véget ért, mert a gépen megváltozott a munkamenet vagy a bejelentkezett felhasználó.",
      expired:
        "A kapcsolat véget ért, mert a gép nem kapott időben megújítást.",
      offline: "A kapcsolat véget ért, mert a gép nem érhető el.",
      revoked:
        "A kapcsolat véget ért, mert a jogosultságod vagy a bejelentkezésed időközben megszűnt.",
      stopped: "A kapcsolat a gépen leállt.",
    })[reason ?? ""] ?? "A kapcsolat véget ért.",
  failedFresh:
    "A felhasználó engedélyezte a kérést, de a passkey-megerősítés közben elavult. Küldj új kérést; a panel előtte újra megerősítteti.",
  failed:
    "A kérés nem teljesült, mert a jogosultság vagy a gép kapcsolata időközben megváltozott. Nem jött létre kapcsolat.",
  someone: "a felhasználó",
  cancel: "Kérés visszavonása",
  end: "Kapcsolat bontása",
  again: "Új kérés",
  fresh:
    "Ehhez a módhoz friss passkey-megerősítés kell. Erősítsd meg a passkeyddel, majd küldd el ismét a kérést.",
  authenticate: "Megerősítés passkeyjel",
  busy: "A gépen már folyamatban van egy távsegítés vagy kérés. Próbáld újra később.",
  offline:
    "A gép most nem érhető el, vagy a kijelölt munkamenet elavult. Kérd le ismét a munkameneteket.",
  denied: "Nincs jogosultságod ehhez a kéréshez.",
  error:
    "A kérés nem küldhető el. Ellenőrizd a kapcsolatot, majd próbáld újra.",
};

const en: {
  [Key in keyof typeof hu]: (typeof hu)[Key] extends string
    ? string
    : (typeof hu)[Key];
} = {
  title: "Request remote support",
  intro:
    "The signed-in user sees your name, your organisation and the requested actions on their computer, and decides. Without an answer, the request expires after 60 seconds.",
  normal: "Request remote control",
  administrative: "Request administrative remote control",
  uac: "Handle Windows administrator prompts (UAC)",
  clipboard: "Clipboard (text) both ways",
  upload: "Send files to the device",
  download: "Download files from the device",
  transfer: (transfer) =>
    [
      transfer.clipboard && "clipboard",
      transfer.upload && "sending files",
      transfer.download && "downloading files",
    ]
      .filter(Boolean)
      .join(", "),
  sending: "Sending request…",
  unsupported:
    "Remote support is not turned on for this device. Turn on the agent's remote support setting to send requests.",
  target: (device, session) => `Device: ${device} · session ${session}`,
  people: (requester, account) =>
    `Requester: ${requester}${account ? ` · Signed-in user: ${account}` : ""}`,
  mode: (administrative, uac) =>
    `Mode: ${administrative ? "administrative remote control" : "normal remote control"}${uac ? ", with UAC handling" : ""}`,
  waiting: (who) => `Waiting for ${who} to answer.`,
  remaining: (seconds) => `The request expires in ${seconds} seconds.`,
  active:
    "The user allowed the request and the authorization is valid. Remote view and control are not available yet.",
  activeRemaining: (seconds) =>
    `The authorization remains valid for ${seconds} seconds.`,
  declined: (who) => `${who} declined the request. No connection was made.`,
  expired: (who) =>
    `${who} did not answer and the request expired. No connection was made.`,
  unavailable:
    "Local consent cannot be requested now: nobody is signed in, or remote support is already running. No connection was made.",
  cancelled: "You withdrew the request. No connection was made.",
  ended: "The connection has ended.",
  endedBy: (reason: string | null) =>
    ({
      local: "The user ended the connection on their computer.",
      desktop:
        "The connection ended because the session or the signed-in user changed on the computer.",
      expired:
        "The connection ended because the computer did not receive a renewal in time.",
      offline: "The connection ended because the computer is not reachable.",
      revoked:
        "The connection ended because your permission or sign-in ended meanwhile.",
      stopped: "The connection stopped on the computer.",
    })[reason ?? ""] ?? "The connection has ended.",
  failedFresh:
    "The user allowed the request, but the passkey confirmation went stale meanwhile. Send a new request; the panel asks for a confirmation first.",
  failed:
    "The request did not complete because a permission or the device connection changed meanwhile. No connection was made.",
  someone: "the user",
  cancel: "Withdraw request",
  end: "End connection",
  again: "New request",
  fresh:
    "This mode needs a fresh passkey confirmation. Confirm with your passkey, then send the request again.",
  authenticate: "Confirm with passkey",
  busy: "Remote support or a request is already in progress on this device. Try again later.",
  offline:
    "The device is not reachable now, or the selected session is out of date. Fetch the sessions again.",
  denied: "You do not have permission for this request.",
  error: "The request could not be sent. Check the connection and try again.",
};

export const requestCopy = { hu, en };
