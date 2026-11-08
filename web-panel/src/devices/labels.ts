import type { Language } from "../i18n";

export const checkNames: Record<Language, Record<string, string>> = {
  hu: {
    services: "Szolgáltatások",
    updates: "Frissítések",
    antivirus: "Vírusvédelem",
    encryption: "Titkosítás",
    storage: "Tárolók",
  },
  en: {
    services: "Services",
    updates: "Updates",
    antivirus: "Antivirus",
    encryption: "Encryption",
    storage: "Storage",
  },
};

export const reasons: Record<Language, Record<string, string>> = {
  hu: {
    running: "Minden automatikus szolgáltatás fut",
    stopped: "Hibával leállt szolgáltatás",
    current: "Naprakész",
    reboot_pending: "Újraindításra vár",
    outdated: "Régóta nem frissült",
    search_failed: "A frissítéskeresés nem sikerült",
    security_pending: "Biztonsági frissítés vár",
    pending: "Frissítés vár",
    protected: "Védett",
    passive: "Passzív mód, másik vírusvédelem mellett",
    other_product: "Másik vírusvédelem védi",
    disabled: "A védelem ki van kapcsolva",
    realtime_off: "A valós idejű védelem ki van kapcsolva",
    signatures_old: "Elavult vírusdefiníciók",
    encrypted: "Titkosított",
    not_encrypted: "Nem titkosított",
    suspended: "A titkosítás szünetel",
    in_progress: "Titkosítás folyamatban",
    healthy: "Elegendő szabad hely",
    low_space: "Kevés szabad hely",
    disk_warning: "Lemezfigyelmeztetés",
    disk_unhealthy: "Lemezhiba",
    fs_errors: "Fájlrendszerhiba",
    not_supported: "Ezen a rendszeren nem támogatott",
    access_denied: "Az agent nem kap hozzáférést",
    error: "A lekérdezés nem sikerült",
  },
  en: {
    running: "All automatic services are running",
    stopped: "A service stopped with an error",
    current: "Up to date",
    reboot_pending: "Restart pending",
    outdated: "Not updated for a long time",
    search_failed: "Update search failed",
    security_pending: "Security update pending",
    pending: "Update pending",
    protected: "Protected",
    passive: "Passive mode beside another antivirus",
    other_product: "Protected by another antivirus",
    disabled: "Protection is off",
    realtime_off: "Real-time protection is off",
    signatures_old: "Outdated security intelligence",
    encrypted: "Encrypted",
    not_encrypted: "Not encrypted",
    suspended: "Encryption suspended",
    in_progress: "Encryption in progress",
    healthy: "Enough free space",
    low_space: "Low free space",
    disk_warning: "Disk warning",
    disk_unhealthy: "Disk failure",
    fs_errors: "File system errors",
    not_supported: "Not supported on this system",
    access_denied: "The agent has no access",
    error: "The query failed",
  },
};

export const settingNames: Record<Language, Record<string, string>> = {
  hu: {
    "health.low_space_percent": "Kevés szabad hely (%)",
    "health.critical_space_percent": "Kritikusan kevés szabad hely (%)",
    "health.update_warning_days": "Frissítés figyelmeztetés (nap)",
    "health.update_critical_days": "Frissítés kritikus (nap)",
    "health.signature_warning_days": "Vírusdefiníció figyelmeztetés (nap)",
    "health.signature_critical_days": "Vírusdefiníció kritikus (nap)",
    "alerts.offline_minutes": "Offline riasztás késleltetése (perc)",
  },
  en: {
    "health.low_space_percent": "Low free space (%)",
    "health.critical_space_percent": "Critically low free space (%)",
    "health.update_warning_days": "Update warning (days)",
    "health.update_critical_days": "Update critical (days)",
    "health.signature_warning_days": "Security intelligence warning (days)",
    "health.signature_critical_days": "Security intelligence critical (days)",
    "alerts.offline_minutes": "Offline alert delay (minutes)",
  },
};

export const inventoryKinds: Record<Language, Record<string, string>> = {
  hu: {
    hardware: "Hardver",
    os: "Operációs rendszer",
    disk: "Kötetek",
    software: "Szoftverek",
    admin: "Helyi rendszergazdák",
  },
  en: {
    hardware: "Hardware",
    os: "Operating system",
    disk: "Volumes",
    software: "Software",
    admin: "Local administrators",
  },
};

export const inventoryKeys: Record<Language, Record<string, string>> = {
  hu: {
    host_name: "Gépnév",
    cpu: "Processzor",
    logical_cpus: "Logikai processzorok",
    memory_bytes: "Memória",
    name: "Név",
    version: "Verzió",
    long_version: "Teljes név",
    kernel: "Kernel",
  },
  en: {
    host_name: "Host name",
    cpu: "Processor",
    logical_cpus: "Logical processors",
    memory_bytes: "Memory",
    name: "Name",
    version: "Version",
    long_version: "Full name",
    kernel: "Kernel",
  },
};

export function alertTitle(kind: string, language: Language) {
  if (kind === "device.offline")
    return language === "hu" ? "Az eszköz nem elérhető" : "Device offline";
  const check = kind.startsWith("health.") ? kind.slice(7) : kind;
  return checkNames[language][check] ?? kind;
}

export function since(seconds: number | null, language: Language) {
  if (seconds === null) return null;
  const elapsed = Math.round(seconds - Date.now() / 1000);
  const format = new Intl.RelativeTimeFormat(language, { numeric: "auto" });
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  for (const [unit, size] of steps)
    if (Math.abs(elapsed) >= size)
      return format.format(Math.round(elapsed / size), unit);
  return format.format(0, "minute");
}

export function moment(seconds: number | null, language: Language) {
  if (seconds === null) return null;
  return new Intl.DateTimeFormat(language, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
}

export function bytes(value: number, language: Language) {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${new Intl.NumberFormat(language, { maximumFractionDigits: size < 10 ? 1 : 0 }).format(size)} ${units[unit]}`;
}
