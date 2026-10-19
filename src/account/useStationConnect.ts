import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { isAccountLineId, type AccountLineId } from "../configuration/preview";
import type { AccountProjection } from "./session";
import {
  decodeStationConnectProgress,
  STATION_CONNECT_PROGRESS_EVENT,
} from "./stationConnect";

const TOAST_ID = "station-connect-progress";

function connectFailureMessage(
  reasonCode: string | undefined,
  translate: (key: string) => string,
): string {
  if (!reasonCode) return translate("stationConnect.failedGeneric");
  const key = `stationConnect.reason.${reasonCode}`;
  const localized = translate(key);
  return localized === key
    ? translate("stationConnect.failedGeneric")
    : localized;
}

export function useStationConnect(options: {
  lineId: AccountLineId;
  onLineChange: (lineId: AccountLineId) => void;
  refresh: () => Promise<AccountProjection | null>;
  onSignedIn?: () => void;
}) {
  const { t } = useTranslation();
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen(STATION_CONNECT_PROGRESS_EVENT, (event) => {
      const payload = decodeStationConnectProgress(event.payload);
      if (!payload) return;
      const { lineId, onLineChange, refresh, onSignedIn } = optionsRef.current;

      // 第一次见到的站点由 StationTrustDialog 负责询问，这里不弹提示。
      if (payload.phase === "confirm_required") return;

      if (payload.phase === "connecting") {
        toast.loading(t("stationConnect.connecting"), { id: TOAST_ID });
        return;
      }

      toast.dismiss(TOAST_ID);

      if (payload.phase === "signed_in") {
        if (payload.lineId && isAccountLineId(payload.lineId)) {
          if (payload.lineId !== lineId) onLineChange(payload.lineId);
        }
        void refresh().then((projection) => {
          if (projection?.status === "signed_in") {
            toast.success(t("stationConnect.signedIn"));
            onSignedIn?.();
          } else if (projection) {
            toast.message(t("stationConnect.inspectRetry"), {
              description: t("stationConnect.inspectRetryBody"),
            });
          }
        });
        return;
      }

      if (payload.phase === "failed" || payload.phase === "rejected") {
        const description = connectFailureMessage(payload.reasonCode, t);
        if (payload.recoverable) {
          toast.error(t("stationConnect.failedRecoverable"), {
            description,
            action: {
              label: t("workbench.retry"),
              onClick: () => void refresh(),
            },
          });
        } else {
          toast.error(t("stationConnect.failed"), { description });
        }
      }
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });

    return () => {
      disposed = true;
      unlisten?.();
      toast.dismiss(TOAST_ID);
    };
  }, [t]);
}
