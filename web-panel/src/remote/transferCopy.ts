import type { Problem } from "./transferModel";

const hu = {
  title: "Vágólap és fájlok",
  clipboard: "Vágólap",
  outgoing: "Szöveg a távoli vágólapra",
  sendClipboard: "Küldés a távoli vágólapra",
  getClipboard: "Távoli vágólap lekérése",
  incoming: "A távoli vágólap szövege",
  copy: "Másolás a saját vágólapra",
  clipboardSent: "A szöveg a távoli gép vágólapjára került.",
  clipboardReceived: "Megérkezett a távoli vágólap szövege.",
  copied: "A szöveg a saját vágólapodra került.",
  tooLong: "A szöveg legfeljebb 32 KiB lehet.",
  upload: "Fájl küldése a gépre",
  uploadHint:
    "A fájl a felhasználó Letöltések\\MIDAS mappájába kerül. Megszakadás után ugyanazt a fájlt újra kiválasztva a küldés folytatódik.",
  choose: "Fájl kiválasztása",
  uploading: (name: string, done: string, total: string) =>
    `${name}: ${done} / ${total} elküldve`,
  uploaded: (name: string) => `Elkészült a gépen: ${name}`,
  download: "Fájl letöltése a gépről",
  browse: "Mappa megnyitása",
  up: "Egy szinttel feljebb",
  name: "Név",
  size: "Méret",
  modified: "Módosítva",
  folder: "mappa",
  fetch: "Letöltés",
  truncated: "A mappában több elem van, mint amennyi itt látszik.",
  downloading: (name: string, done: string, total: string) =>
    `${name}: ${done} / ${total} letöltve`,
  downloaded: (name: string) => `Letöltve: ${name}`,
  memoryOnly:
    "Ez a böngésző nem tud részleges letöltést tárolni; nagy fájlnál és megszakadáskor a letöltés elölről indul.",
  tooLargeHere: "Ez a fájl ebben a böngészőben túl nagy a letöltéshez.",
  closed:
    "Az átviteli csatorna lezárult; a megkezdett átvitel új kapcsolatban folytatható.",
  problem: (problem: Problem) =>
    ({
      unavailable: "A művelet most nem végezhető el a távoli gépen.",
      denied: "A felhasználó jogosultsága ezt nem engedi.",
      not_found: "Nem található.",
      too_large: "Túl nagy.",
      changed: "A fájl a listázás óta megváltozott; nyisd meg újra a mappát.",
      no_space: "Nincs elég hely a távoli gépen.",
      empty: "A távoli vágólapon nincs szöveg.",
      invalid: "Az átvitel megszakadt. Indítsd újra.",
    })[problem],
};

const en: {
  [Key in keyof typeof hu]: (typeof hu)[Key] extends string
    ? string
    : (typeof hu)[Key];
} = {
  title: "Clipboard and files",
  clipboard: "Clipboard",
  outgoing: "Text for the remote clipboard",
  sendClipboard: "Send to the remote clipboard",
  getClipboard: "Get the remote clipboard",
  incoming: "Text from the remote clipboard",
  copy: "Copy to my clipboard",
  clipboardSent: "The text is on the remote computer's clipboard.",
  clipboardReceived: "The remote clipboard's text arrived.",
  copied: "The text is on your clipboard.",
  tooLong: "The text can be at most 32 KiB.",
  upload: "Send a file to the device",
  uploadHint:
    "The file goes into the user's Downloads\\MIDAS folder. After an interruption, choose the same file again to resume.",
  choose: "Choose a file",
  uploading: (name, done, total) => `${name}: ${done} of ${total} sent`,
  uploaded: (name) => `Saved on the device as ${name}`,
  download: "Download a file from the device",
  browse: "Open folder",
  up: "Up one level",
  name: "Name",
  size: "Size",
  modified: "Modified",
  folder: "folder",
  fetch: "Download",
  truncated: "The folder holds more items than shown here.",
  downloading: (name, done, total) => `${name}: ${done} of ${total} received`,
  downloaded: (name) => `Downloaded ${name}`,
  memoryOnly:
    "This browser cannot keep a partial download; a large file or an interruption starts the download over.",
  tooLargeHere: "This file is too large to download in this browser.",
  closed:
    "The transfer channel closed; a started transfer can resume in a new connection.",
  problem: (problem) =>
    ({
      unavailable: "The remote computer cannot do this now.",
      denied: "The user's permissions do not allow it.",
      not_found: "Not found.",
      too_large: "Too large.",
      changed: "The file changed since it was listed; open the folder again.",
      no_space: "There is not enough space on the remote computer.",
      empty: "The remote clipboard holds no text.",
      invalid: "The transfer broke off. Start it again.",
    })[problem],
};

export const transferCopy = { hu, en };
