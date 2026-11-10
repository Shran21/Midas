const hu = {
  title: "Felügyelet nélküli elérés ellenőrzése",
  intro:
    "Ellenőrizd a kiválasztott cél és a szabályzat aktuális állapotát. Ez a lépés még nem indít kapcsolatot.",
  mode: "Vezérlés módja",
  normal: "Normál",
  admin: "Rendszergazdai",
  uac: "UAC-ablak kezelése",
  reason: "Művelet indoklása",
  reasonHint:
    "Rövid indoklás egy sorban, legfeljebb 512 UTF-8 bájt. Ne adj meg jelszót vagy más titkot.",
  invalidReason: "Adj meg rövid, egysoros indoklást.",
  check: "Hozzáférés ellenőrzése",
  checking: "Ellenőrzés…",
  eligible:
    "Az aktuális feltételek megfelelnek a felügyelet nélküli eléréshez. Kapcsolat még nem indult.",
  consent:
    "A célon felhasználó van jelen. Helyi jóváhagyás szükséges; felügyelet nélküli kapcsolat nem indítható.",
  expired: "Az ellenőrzés lejárt. A folytatáshoz új ellenőrzés szükséges.",
  denied:
    "Nincs megfelelő jogosultság, vagy a szabályzat és a karbantartási időablak nem engedélyezi ezt a műveletet.",
  unavailable:
    "A cél állapota megváltozott vagy nem ellenőrizhető. Kérd le újra a munkameneteket.",
  failed: "Az ellenőrzés nem sikerült. Próbáld újra.",
  fresh:
    "Friss passkey-hitelesítés szükséges. Utána kérd le és jelöld ki újra a célmunkamenetet.",
  authenticate: "Személyazonosság megerősítése",
};
const en: Record<keyof typeof hu, string> = {
  title: "Check unattended access",
  intro:
    "Check the selected target and current policy. This step does not start a connection.",
  mode: "Control mode",
  normal: "Normal",
  admin: "Administrative",
  uac: "Interact with UAC prompts",
  reason: "Reason for the operation",
  reasonHint:
    "A short single-line reason, up to 512 UTF-8 bytes. Do not include passwords or other secrets.",
  invalidReason: "Enter a short, single-line reason.",
  check: "Check access",
  checking: "Checking…",
  eligible:
    "Current conditions permit unattended access. No connection has been started.",
  consent:
    "A user is present in the target session. Local approval is required; unattended access is not permitted.",
  expired: "This check has expired. Check again before continuing.",
  denied:
    "Permissions, policy or the maintenance window do not allow this operation.",
  unavailable:
    "The target changed or could not be verified. Fetch sessions again.",
  failed: "The check failed. Try again.",
  fresh:
    "Fresh passkey authentication is required. Afterwards, fetch sessions and select the target again.",
  authenticate: "Confirm identity",
};
export const decisionCopy = { hu, en };
