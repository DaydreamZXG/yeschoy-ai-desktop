/**
 * Fixed station connect page path and HTTPS origin helpers (desktop + docs).
 * Validation mirrors `connect_origin_is_acceptable` in `station_connect.rs`.
 */

export const STATION_CONNECT_PAGE_PATH = "/desktop/connect";

export const STATION_CONNECT_LINE_PREFIX = "station_connect:";

export function stationConnectPageUrl(origin: string): string {
  const base = origin.endsWith("/") ? origin.slice(0, -1) : origin;
  return `${base}${STATION_CONNECT_PAGE_PATH}`;
}

export type StationConnectLineId = `${typeof STATION_CONNECT_LINE_PREFIX}${string}`;

export function lineIdForStationConnectOrigin(origin: string): StationConnectLineId {
  return `${STATION_CONNECT_LINE_PREFIX}${origin}`;
}

function hostIsLiteralIp(host: string): boolean {
  const bare = host.startsWith("[") && host.endsWith("]") ? host.slice(1, -1) : host;
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(bare)) return true;
  if (bare.includes(":")) return true;
  return false;
}

/**
 * Normalize user input (domain or full HTTPS URL) to canonical station origin.
 * Returns null when the value cannot be used for station connect.
 */
export function canonicalStationHttpsOrigin(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  let candidate = trimmed;
  if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(candidate)) {
    candidate = `https://${candidate}`;
  }

  let url: URL;
  try {
    url = new URL(candidate);
  } catch {
    return null;
  }

  if (url.protocol !== "https:" || url.port) return null;
  if (url.username || url.password) return null;
  if (url.search || url.hash) return null;

  const host = url.hostname;
  if (!host || hostIsLiteralIp(host)) return null;

  return `https://${host.toLowerCase()}`;
}
