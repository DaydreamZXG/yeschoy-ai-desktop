import type { RequestObservation } from "./connections";
import { groupLabel } from "./BillingGroupPicker";
import type { ActivationToolId } from "./activation";

const outcomes: Record<RequestObservation["outcome"], string> = {
  ok: "已确认经野菜中转完成",
  timeout: "等待模型回复超时",
  network_error: "未能连接模型服务",
  upstream_error: "模型服务未完成请求",
  invalid_response: "模型回复格式异常",
  stream_interrupted: "回复在完成前中断",
  unknown_model: "这个模型尚未加入常用列表",
  payload_too_large: "请求内容超过本机安全上限",
  local_busy: "本机正在处理另一条大请求",
};
export function RecentRequest({
  value,
  toolId,
  onRefresh,
  loading = false,
}: {
  value?: RequestObservation;
  toolId?: ActivationToolId;
  onRefresh?: () => void;
  loading?: boolean;
}) {
  return (
    <section className="recent-request" aria-label="最近连接结果">
      <header>
        <strong>最近野菜中转记录</strong>
        {onRefresh && (
          <button
            type="button"
            className="text-button"
            disabled={loading}
            onClick={onRefresh}
          >
            刷新结果
          </button>
        )}
      </header>
      {value ? (
        <>
          <p className="request-outcome" data-outcome={value.outcome}>
            {outcomes[value.outcome]}
            {value.httpStatus > 0 ? ` · HTTP ${value.httpStatus}` : ""}
          </p>
          <code>{value.modelId || "未指定模型"}</code>
          <small>
            {value.billingGroup ? `${groupLabel(value.billingGroup)} · ` : ""}
            {value.lineId === "global_accelerated"
              ? "全球加速"
              : "大陆优化"} ·{" "}
            {new Date(value.observedAtEpochMs).toLocaleTimeString()}
          </small>
          {value.outcome !== "ok" && (
            <p>
              {value.outcome === "payload_too_large"
                ? "单次请求超过 200 MiB，未发送到上游。请减少一次附带的文件或图片后重试。"
                : value.outcome === "local_busy"
                  ? "为避免桌面助手卡死，本机一次只缓冲一条大请求。请等待当前请求完成后重试。"
                  : [401, 403].includes(value.httpStatus)
                    ? "请检查账户与这个模型的使用权限。"
                    : value.httpStatus === 429
                      ? "请求较多或额度受限，请稍后重试并检查账户。"
                      : value.outcome === "unknown_model"
                        ? "请选用已配置的模型，或将新模型加入列表后更新接入。"
                        : "请先重试；若持续失败，可手动更换线路。不会替你更换模型或计费分组。"}
            </p>
          )}
        </>
      ) : (
        <p>
          尚未收到该应用经野菜中转的请求。发送一条消息后，可在这里刷新确认。
        </p>
      )}
      {toolId === "codex_desktop" && (
        <p>
          Codex
          显示的官方账号是登录身份，不是本次模型请求线路或计费方的证明；这里出现中转记录后，才说明请求进入了野菜本地桥。
        </p>
      )}
      <small>
        记录只会在请求进入野菜本地桥后出现，并显示实际转发的完整模型
        ID；不依据应用缩写或 AI
        的自我介绍判断。只保留本次运行的最近结果，不保存对话。
      </small>
    </section>
  );
}
