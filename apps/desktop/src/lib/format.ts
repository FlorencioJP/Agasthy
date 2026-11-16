import type { Availability } from "@agasthy/shared-types";

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Binary-unit byte formatting (1 GB = 1024³ bytes), as macOS Activity Monitor does. */
export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(v >= 100 ? 0 : digits)} ${UNITS[i]}`;
}

export function formatPercent(v: number, digits = 0): string {
  return `${v.toFixed(digits)}%`;
}

export function formatNumber(n: number): string {
  return new Intl.NumberFormat().format(n);
}

export function formatRelative(ms: number, now: number = Date.now()): string {
  const diff = Math.round((now - ms) / 1000);
  const abs = Math.abs(diff);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  if (abs < 45) return diff >= 0 ? "just now" : "in a moment";
  const table: [number, Intl.RelativeTimeFormatUnit][] = [
    [60, "second"],
    [3600, "minute"],
    [86400, "hour"],
    [604800, "day"],
    [2629800, "week"],
    [31557600, "month"],
    [Infinity, "year"],
  ];
  const divisors: Record<string, number> = {
    second: 1,
    minute: 60,
    hour: 3600,
    day: 86400,
    week: 604800,
    month: 2629800,
    year: 31557600,
  };
  for (const [limit, unit] of table) {
    if (abs < limit) return rtf.format(-Math.round(diff / divisors[unit]), unit);
  }
  return new Date(ms).toLocaleDateString();
}

export function formatDateTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatDuration(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function value<T>(a: Availability<T> | undefined | null): T | null {
  return a && a.status === "available" ? a.value : null;
}

/** Human explanation for a non-available value. */
export function unavailableReason<T>(a: Availability<T>): string {
  switch (a.status) {
    case "available":
      return "";
    case "unavailable":
      return a.reason;
    case "permission_denied":
      return `Permission denied: ${a.detail}`;
    case "unsupported":
      return `Not supported on ${a.platform} in this build`;
    default:
      return "";
  }
}

export function shortenHome(path: string, home?: string | null): string {
  if (home && path.startsWith(home + "/")) return "~" + path.slice(home.length);
  const m = /^\/Users\/[^/]+(\/.*)?$/.exec(path);
  if (m) return "~" + (m[1] ?? "");
  return path;
}
