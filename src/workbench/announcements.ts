import { invoke } from "@tauri-apps/api/core";

/**
 * 公告读取。
 *
 * 服务端契约（待实现）：bootstrap 增加可选字段 `announcements_path`，
 * 形如 `/api/desktop/v2/notices`；该路径带会话令牌 GET，返回
 * `{"success":true,"data":{"notices":[
 *    {"id":"...","title":"...","body":"...","severity":"info"|"warning",
 *     "publishedAtEpochMs":1758000000000}]}}`
 *
 * 服务端还没宣告这个字段时 `available` 为 false，侧边栏不显示入口。
 * 客户端和服务端谁先发布都不会出错 —— 这正是 bootstrap 契约从
 * `deny_unknown_fields` 改成宽松之后才成立的事。
 */
export type NoticeSeverity = "info" | "warning";

export type Notice = {
  id: string;
  title: string;
  body: string;
  severity: NoticeSeverity;
  publishedAtEpochMs: number;
};

export type AnnouncementsProjection = {
  available: boolean;
  notices: Notice[];
};

const UNAVAILABLE: AnnouncementsProjection = { available: false, notices: [] };

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** 公告是服务端自由文本，渲染前当不可信数据处理（后端已过滤一遍，这里是第二道）。 */
function decodeNotice(value: unknown): Notice | null {
  if (!object(value)) return null;
  const title = typeof value.title === "string" ? value.title : "";
  if (title.length === 0) return null;
  return {
    id: typeof value.id === "string" ? value.id : title,
    title,
    body: typeof value.body === "string" ? value.body : "",
    severity: value.severity === "warning" ? "warning" : "info",
    publishedAtEpochMs:
      typeof value.publishedAtEpochMs === "number" &&
      Number.isFinite(value.publishedAtEpochMs) &&
      value.publishedAtEpochMs >= 0
        ? value.publishedAtEpochMs
        : 0,
  };
}

export async function readAnnouncements(
  lineId: string,
): Promise<AnnouncementsProjection> {
  // 公告读不到只是没有公告。它绝不能影响登录或接入，所以这里吞掉所有错误。
  try {
    const result = await invoke("account_announcements_read_v2", {
      request: { requestId: `announcements-${Date.now()}`, lineId },
    });
    if (!object(result) || result.available !== true) return UNAVAILABLE;
    const notices = Array.isArray(result.notices)
      ? result.notices.map(decodeNotice).filter((n): n is Notice => n !== null)
      : [];
    return { available: true, notices };
  } catch {
    return UNAVAILABLE;
  }
}
