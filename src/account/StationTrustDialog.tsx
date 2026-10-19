import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ShieldAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  decodeStationConnectProgress,
  STATION_CONNECT_PROGRESS_EVENT,
} from "./stationConnect";

/** 显示用：只保留域名，避免长网址把「真正要看的那一段」挤出去。 */
export const STATION_TRUST_SYNC_EVENT = "yeschoy://station-trust-sync";

export function stationHost(origin: string): string {
  try {
    return new URL(origin).host;
  } catch {
    return origin;
  }
}

/**
 * 网页点「打开桌面助手」拉起 yeschoy://connect，而这个站点是第一次见：
 * 先停下来问一句。小白分不清钓鱼站和自己的中转站，所以文案只讲两件事 ——
 * 这是谁、连上之后会发生什么。默认焦点放在「取消」上。
 */
export function StationTrustDialog() {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [queue, setQueue] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const origin = queue[0];

  const enqueue = useCallback((next: string) => {
    setQueue((current) =>
      current.includes(next) ? current : [...current, next],
    );
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen(STATION_CONNECT_PROGRESS_EVENT, (event) => {
      const payload = decodeStationConnectProgress(event.payload);
      if (payload?.phase === "confirm_required" && payload.origin)
        enqueue(payload.origin);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    })
    // 冷启动：链接可能在界面加载完之前就到了。监听就绪后请原生侧重发一次
    // 待确认的站点（用事件而不是命令，不打扰其他依赖命令顺序的流程）。
    .then(() => emit(STATION_TRUST_SYNC_EVENT))
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [enqueue]);

  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    if (origin && !node.open) node.showModal?.();
    if (!origin && node.open) node.close();
  }, [origin]);

  const answer = async (accept: boolean) => {
    if (!origin) return;
    setBusy(true);
    try {
      await invoke("station_connect_confirm_origin_v1", { origin, accept });
    } catch (error) {
      const code = String(error);
      toast.error(t("stationTrust.failed"), {
        description: t(`stationTrust.reason.${code}`, {
          defaultValue: t("stationTrust.reason.generic"),
        }),
      });
    } finally {
      setBusy(false);
      setQueue((current) => current.slice(1));
    }
  };

  return (
    <dialog
      ref={dialog}
      className="restore-dialog station-trust-dialog"
      aria-labelledby="station-trust-title"
      onCancel={(event) => {
        event.preventDefault();
        void answer(false);
      }}
    >
      {origin && (
        <>
          <header>
            <h2 id="station-trust-title">
              <ShieldAlert aria-hidden="true" />
              {t("stationTrust.title")}
            </h2>
          </header>
          <p className="station-trust-host">
            <strong>{stationHost(origin)}</strong>
          </p>
          <p>{t("stationTrust.body")}</p>
          <ul className="station-trust-points">
            <li>{t("stationTrust.point1")}</li>
            <li>{t("stationTrust.point2")}</li>
          </ul>
          <p className="station-trust-warn">{t("stationTrust.warn")}</p>
          <footer>
            <button
              autoFocus
              disabled={busy}
              onClick={() => void answer(false)}
            >
              {t("stationTrust.decline")}
            </button>
            <button
              className="primary-action"
              disabled={busy}
              onClick={() => void answer(true)}
            >
              {t("stationTrust.accept")}
            </button>
          </footer>
        </>
      )}
    </dialog>
  );
}
