import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
  decodeStationConnectProgress,
  STATION_CONNECT_PROGRESS_EVENT,
} from "../account/stationConnect";
import { STATION_TRUST_SYNC_EVENT } from "../account/StationTrustDialog";
import type { AccountProjection } from "../account/session";
import { isAccountLineId, type AccountLineId } from "../configuration/preview";

export type LinkEvent =
  | { phase: "connecting" }
  | { phase: "signed_in"; lineId?: string }
  | { phase: "failed"; reasonCode: string | undefined };

/**
 * 站点在浏览器里授权完成后，原生侧会发进度过来。这里只负责听、把线路切过去、
 * 把结果交给界面；怎么说由角色决定，不弹提示条。
 *
 * 第一次见到的站点（网页拉起客户端时）先排队，等用户答一句再连。
 */
export function useStationLink(options: {
  lineId: AccountLineId;
  onLineChange: (lineId: AccountLineId) => void;
  refresh: () => Promise<AccountProjection | null>;
  onEvent: (event: LinkEvent) => void;
}) {
  const latest = useRef(options);
  latest.current = options;
  const [asking, setAsking] = useState<string[]>([]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen(STATION_CONNECT_PROGRESS_EVENT, (event) => {
      const payload = decodeStationConnectProgress(event.payload);
      if (!payload) return;
      const { lineId, onLineChange, refresh, onEvent } = latest.current;
      if (payload.phase === "confirm_required") {
        const origin = payload.origin;
        if (origin)
          setAsking((queue) =>
            queue.includes(origin) ? queue : [...queue, origin],
          );
        return;
      }
      if (payload.phase === "connecting") {
        onEvent({ phase: "connecting" });
        return;
      }
      if (payload.phase === "signed_in") {
        if (
          payload.lineId &&
          isAccountLineId(payload.lineId) &&
          payload.lineId !== lineId
        )
          onLineChange(payload.lineId);
        void refresh().then((projection) =>
          latest.current.onEvent(
            projection?.status === "signed_in"
              ? { phase: "signed_in", lineId: payload.lineId }
              : { phase: "failed", reasonCode: projection?.status },
          ),
        );
        return;
      }
      onEvent({ phase: "failed", reasonCode: payload.reasonCode });
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      // 冷启动时链接可能比界面先到：监听就绪后请原生侧把待确认的站点重发一次。
      .then(() => emit(STATION_TRUST_SYNC_EVENT))
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const trustOrigin = asking[0] ?? null;
  const answerTrust = useCallback(
    async (accept: boolean): Promise<string | null> => {
      if (!trustOrigin) return null;
      let failure: string | null = null;
      try {
        await invoke("station_connect_confirm_origin_v1", {
          origin: trustOrigin,
          accept,
        });
      } catch (error) {
        failure = String(error);
      }
      setAsking((queue) => queue.slice(1));
      return failure;
    },
    [trustOrigin],
  );

  return { trustOrigin, answerTrust };
}
