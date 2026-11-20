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
  secureDesktop:
    "A távoli gépen a biztonságos asztal látszik, például egy UAC-kérdés. A képet és a bevitelt most a rendszerkomponens kezeli.",
  network: (rtt: number | null, loss: number | null) =>
    `Késleltetés: ${rtt === null ? "nincs mérés" : `${rtt.toLocaleString("hu-HU")} ms`} · csomagvesztés: ${loss === null ? "nincs mérés" : `${(loss * 100).toLocaleString("hu-HU", { maximumFractionDigits: 1 })}%`}`,
  degraded:
    "A hálózat romlott (veszteség, késés vagy ingadozás). A minőség automatikusan csökkent, és javuláskor fokozatosan visszaáll.",
  direct: "Közvetlen kapcsolat",
  relay: "Kapcsolat a relén át",
  keys: "Rendszerbillentyűk",
  start: "Start menü (Win)",
  switchWindow: "Ablakváltás (Alt+Tab)",
  taskManager: "Feladatkezelő (Ctrl+Shift+Esc)",
  close: "Ablak bezárása (Alt+F4)",
  run: "Futtatás (Win+R)",
  desktop: "Asztal megjelenítése (Win+D)",
  secureAttention:
    "A Ctrl+Alt+Del csak UAC-jogú kéréssel, a gép konzol-munkamenetében küldhető, ha a Windows-szabályzat engedi.",
  sas: "Ctrl+Alt+Del",
  sasSent: "A Ctrl+Alt+Del elküldve; a biztonsági képernyő a képen látszik.",
  sasUnavailable:
    "A Ctrl+Alt+Del nem küldhető: konzol-munkamenet, UAC-jog és a Windows „szoftveres SAS” szabályzata kell hozzá.",
  typeText: "Szöveg küldése",
  textHint:
    "Ékezetes betűk, más billentyűzetkiosztás és IME esetén: a szöveg karakterenként érkezik, a távoli kiosztástól függetlenül.",
  send: "Küldés",
  elevatedWindow:
    "Rendszergazdai jogú ablak van elöl vagy a mutató alatt. A bevitelt most a rendszerkomponens kézbesíti.",
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
  secureDesktop:
    "The remote computer shows its secure desktop, for example a UAC prompt. The system component now handles the picture and input.",
  network: (rtt, loss) =>
    `Latency: ${rtt === null ? "not measured" : `${rtt.toLocaleString("en-GB")} ms`} · packet loss: ${loss === null ? "not measured" : `${(loss * 100).toLocaleString("en-GB", { maximumFractionDigits: 1 })}%`}`,
  degraded:
    "The network has degraded (loss, delay or jitter). Quality was lowered automatically and recovers gradually as it improves.",
  direct: "Direct connection",
  relay: "Connection through the relay",
  keys: "System keys",
  start: "Start menu (Win)",
  switchWindow: "Switch window (Alt+Tab)",
  taskManager: "Task Manager (Ctrl+Shift+Esc)",
  close: "Close window (Alt+F4)",
  run: "Run (Win+R)",
  desktop: "Show desktop (Win+D)",
  secureAttention:
    "Ctrl+Alt+Del can be sent only with UAC rights, to the computer's console session, when Windows policy allows it.",
  sas: "Ctrl+Alt+Del",
  sasSent: "Ctrl+Alt+Del was sent; the security screen shows in the picture.",
  sasUnavailable:
    "Ctrl+Alt+Del cannot be sent: it needs the console session, UAC rights and the Windows software SAS policy.",
  typeText: "Send text",
  textHint:
    "For accented letters, other keyboard layouts and IME: the text arrives as characters, whatever the remote layout.",
  send: "Send",
  elevatedWindow:
    "An administrator window is in front or under the pointer. The system component now delivers the input.",
};

export const viewerCopy = { hu, en };
