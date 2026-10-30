import type {
  AccountLineId,
  ConfigurationLineId,
} from "../configuration/preview";
import { isAccountLineId } from "../configuration/preview";

const STORAGE_KEY = "yeschoy.accountLinePreference";

export function readAccountLinePreference(): AccountLineId | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw || !isAccountLineId(raw)) return null;
    return raw;
  } catch {
    return null;
  }
}

export function writeAccountLinePreference(lineId: AccountLineId): void {
  if (lineId.startsWith("station_connect:")) return;
  try {
    localStorage.setItem(STORAGE_KEY, lineId);
  } catch {
    /* ignore */
  }
}

export function initialAccountLineId(): AccountLineId {
  return readAccountLinePreference() ?? "mainland_optimized";
}

export function catalogLineFromSnapshot(
  storedLineId: string | null | undefined,
): ConfigurationLineId | null {
  if (
    storedLineId === "mainland_optimized" ||
    storedLineId === "global_accelerated"
  ) {
    return storedLineId;
  }
  if (storedLineId === "station_connect:https://yeschoy.com") {
    return "mainland_optimized";
  }
  if (storedLineId === "station_connect:https://api.yeschoy.io") {
    return "global_accelerated";
  }
  return null;
}
