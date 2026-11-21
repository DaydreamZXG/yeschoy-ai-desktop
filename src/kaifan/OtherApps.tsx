import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { AccountLineId } from "../configuration/preview";
import { modelDisplayName } from "../model-profiles/profile";
import { originOfLine } from "./checkup";
import { addressesOf, vendorName, type ProviderPreset } from "./providers";

interface Props {
  lineId: AccountLineId;
  preset: ProviderPreset | null;
  /** 现在选的那位的型号。别的应用里「模型」那一栏填它。 */
  modelId: string;
  /** 是不是粘贴密钥连的：是的话密钥就在用户手里。 */
  viaKey: boolean;
}

/**
 * 名单之外的应用：客户端不认识它的设置文件，没法替用户改。把要填的三样东西
 * 给出来，一键复制。密钥不从密码库里读出来显示，只说去哪拿。
 */
export function OtherApps({ lineId, preset, modelId, viaKey }: Props) {
  const { t } = useTranslation();
  const address = addressesOf(originOfLine(lineId), preset);
  const [copied, setCopied] = useState<{ what: string; ok: boolean } | null>(
    null,
  );
  const copy = async (what: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied({ what, ok: true });
    } catch {
      setCopied({ what, ok: false });
    }
  };
  const rows = [
    { what: "openai", value: address?.openai ?? "" },
    { what: "anthropic", value: address?.anthropic ?? "" },
    { what: "model", value: modelId },
  ].filter((row) => row.value);

  return (
    <div className="oth" data-testid="kaifan-join-other">
      <p className="hint">{t("kaifan.join.other.intro")}</p>
      {address ? (
        <dl>
          {rows.map((row) => (
            <div key={row.what}>
              <dt>{t(`kaifan.join.other.${row.what}`)}</dt>
              <dd>
                <code title={row.value}>{row.value}</code>
                {row.what === "model" &&
                  modelDisplayName(row.value) !== row.value && (
                    <small>{modelDisplayName(row.value)}</small>
                  )}
                <button
                  type="button"
                  className="sm"
                  onClick={() => void copy(row.what, row.value)}
                >
                  {t(
                    copied?.what === row.what && copied.ok
                      ? "kaifan.join.other.copied"
                      : "kaifan.join.other.copy",
                  )}
                </button>
              </dd>
            </div>
          ))}
          <div>
            <dt>{t("kaifan.join.other.key")}</dt>
            <dd>
              {t(
                viaKey
                  ? "kaifan.join.other.keyYours"
                  : "kaifan.join.other.keyFromStation",
              )}
            </dd>
          </div>
        </dl>
      ) : (
        <p className="mend" role="note">
          {t("kaifan.join.other.noAddress")}
        </p>
      )}
      {address && preset?.sessionHeader && (
        <p className="mend" role="note" data-testid="kaifan-join-other-header">
          {t("kaifan.join.other.sessionHeader", {
            vendor: vendorName(preset, t),
            header: preset.sessionHeader,
          })}
        </p>
      )}
      {copied && !copied.ok && (
        <p className="mend" role="alert">
          {t("kaifan.join.other.copyFailed")}
        </p>
      )}
      {preset?.billing === "plan" && (
        <p className="mend" role="note" data-testid="kaifan-join-other-risk">
          {t("kaifan.join.other.planRisk", { vendor: vendorName(preset, t) })}
        </p>
      )}
    </div>
  );
}
