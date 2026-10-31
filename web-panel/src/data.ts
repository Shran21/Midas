export type DeviceStatus = "online" | "attention" | "offline";
export type Device = {
  id: string;
  name: string;
  kind: "server" | "workstation";
  os: string;
  family: "windows" | "linux";
  customer: string;
  site: string;
  ip: string;
  user: string;
  status: DeviceStatus;
  cpu: number | null;
  memory: number | null;
  disk: number | null;
  updates: number;
  restart: boolean;
  seenMinutes: number;
};

export const devices: Device[] = [
  {
    id: "file01",
    name: "SRV-FILE-01",
    kind: "server",
    os: "Windows Server 2022",
    family: "windows",
    customer: "Norda Kft.",
    site: "Budapest",
    ip: "10.10.1.12",
    user: "—",
    status: "attention",
    cpu: 23,
    memory: 67,
    disk: 92,
    updates: 1,
    restart: false,
    seenMinutes: 0,
  },
  {
    id: "bud14",
    name: "WS-BUD-014",
    kind: "workstation",
    os: "Windows 11 Pro",
    family: "windows",
    customer: "Norda Kft.",
    site: "Budapest",
    ip: "10.10.2.14",
    user: "kovacs.anna",
    status: "attention",
    cpu: 16,
    memory: 62,
    disk: 57,
    updates: 5,
    restart: true,
    seenMinutes: 0,
  },
  {
    id: "ad01",
    name: "SRV-AD-01",
    kind: "server",
    os: "Windows Server 2022",
    family: "windows",
    customer: "Norda Kft.",
    site: "Budapest",
    ip: "10.10.1.10",
    user: "—",
    status: "online",
    cpu: 12,
    memory: 42,
    disk: 34,
    updates: 0,
    restart: false,
    seenMinutes: 0,
  },
  {
    id: "lnx01",
    name: "SRV-LNX-01",
    kind: "server",
    os: "Ubuntu 24.04 LTS",
    family: "linux",
    customer: "Atlasz Logisztika",
    site: "Győr",
    ip: "10.20.1.20",
    user: "—",
    status: "online",
    cpu: 6,
    memory: 28,
    disk: 47,
    updates: 0,
    restart: false,
    seenMinutes: 0,
  },
  {
    id: "bud21",
    name: "WS-BUD-021",
    kind: "workstation",
    os: "Windows 11 Pro",
    family: "windows",
    customer: "Norda Kft.",
    site: "Budapest",
    ip: "10.10.2.21",
    user: "nagy.peter",
    status: "online",
    cpu: 18,
    memory: 54,
    disk: 32,
    updates: 0,
    restart: false,
    seenMinutes: 0,
  },
  {
    id: "bud32",
    name: "WS-BUD-032",
    kind: "workstation",
    os: "Windows 11 Pro",
    family: "windows",
    customer: "Vertex Studio",
    site: "Budapest",
    ip: "10.30.2.32",
    user: "toth.dora",
    status: "online",
    cpu: 8,
    memory: 45,
    disk: 24,
    updates: 3,
    restart: false,
    seenMinutes: 0,
  },
  {
    id: "gyr03",
    name: "WS-GYR-003",
    kind: "workstation",
    os: "Windows 11 Pro",
    family: "windows",
    customer: "Atlasz Logisztika",
    site: "Győr",
    ip: "10.20.2.3",
    user: "szabo.marton",
    status: "offline",
    cpu: null,
    memory: null,
    disk: null,
    updates: 2,
    restart: false,
    seenMinutes: 48,
  },
  {
    id: "deb08",
    name: "WS-DEB-008",
    kind: "workstation",
    os: "Ubuntu 24.04 LTS",
    family: "linux",
    customer: "Vertex Studio",
    site: "Debrecen",
    ip: "10.30.3.8",
    user: "varga.adam",
    status: "online",
    cpu: 26,
    memory: 71,
    disk: 61,
    updates: 0,
    restart: false,
    seenMinutes: 0,
  },
];

export const alerts = [
  {
    id: "storage",
    deviceId: "file01",
    severity: "critical",
    title: { hu: "Kevés szabad lemezterület", en: "Low disk space" },
    detail: {
      hu: "A rendszermeghajtó 92%-ban foglalt.",
      en: "The system drive is 92% full.",
    },
  },
  {
    id: "restart",
    deviceId: "bud14",
    severity: "warning",
    title: { hu: "Újraindítás szükséges", en: "Restart required" },
    detail: {
      hu: "A biztonsági frissítések befejezésére vár.",
      en: "Waiting to complete security updates.",
    },
  },
  {
    id: "connection",
    deviceId: "gyr03",
    severity: "warning",
    title: { hu: "Az eszköz nem elérhető", en: "Device is offline" },
    detail: {
      hu: "48 perce nem érkezett állapotjelzés.",
      en: "No status update for 48 minutes.",
    },
  },
] as const;
