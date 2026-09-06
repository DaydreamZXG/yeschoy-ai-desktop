import type { RequestObservation } from "./connections";
import { groupLabel } from "./BillingGroupPicker";

const outcomes: Record<RequestObservation["outcome"], string> = {
  ok: "请求已完成",
  timeout: "等待模型回复超时",
  network_error: "未能连接模型服务",
  upstream_error: "模型服务未完成请求",
  invalid_response: "模型回复格式异常",
  stream_interrupted: "回复在完成前中断",
  unknown_model: "这个模型尚未加入常用列表",
};
export function RecentRequest({
  value,
  onRefresh,
  loading = false,
}: {
  value?: RequestObservation;
  onRefresh?: () => void;
  loading?: boolean;
}) {
  return (
    <section className="recent-request" aria-label="最近连接结果">
      <header>
        <strong>最近连接结果</strong>
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
              {[401, 403].includes(value.httpStatus)
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
        <p>本次启动后暂无已完成的请求记录。发送消息后，可在这里刷新查看。</p>
      )}
      <small>
        显示实际转发的模型 ID，不依据 AI
        的自我介绍判断。只保留本次运行的最近结果，不保存对话。
      </small>
    </section>
  );
}
