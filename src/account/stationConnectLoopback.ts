import { invoke } from "@tauri-apps/api/core";
import type { AccountLineId } from "../configuration/preview";

export interface StationConnectLoopbackCapabilities {
  available: boolean;
  reasonCode: string;
}

export interface StationConnectLoopbackBeginResult {
  started: boolean;
  reasonCode: string;
}

export async function fetchStationConnectLoopbackCapabilities(
  lineId: AccountLineId,
): Promise<StationConnectLoopbackCapabilities> {
  return invoke<StationConnectLoopbackCapabilities>(
    "station_connect_loopback_capabilities_v1",
    { lineId },
  );
}

export async function beginStationConnectLoopbackOauth(
  lineId: AccountLineId,
): Promise<StationConnectLoopbackBeginResult> {
  return invoke<StationConnectLoopbackBeginResult>(
    "station_connect_begin_loopback_oauth_v1",
    { lineId },
  );
}

export async function cancelStationConnectLoopbackOauth(): Promise<void> {
  await invoke("station_connect_cancel_loopback_oauth_v1");
}
