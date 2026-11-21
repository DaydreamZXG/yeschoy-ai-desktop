import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
  decodeStationConnectProgress,
  STATION_CONNECT_PROGRESS_EVENT,
  STATION_TRUST_SYNC_EVENT,
} from "../account/stationConnect";
import type { AccountProjection } from "../account/session";
import { isAccountLineId, type AccountLineId } from "../configuration/preview";

export type LinkEvent =
  | { phase: "connecting" }
  | { phase: "signed_in"; lineId?: string }
  | { phase: "failed"; reasonCode: string | undefined };

/** 本机后台给过明确答复的失败：那次请求已经作废，再点也没用。 */
const TRUST_SETTLED = new Set([
  "connect_code_expired",
  "connect_origin_not_acceptable",
  "trust_store_full",
]);

/** 这次没答成、但问话还留着（本机后台没回话），可以再点一次。 */
export const trustAnswerPending = (failure: string | null) =>
  !!failure && !TRUST_SETTLED.has(failure);

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
  const [arrived, setArrived] = useState<{
    lineId: AccountLineId;
    reported: string | undefined;
  } | null>(null);
  const lineId = options.lineId;
  useEffect(() => {
    if (!arrived || arrived.lineId !== lineId) return;
    setArrived(null);
    void Promise.resolve(latest.current.refresh())
      .catch(() => null)
      .then((projection) =>
        latest.current.onEvent(
          projection?.status === "signed_in"
            ? { phase: "signed_in", lineId: arrived.reported }
            : { phase: "failed", reasonCode: projection?.status },
        ),
      );
  }, [arrived, lineId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen(STATION_CONNECT_PROGRESS_EVENT, (event) => {
      const payload = decodeStationConnectProgress(event.payload);
      if (!payload) return;
      const { lineId, onLineChange, onEvent } = latest.current;
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
        const next =
          payload.lineId && isAccountLineId(payload.lineId)
            ? payload.lineId
            : lineId;
        if (next !== lineId) onLineChange(next);
        // 读账户要等线路真换过去以后（见下面那个 effect）：这一刻手里的
        // `refresh` 读的还是原来那个站。
        setArrived({ lineId: next, reported: payload.lineId });
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
      // 没答成（本机后台没回话）：问话留着，还能再点一次；有了答复才换下一个。
      if (!trustAnswerPending(failure))
        setAsking((queue) => queue.filter((item) => item !== trustOrigin));
      return failure;
    },
    [trustOrigin],
  );

  return { trustOrigin, answerTrust };
}
