const hu = {
  title: "Távoli asztal",
  surface:
    "Távoli asztal. Kattints bele, hogy az egér és a billentyűzet a távoli gépre menjen.",
  hint: "Az egér és a billentyűzet csak akkor megy a távoli gépre, ha a kép kijelölt.",
  connecting: "Kapcsolódás a távoli asztalhoz…",
  live: "A kép él.",
  unmeasured:
    "A kapcsolat él, de nincs friss mérés; a minőség most nem ellenőrizhető.",
  failed:
    "A kép nem érhető el. A kapcsolat megszakadt vagy nem jött létre; kérj új hozzáférést.",
  closed: "A kép kapcsolata lezárult.",
  quality: (auto: boolean, size: string, rate: string, fps: number) =>
    `${auto ? "Automatikus minőség" : "Kézi minőség"} · ${size} · ${rate} · ${fps.toLocaleString("hu-HU")} kép/s`,
  profile: "Képprofil",
  text: "Szöveg",
  motion: "Mozgás",
};

const en: {
  [Key in keyof typeof hu]: (typeof hu)[Key] extends string
    ? string
    : (typeof hu)[Key];
} = {
  title: "Remote desktop",
  surface:
    "Remote desktop. Click into it to send the mouse and keyboard to the remote computer.",
  hint: "The mouse and keyboard go to the remote computer only while the picture is selected.",
  connecting: "Connecting to the remote desktop…",
  live: "The picture is live.",
  unmeasured:
    "The connection is up, but there is no fresh measurement; quality cannot be checked now.",
  failed:
    "The picture is not available. The connection dropped or was not made; request new access.",
  closed: "The picture connection has closed.",
  quality: (auto, size, rate, fps) =>
    `${auto ? "Automatic quality" : "Manual quality"} · ${size} · ${rate} · ${fps.toLocaleString("en-GB")} fps`,
  profile: "Picture profile",
  text: "Text",
  motion: "Motion",
};

export const viewerCopy = { hu, en };
