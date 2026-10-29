import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import {
  readCodexProbeEnabled,
  writeCodexProbeEnabled,
} from "../configuration/codexProbe";

/**
 * 设置页里 Codex 传输探测的开关。
 *
 * 为什么要有这个开关：探测会让每个模型多发一次最小生成请求，成功的那次会
 * 真的计费；而产品对用户承诺的是「不发收费的测试请求」。两者的取舍不该由
 * 代码替用户决定，所以默认关闭，把选择交给用户，并在文案里写清代价。
 */
export function CodexProbeSetting() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(readCodexProbeEnabled);
  return (
    <div className="setting-row" data-testid="codex-probe-setting">
      <div className="setting-row-copy">
        <strong>{t("yeschoySettings.codexProbe.title")}</strong>
        <p>{t("yeschoySettings.codexProbe.note")}</p>
        {enabled && (
          <p className="setting-row-warning" data-testid="codex-probe-warning">
            {t("yeschoySettings.codexProbe.warning")}
          </p>
        )}
      </div>
      {enabled ? (
        <button
          type="button"
          className="setting-state"
          aria-pressed="true"
          onClick={() => {
            setEnabled(false);
            writeCodexProbeEnabled(false);
          }}
        >
          <Check aria-hidden="true" />
          {t("yeschoySettings.codexProbe.on")}
        </button>
      ) : (
        <button
          type="button"
          className="setting-state"
          aria-pressed="false"
          onClick={() => {
            setEnabled(true);
            writeCodexProbeEnabled(true);
          }}
        >
          {t("yeschoySettings.codexProbe.off")}
        </button>
      )}
    </div>
  );
}
